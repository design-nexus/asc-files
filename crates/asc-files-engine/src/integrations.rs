// SPDX-License-Identifier: MIT
//! Making Files the desktop's file chooser and its file manager, for the user (legacy's
//! `portal_setup.rs`, without its Strata and Omarchy migrations): the portal's preference in
//! `xdg-desktop-portal/*portals.conf` and the bus activation files in the user's data folder,
//! each undone exactly, keeping what the user changed meanwhile. Only an executable no other
//! user can replace is ever named for activation.

use std::{
    env, fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const FILE_CHOOSER_KEY: &str = "org.freedesktop.impl.portal.FileChooser";
const PORTAL_NAME: &str = "asc_files";
const PORTAL_FILE: &str = "asc_files.portal";
const SERVICE_FILE: &str = "org.freedesktop.impl.portal.desktop.asc_files.service";
const STATE_DIRECTORY: &str = "asc-files/portal-install";
const STATE_FILE: &str = "state.toml";
pub const DESKTOP_ID: &str = "asc-files.desktop";
const FILE_MANAGER_SERVICE: &str = "asc-files.FileManager1.service";
const FILE_MANAGER_STATE_DIRECTORY: &str = "asc-files/file-manager-install";
const PORTAL_DESCRIPTION: &str = "[portal]\nDBusName=org.freedesktop.impl.portal.desktop.asc_files\nInterfaces=org.freedesktop.impl.portal.FileChooser;\n";

/// Where the user's and the system's desktop files are.
#[derive(Clone, Debug)]
pub struct Context {
    pub data_home: PathBuf,
    pub config_home: PathBuf,
    /// Where `xdg-desktop-portal/*.conf` is looked for, first first.
    pub search_roots: Vec<PathBuf>,
    /// The desktop's own configuration names, then `portals.conf`.
    pub config_names: Vec<String>,
}

impl Context {
    pub fn from_environment() -> Result<Self, String> {
        let home = env::var_os("HOME").filter(|value| !value.is_empty());
        let data_home = xdg_home("XDG_DATA_HOME", home.as_deref(), ".local/share")?;
        let config_home = xdg_home("XDG_CONFIG_HOME", home.as_deref(), ".config")?;
        let mut search_roots = vec![config_home.clone()];
        search_roots.extend(xdg_directories("XDG_CONFIG_DIRS", "/etc/xdg"));
        search_roots.push(PathBuf::from("/etc"));
        search_roots.push(data_home.clone());
        search_roots.extend(xdg_directories(
            "XDG_DATA_DIRS",
            "/usr/local/share:/usr/share",
        ));
        let mut config_names: Vec<String> = env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter_map(portal_config_name)
            .collect();
        config_names.push("portals.conf".to_owned());
        config_names.dedup();
        Ok(Self {
            data_home,
            config_home,
            search_roots,
            config_names,
        })
    }

    fn portal_directory(&self) -> PathBuf {
        self.config_home.join("xdg-desktop-portal")
    }

    fn state_directory(&self) -> PathBuf {
        self.data_home.join(STATE_DIRECTORY)
    }

    fn services(&self) -> PathBuf {
        self.data_home.join("dbus-1/services")
    }
}

fn xdg_home(
    variable: &str,
    home: Option<&std::ffi::OsStr>,
    fallback: &str,
) -> Result<PathBuf, String> {
    let path = env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(Path::new).map(|path| path.join(fallback)))
        .ok_or_else(|| format!("Neither {variable} nor HOME is set"))?;
    path.is_absolute()
        .then_some(path)
        .ok_or_else(|| format!("{variable} must resolve to an absolute path"))
}

fn xdg_directories(variable: &str, default: &str) -> Vec<PathBuf> {
    let value = env::var_os(variable)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.into());
    env::split_paths(&value)
        .filter(|path| path.is_absolute())
        .collect()
}

fn portal_config_name(desktop: &str) -> Option<String> {
    let desktop = desktop.trim().to_ascii_lowercase();
    (!desktop.is_empty()
        && desktop
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then(|| format!("{desktop}-portals.conf"))
}

/// A key file kept line by line, so comments and everything Files doesn't touch stay as they were.
struct KeyFile {
    lines: Vec<String>,
}

impl KeyFile {
    fn parse(contents: &str) -> Result<Self, String> {
        for line in contents.lines() {
            let trimmed = line.trim();
            if !(trimmed.is_empty()
                || trimmed.starts_with('#')
                || (trimmed.starts_with('[') && trimmed.ends_with(']'))
                || trimmed.contains('='))
            {
                return Err(format!("Could not parse portal configuration: “{trimmed}”"));
            }
        }
        Ok(Self {
            lines: contents.lines().map(str::to_owned).collect(),
        })
    }

    /// The line range of `[section]`'s keys, if it is there.
    fn section(&self, section: &str) -> Option<(usize, usize)> {
        let header = format!("[{section}]");
        let start = self.lines.iter().position(|line| line.trim() == header)? + 1;
        let end = self.lines[start..]
            .iter()
            .position(|line| line.trim().starts_with('['))
            .map_or(self.lines.len(), |offset| start + offset);
        Some((start, end))
    }

    fn key_line(&self, section: &str, key: &str) -> Option<usize> {
        let (start, end) = self.section(section)?;
        (start..end).find(|&index| {
            self.lines[index].split_once('=').is_some_and(|(name, _)| {
                name.trim() == key && !self.lines[index].trim_start().starts_with('#')
            })
        })
    }

    fn value(&self, section: &str, key: &str) -> Option<String> {
        let line = &self.lines[self.key_line(section, key)?];
        line.split_once('=')
            .map(|(_, value)| value.trim().to_owned())
    }

    fn set(&mut self, section: &str, key: &str, value: &str) {
        let line = format!("{key}={value}");
        if let Some(index) = self.key_line(section, key) {
            self.lines[index] = line;
        } else if let Some((_, end)) = self.section(section) {
            // After the section's last key, before blank lines that part it from the next.
            let mut at = end;
            while at > 0 && self.lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            self.lines.insert(at, line);
        } else {
            if self
                .lines
                .last()
                .is_some_and(|last| !last.trim().is_empty())
            {
                self.lines.push(String::new());
            }
            self.lines.push(format!("[{section}]"));
            self.lines.push(line);
        }
    }

    fn remove(&mut self, section: &str, key: &str) {
        if let Some(index) = self.key_line(section, key) {
            self.lines.remove(index);
        }
    }

    fn data(&self) -> String {
        let mut text = self.lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        text
    }
}

fn backend_values(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `contents` with Files first for the file chooser (`enable`), or taken out of it; the
/// backends already chosen stay after it, as the fallbacks they were.
pub fn update_config(contents: &str, enable: bool) -> Result<String, String> {
    let mut config = KeyFile::parse(contents)?;
    let chooser = config.value("preferred", FILE_CHOOSER_KEY);
    if chooser.is_none() && !enable {
        return Ok(contents.to_owned());
    }
    let mut fallback = config
        .value("preferred", "default")
        .map_or_else(|| vec!["*".to_owned()], |value| backend_values(&value));
    fallback.retain(|value| value != PORTAL_NAME);
    if fallback.is_empty() {
        fallback.push("*".to_owned());
    }
    let mut values = chooser
        .as_deref()
        .map_or_else(|| fallback.clone(), backend_values);
    values.retain(|value| value != PORTAL_NAME);
    if enable {
        if values.is_empty() {
            values = fallback;
        }
        values.insert(0, PORTAL_NAME.to_owned());
        config.set(
            "preferred",
            FILE_CHOOSER_KEY,
            &format!("{};", values.join(";")),
        );
    } else if values.is_empty() || values == fallback {
        config.remove("preferred", FILE_CHOOSER_KEY);
    } else {
        config.set(
            "preferred",
            FILE_CHOOSER_KEY,
            &format!("{};", values.join(";")),
        );
    }
    Ok(config.data())
}

/// What installing changed, to undo it exactly.
#[derive(Debug, Deserialize, Serialize)]
struct InstallState {
    target_name: String,
    target_existed: bool,
    original: String,
    installed: String,
}

/// Whether Files is the desktop's file chooser, and whether it can be (its activation files are
/// there).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChooserStatus {
    pub preferred: bool,
    pub activatable: bool,
}

/// Makes Files the file chooser: its portal description and activation, and its name first in the
/// active configuration (copied to the user's when it is the system's). Returns the file changed.
pub fn install_chooser(context: &Context, executable: &Path) -> Result<PathBuf, String> {
    let executable = activation_executable(executable)?;
    let config_directory = context.portal_directory();
    let state_directory = context.state_directory();
    let (target, state, installed) = if let Some(state) = read_state(&state_directory)? {
        let target = config_directory.join(&state.target_name);
        ensure_regular_or_missing(&target)?;
        let current = if target.exists() {
            read_utf8(&target)?
        } else {
            state.installed.clone()
        };
        (target, None, update_config(&current, true)?)
    } else {
        let source = active_config(context);
        let target_name = source
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("portals.conf")
            .to_owned();
        let target = config_directory.join(&target_name);
        ensure_regular_or_missing(&target)?;
        let target_existed = target.exists();
        let original = target_existed.then(|| read_utf8(&target)).transpose()?;
        let base = match &original {
            Some(contents) => contents.clone(),
            None => source
                .as_ref()
                .map_or_else(|| Ok(String::new()), |path| read_utf8(path))?,
        };
        let original = original
            .as_deref()
            .map(|contents| update_config(contents, false))
            .transpose()?
            .unwrap_or_default();
        let installed = update_config(&base, true)?;
        (
            target,
            Some(InstallState {
                target_name,
                target_existed,
                original,
                installed: installed.clone(),
            }),
            installed,
        )
    };
    fs::create_dir_all(&config_directory)
        .map_err(|error| path_error("create", &config_directory, &error))?;
    let portals = context.data_home.join("xdg-desktop-portal/portals");
    for directory in [&portals, &context.services()] {
        fs::create_dir_all(directory).map_err(|error| path_error("create", directory, &error))?;
    }
    write_file(
        &portals.join(PORTAL_FILE),
        PORTAL_DESCRIPTION.as_bytes(),
        0o644,
    )?;
    let service = format!(
        "[D-BUS Service]\nName=org.freedesktop.impl.portal.desktop.asc_files\nExec={executable} --chooser\n"
    );
    write_file(
        &context.services().join(SERVICE_FILE),
        service.as_bytes(),
        0o644,
    )?;
    if let Some(state) = state {
        write_state(&state_directory, &state)?;
    }
    write_file(&target, installed.as_bytes(), file_mode(&target))?;
    Ok(target)
}

/// Undoes `install_chooser`. A configuration the user changed since keeps their changes, with only
/// Files taken out (returned as true).
pub fn uninstall_chooser(context: &Context) -> Result<bool, String> {
    let state_directory = context.state_directory();
    let mut preserved_edits = false;
    if let Some(state) = read_state(&state_directory)? {
        let target = context.portal_directory().join(&state.target_name);
        if target.exists() {
            let current = read_utf8(&target)?;
            if current == state.installed {
                if state.target_existed {
                    write_file(&target, state.original.as_bytes(), file_mode(&target))?;
                } else {
                    remove_if_exists(&target)?;
                }
            } else {
                let disabled = update_config(&current, false)?;
                write_file(&target, disabled.as_bytes(), file_mode(&target))?;
                preserved_edits = true;
            }
        }
        remove_state(&state_directory)?;
    } else {
        preserved_edits = disable_user_configs(context)?;
    }
    remove_if_exists(
        &context
            .data_home
            .join("xdg-desktop-portal/portals")
            .join(PORTAL_FILE),
    )?;
    remove_if_exists(&context.services().join(SERVICE_FILE))?;
    Ok(preserved_edits)
}

pub fn chooser_status(context: &Context) -> Result<ChooserStatus, String> {
    let preferred = match active_config(context) {
        // The chooser's own preference, else the default backends.
        Some(path) => {
            let config = KeyFile::parse(&read_utf8(&path)?)?;
            config
                .value("preferred", FILE_CHOOSER_KEY)
                .or_else(|| config.value("preferred", "default"))
                .is_some_and(|value| {
                    backend_values(&value).first().map(String::as_str) == Some(PORTAL_NAME)
                })
        }
        None => false,
    };
    let activatable = context
        .data_home
        .join("xdg-desktop-portal/portals")
        .join(PORTAL_FILE)
        .is_file()
        && context.services().join(SERVICE_FILE).is_file();
    Ok(ChooserStatus {
        preferred,
        activatable,
    })
}

fn active_config(context: &Context) -> Option<PathBuf> {
    context.search_roots.iter().find_map(|root| {
        context.config_names.iter().find_map(|name| {
            let path = root.join("xdg-desktop-portal").join(name);
            path.is_file().then_some(path)
        })
    })
}

fn disable_user_configs(context: &Context) -> Result<bool, String> {
    let directory = context.portal_directory();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(path_error("read", &directory, &error)),
    };
    let mut changed = false;
    for entry in entries {
        let path = entry
            .map_err(|error| path_error("read", &directory, &error))?
            .path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if config_name_is_unsafe(name) || !path.is_file() {
            continue;
        }
        let current = read_utf8(&path)?;
        let disabled = update_config(&current, false)?;
        if current != disabled {
            write_file(&path, disabled.as_bytes(), file_mode(&path))?;
            changed = true;
        }
    }
    Ok(changed)
}

fn config_name_is_unsafe(name: &str) -> bool {
    name.is_empty()
        || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
        || !(name == "portals.conf" || name.ends_with("-portals.conf"))
}

fn write_state(directory: &Path, state: &InstallState) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| path_error("create", directory, &error))?;
    let contents = toml::to_string(state).map_err(|error| error.to_string())?;
    write_file(&directory.join(STATE_FILE), contents.as_bytes(), 0o600)
}

fn read_state(directory: &Path) -> Result<Option<InstallState>, String> {
    let path = directory.join(STATE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let state: InstallState = toml::from_str(&read_utf8(&path)?)
        .map_err(|error| format!("Could not read portal installation state: {error}"))?;
    if config_name_is_unsafe(&state.target_name) {
        return Err(crate::i18n::tr(
            "The saved portal configuration filename is invalid",
        ));
    }
    Ok(Some(state))
}

fn remove_state(directory: &Path) -> Result<(), String> {
    remove_if_exists(&directory.join(STATE_FILE))?;
    match fs::remove_dir(directory) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(path_error("remove", directory, &error)),
    }
}

/// Whether Files is the default folder handler and answers FileManager1 when not running.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileManagerStatus {
    pub default: bool,
    pub has_service: bool,
    pub has_installation: bool,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct FileManagerState {
    previous_default: Option<String>,
}

/// Starts Files for FileManager1 calls, recording `previous` (the folder handler before) so
/// uninstalling can restore it. Refused where another per-user provider is installed.
pub fn install_file_manager(
    context: &Context,
    executable: &Path,
    previous: Option<&str>,
) -> Result<(), String> {
    let executable = activation_executable(executable)?;
    let services = context.services();
    fs::create_dir_all(&services).map_err(|error| path_error("create", &services, &error))?;
    if let Some(conflict) = file_manager_conflict(&services)? {
        return Err(format!(
            "Another per-user FileManager1 provider is already installed: {conflict}"
        ));
    }
    let state_directory = context.data_home.join(FILE_MANAGER_STATE_DIRECTORY);
    if read_file_manager_state(&state_directory)?.is_none() {
        fs::create_dir_all(&state_directory)
            .map_err(|error| path_error("create", &state_directory, &error))?;
        let state = FileManagerState {
            previous_default: previous.map(str::to_owned),
        };
        let contents = toml::to_string(&state).map_err(|error| error.to_string())?;
        write_file(
            &state_directory.join(STATE_FILE),
            contents.as_bytes(),
            0o600,
        )?;
    }
    let service =
        format!("[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec={executable}\n");
    write_file(
        &services.join(FILE_MANAGER_SERVICE),
        service.as_bytes(),
        0o644,
    )
}

/// Undoes `install_file_manager`, handing the folder handler recorded to `restore` (which sets
/// it, and returns what it set); the service and state go only once that worked.
pub fn uninstall_file_manager(
    context: &Context,
    restore: impl FnOnce(Option<&str>) -> Result<Option<String>, String>,
) -> Result<Option<String>, String> {
    let state_directory = context.data_home.join(FILE_MANAGER_STATE_DIRECTORY);
    let previous =
        read_file_manager_state(&state_directory)?.and_then(|state| state.previous_default);
    let restored = restore(previous.as_deref())?;
    remove_if_exists(&context.services().join(FILE_MANAGER_SERVICE))?;
    remove_state(&state_directory)?;
    Ok(restored)
}

/// The folder handler to restore: only while Files is still it; the one recorded, else Nautilus
/// where it is installed. `set` makes it the default.
pub fn restore_folder_handler(
    current: Option<&str>,
    previous: Option<&str>,
    nautilus: impl FnOnce() -> Option<String>,
    set: impl FnOnce(&str) -> Result<(), String>,
) -> Result<Option<String>, String> {
    if current.is_some_and(|id| id != DESKTOP_ID) {
        return Ok(None);
    }
    let target = previous.map(str::to_owned).or_else(nautilus).ok_or_else(|| {
        crate::i18n::tr(
            "No previous folder handler was recorded and Nautilus was not detected. Choose another default file manager first, then retry Restore default.",
        )
    })?;
    set(&target)?;
    Ok(Some(target))
}

pub fn file_manager_status(
    context: &Context,
    executable: &Path,
    default: Option<&str>,
) -> Result<FileManagerStatus, String> {
    let service = context.services().join(FILE_MANAGER_SERVICE);
    let has_service = service.is_file() && {
        let config = KeyFile::parse(&read_utf8(&service)?)?;
        config.value("D-BUS Service", "Name").as_deref() == Some("org.freedesktop.FileManager1")
            && config.value("D-BUS Service", "Exec").as_deref() == executable.to_str()
            && file_manager_conflict(&context.services())?.is_none()
    };
    let state = context
        .data_home
        .join(FILE_MANAGER_STATE_DIRECTORY)
        .join(STATE_FILE);
    Ok(FileManagerStatus {
        default: default == Some(DESKTOP_ID),
        has_service,
        has_installation: service.exists() || state.exists(),
    })
}

fn read_file_manager_state(directory: &Path) -> Result<Option<FileManagerState>, String> {
    let path = directory.join(STATE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    toml::from_str(&read_utf8(&path)?)
        .map(Some)
        .map_err(|error| format!("Could not read file manager installation state: {error}"))
}

fn file_manager_conflict(services: &Path) -> Result<Option<String>, String> {
    let entries = match fs::read_dir(services) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(path_error("read", services, &error)),
    };
    for entry in entries {
        let path = entry
            .map_err(|error| path_error("read", services, &error))?
            .path();
        if path
            .file_name()
            .is_some_and(|name| name == FILE_MANAGER_SERVICE)
            || path
                .extension()
                .is_none_or(|extension| extension != "service")
        {
            continue;
        }
        let Ok(config) = KeyFile::parse(&read_utf8(&path)?) else {
            continue;
        };
        if config.value("D-BUS Service", "Name").as_deref() == Some("org.freedesktop.FileManager1")
        {
            return Ok(Some(path.display().to_string()));
        }
    }
    Ok(None)
}

/// The desktop's default handler for folders (`xdg-mime`), when it has one.
pub fn default_folder_handler() -> Option<String> {
    let output = std::process::Command::new("xdg-mime")
        .args(["query", "default", "inode/directory"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let id = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !id.is_empty()).then_some(id)
}

/// Makes `id` the default handler for folders (`xdg-mime`), checking it took.
pub fn set_folder_handler(id: &str) -> Result<(), String> {
    let status = std::process::Command::new("xdg-mime")
        .args(["default", id, "inode/directory"])
        .status()
        .map_err(|error| format!("Could not run xdg-mime: {error}"))?;
    if !status.success() || default_folder_handler().as_deref() != Some(id) {
        return Err(crate::i18n::tr("The folder association did not change"));
    }
    Ok(())
}

/// Nautilus's desktop ID where it is installed (its desktop file in the data folders).
pub fn installed_nautilus() -> Option<String> {
    let mut roots = xdg_directories("XDG_DATA_DIRS", "/usr/local/share:/usr/share");
    if let Ok(context) = Context::from_environment() {
        roots.insert(0, context.data_home);
    }
    ["org.gnome.Nautilus.desktop", "nautilus.desktop"]
        .into_iter()
        .find(|id| {
            roots
                .iter()
                .any(|root| root.join("applications").join(id).is_file())
        })
        .map(str::to_owned)
}

/// `executable` as activation may name it: absolute, a regular executable file, owned (with every
/// folder above it) by the user or root and writable by no one else, and a path D-Bus can quote.
pub fn activation_executable(executable: &Path) -> Result<String, String> {
    activation_executable_for(executable, rustix::process::geteuid().as_raw())
}

fn activation_executable_for(path: &Path, user: u32) -> Result<String, String> {
    let path = fs::canonicalize(path).map_err(|error| path_error("resolve", path, &error))?;
    for (index, component) in path.ancestors().enumerate() {
        let metadata =
            fs::metadata(component).map_err(|error| path_error("inspect", component, &error))?;
        if index == 0 {
            if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
                return Err(crate::i18n::tr(
                    "The Files executable must be a regular executable file",
                ));
            }
        } else if !metadata.is_dir() {
            return Err(crate::i18n::tr(
                "The Files executable path must contain only directories",
            ));
        }
        if metadata.uid() != 0 && metadata.uid() != user {
            return Err(crate::i18n::tr(
                "The Files executable path must be owned by the current user or root",
            ));
        }
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(crate::i18n::tr(
                "The Files executable path must not be writable by other users",
            ));
        }
    }
    let text = path
        .to_str()
        .ok_or_else(|| crate::i18n::tr("Files must be installed at a UTF-8 path"))?
        .to_owned();
    if text
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '\\' | '\'' | '"'))
    {
        return Err(crate::i18n::tr(
            "The Files executable path contains characters unsupported by D-Bus activation",
        ));
    }
    Ok(text)
}

fn read_utf8(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| path_error("read", path, &error))
}

/// Written whole or not at all (through a file beside it), with `mode`.
fn write_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let partial = path.with_extension("asc-files-partial");
    fs::write(&partial, contents)
        .and_then(|()| fs::set_permissions(&partial, fs::Permissions::from_mode(mode & 0o7777)))
        .and_then(|()| fs::rename(&partial, path))
        .map_err(|error| {
            let _ = fs::remove_file(&partial);
            path_error("write", path, &error)
        })
}

fn file_mode(path: &Path) -> u32 {
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode())
        .unwrap_or(0o600)
}

fn ensure_regular_or_missing(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(format!(
            "Refusing to replace non-regular portal configuration {}",
            path.display()
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(path_error("inspect", path, &error)),
    }
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(path_error("remove", path, &error)),
    }
}

fn path_error(action: &str, path: &Path, error: &io::Error) -> String {
    format!(
        "Could not {action} {}: {}",
        path.display(),
        crate::services::io_error_message(error)
    )
}

#[cfg(test)]
mod tests;
