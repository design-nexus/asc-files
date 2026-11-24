// SPDX-License-Identifier: MIT
//! The freedesktop.org Trash: the home Trash and each drive's `$topdir/.Trash/$uid` or
//! `$topdir/.Trash-$uid`. Location rules follow Flea's `topdirtrash` (© 2026 GM, MIT): an
//! administrator's `.Trash` only when it is a real sticky folder on the same drive, never the
//! mount root itself, never anything already in a Trash, records relative to the drive.
//!
//! A put writes its `.trashinfo` exclusively before the item moves, and every move is a
//! `RENAME_NOREPLACE`, so a Trash entry is never overwritten and a lost race costs a number.
//! Restore and delete act only on the entry a listing showed, proven by device and inode.

use std::{
    ffi::OsString,
    io::Write,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
};

use crate::{
    adapters::volume::MountTable,
    error::{Error, ErrorKind, os_io_error},
};

/// An item in the Trash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trashed {
    /// Where the item now is, under its Trash's `files` folder.
    pub file: PathBuf,
    /// Its `.trashinfo`.
    pub info: PathBuf,
}

/// One entry of a Trash listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub file: PathBuf,
    pub info: PathBuf,
    /// Where the item came from, absolute.
    pub original: PathBuf,
    /// The record's `DeletionDate`, as written (`YYYY-MM-DDThh:mm:ss`, local time).
    pub deleted: String,
    /// The Trash folder holding it (the one with `files/` and `info/`).
    pub trash: PathBuf,
    identity: Identity,
}

/// Device and inode, which a filesystem may hand to a new file the moment the old one goes, so
/// with the status-change time too: a file made since has a later one.
type Identity = (u64, u64, i64, i64);

fn identity(meta: &std::fs::Metadata) -> Identity {
    (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
}

/// Whether `item` could go to a Trash on its own drive, by the rules [`put`] follows, without
/// making anything. A drive whose top folder could take a `.Trash-$uid` counts.
pub fn available(item: &Path) -> bool {
    let (Some(parent), Ok(meta)) = (item.parent(), std::fs::symlink_metadata(item)) else {
        return false;
    };
    let Ok(parent) = std::fs::canonicalize(parent) else {
        return false;
    };
    let home = home_trash();
    if same_mount_and_device(&parent, meta.dev(), home.parent()) {
        return true;
    }
    let Some(top) = topdir(&parent) else {
        return false;
    };
    let writable = |folder: &Path| {
        rustix::fs::access(
            folder,
            rustix::fs::Access::WRITE_OK | rustix::fs::Access::EXEC_OK,
        )
        .is_ok()
    };
    match topdir_trash(&top, false) {
        Some(trash) => std::fs::metadata(&trash).is_ok_and(|trash| trash.dev() == meta.dev()),
        None => writable(&top),
    }
}

/// Moves `item` to the Trash on its own drive, never copying between drives.
pub fn put(item: &Path) -> Result<Trashed, Error> {
    put_recording(item, None)
}

/// [`put`], recording `original` as where the item came from when a replace has already moved
/// it to a hidden name. `original` must be on the item's drive, in a folder that exists.
pub(crate) fn put_recording(item: &Path, original: Option<&Path>) -> Result<Trashed, Error> {
    let unnamed = || crate::error::translated_io_error("The item has no name");
    // A symlinked parent would record a path restore cannot trust; record the real one.
    let real = |path: &Path| -> Result<PathBuf, Error> {
        let name = path.file_name().ok_or_else(unnamed)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Ok(std::fs::canonicalize(parent)
            .map_err(os_io_error)?
            .join(name))
    };
    let item = real(item)?;
    let parent = item.parent().ok_or_else(unnamed)?.to_path_buf();
    let meta = std::fs::symlink_metadata(&item).map_err(os_io_error)?;
    let original = match original {
        Some(original) => real(original)?,
        None => item.clone(),
    };
    let name = original.file_name().ok_or_else(unnamed)?.to_os_string();

    let home = home_trash();
    let (trash, recorded) = if same_mount_and_device(&parent, meta.dev(), home.parent()) {
        (home, original.clone())
    } else {
        let top = topdir(&parent).ok_or_else(|| unsupported("This item's drive has no Trash"))?;
        if item == top || original == top {
            return Err(unsupported("A drive's top folder cannot go to the Trash"));
        }
        if in_a_trash(&item, &top) || in_a_trash(&original, &top) {
            return Err(unsupported("This item is already in the Trash"));
        }
        let relative = original
            .strip_prefix(&top)
            .map_err(|_| unsupported("The item's original place is on another drive"))?
            .to_path_buf();
        let trash = topdir_trash(&top, true)
            .ok_or_else(|| unsupported("This drive's Trash could not be made"))?;
        (trash, relative)
    };
    let (files, info) = (trash.join("files"), trash.join("info"));
    for dir in [&files, &info] {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(os_io_error)?;
    }
    if std::fs::metadata(&files).map_err(os_io_error)?.dev() != meta.dev() {
        return Err(unsupported(
            "This item is on a part of the drive the Trash cannot reach, such as a btrfs subvolume",
        ));
    }
    let record = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode(recorded.as_os_str().as_bytes()),
        local_timestamp()
    );
    for n in 1..10_000u32 {
        let mut stored = name.clone();
        if n > 1 {
            stored.push(format!(".{n}"));
        }
        let mut info_name = stored.clone();
        info_name.push(".trashinfo");
        let info_path = info.join(&info_name);
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&info_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(os_io_error(error)),
        };
        if let Err(error) = file
            .write_all(record.as_bytes())
            .and_then(|()| file.sync_all())
        {
            let _ = std::fs::remove_file(&info_path);
            return Err(os_io_error(error));
        }
        let file_path = files.join(&stored);
        match move_in(&item, &file_path) {
            Ok(()) => {
                return Ok(Trashed {
                    file: file_path,
                    info: info_path,
                });
            }
            Err(rustix::io::Errno::EXIST) => {
                let _ = std::fs::remove_file(&info_path);
            }
            Err(error) => {
                let _ = std::fs::remove_file(&info_path);
                return Err(refused(&item, error));
            }
        }
    }
    Err(crate::error::translated_io_error(
        "The Trash has no free name for this item",
    ))
}

/// Every entry in the home Trash and in the Trash of each local drive. Reading never creates a
/// Trash, and network and automounted drives are not touched, so a stalled mount cannot hang it.
pub fn list() -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut roots = vec![(home_trash(), None)];
    let table = MountTable::current();
    for (_, point) in table.points() {
        if skipped_mount(&table, point) {
            continue;
        }
        if let Some(trash) = topdir_trash(point, false) {
            roots.push((trash, Some(point.to_path_buf())));
        }
    }
    for (trash, top) in roots {
        // One drive reached through two mount points lists once.
        let Ok(meta) = std::fs::metadata(trash.join("info")) else {
            continue;
        };
        if !seen.insert((meta.dev(), meta.ino())) {
            continue;
        }
        entries.extend(read_trash(&trash, top.as_deref()));
    }
    entries
}

/// Puts `entry` back where it came from, refusing if something has its name there now.
pub fn restore(entry: &Entry) -> Result<PathBuf, Error> {
    still_listed(entry)?;
    let parent = entry
        .original
        .parent()
        .ok_or_else(|| crate::error::translated_io_error("The original location is unknown"))?;
    if !parent.is_dir() {
        return Err(Error::new(
            ErrorKind::NotFound,
            &crate::i18n::tr("The folder this item came from no longer exists"),
        ));
    }
    match move_out(&entry.file, &entry.original) {
        Ok(()) => {}
        Err(rustix::io::Errno::EXIST) => {
            return Err(Error::new(
                ErrorKind::Exists,
                &crate::i18n::tr("An item with that name already exists"),
            ));
        }
        Err(rustix::io::Errno::XDEV) => {
            return Err(unsupported(
                "The item's original folder is now on another drive",
            ));
        }
        Err(error) => return Err(os_io_error(error)),
    }
    let _ = std::fs::remove_file(&entry.info);
    forget_size(entry);
    Ok(entry.original.clone())
}

/// Deletes `entry` from the Trash for good.
pub fn delete(entry: &Entry) -> Result<(), Error> {
    still_listed(entry)?;
    let removed = if std::fs::symlink_metadata(&entry.file).is_ok_and(|meta| meta.is_dir()) {
        std::fs::remove_dir_all(&entry.file)
    } else {
        std::fs::remove_file(&entry.file)
    };
    match removed {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(os_io_error(error)),
    }
    let _ = std::fs::remove_file(&entry.info);
    forget_size(entry);
    Ok(())
}

/// Deletes every entry in every Trash this user can list. Failures do not stop the rest.
pub fn empty() -> (usize, Vec<(Entry, Error)>) {
    delete_all(list())
}

/// Deletes `entries` for good; returns how many went and the failures, which do not stop the rest.
pub fn delete_all(entries: impl IntoIterator<Item = Entry>) -> (usize, Vec<(Entry, Error)>) {
    let mut deleted = 0;
    let mut failed = Vec::new();
    for entry in entries {
        match delete(&entry) {
            Ok(()) => deleted += 1,
            Err(error) => failed.push((entry, error)),
        }
    }
    (deleted, failed)
}

fn still_listed(entry: &Entry) -> Result<(), Error> {
    let current = std::fs::symlink_metadata(&entry.file).map_err(os_io_error)?;
    if identity(&current) != entry.identity || !entry.info.is_file() {
        return Err(crate::error::translated_io_error(
            "This item changed in the Trash since it was shown",
        ));
    }
    Ok(())
}

fn read_trash(trash: &Path, top: Option<&Path>) -> Vec<Entry> {
    let Ok(infos) = std::fs::read_dir(trash.join("info")) else {
        return Vec::new();
    };
    infos
        .filter_map(|info| {
            let info = info.ok()?.path();
            let stored = info
                .file_name()?
                .as_bytes()
                .strip_suffix(b".trashinfo")?
                .to_vec();
            let file = trash.join("files").join(OsString::from_vec(stored));
            let identity = std::fs::symlink_metadata(&file)
                .ok()
                .map(|meta| identity(&meta))?;
            let (path, deleted) = parse_record(&std::fs::read(&info).ok()?)?;
            let original = match top {
                Some(top) if path.is_relative() => top.join(&path),
                // A drive's record may not restore outside the drive, whatever an older writer put.
                Some(top) if !path.starts_with(top) => return None,
                _ if path.is_relative() => return None,
                _ => path,
            };
            if original
                .components()
                .any(|part| matches!(part, Component::ParentDir))
            {
                return None;
            }
            Some(Entry {
                file,
                info,
                original,
                deleted,
                trash: trash.to_path_buf(),
                identity,
            })
        })
        .collect()
}

/// A record's `Path` and `DeletionDate`; `None` for anything that is not a Trash record.
fn parse_record(bytes: &[u8]) -> Option<(PathBuf, String)> {
    // An encoded Path is at most three times PATH_MAX; a larger file is not a record.
    if bytes.len() > 3 * 4096 + 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim() != "[Trash Info]" {
        return None;
    }
    let (mut path, mut deleted) = (None, String::new());
    for line in lines {
        if let Some(value) = line.strip_prefix("Path=") {
            path = Some(PathBuf::from(OsString::from_vec(percent_decode(value)?)));
        } else if let Some(value) = line.strip_prefix("DeletionDate=") {
            deleted = value.to_owned();
        } else if line.starts_with('[') {
            break;
        }
    }
    Some((path?, deleted))
}

/// The spec's `directorysizes` cache must not keep a line for an entry that left the Trash.
fn forget_size(entry: &Entry) {
    let cache = entry.trash.join("directorysizes");
    // Read, changed and replaced under a lock on the Trash folder itself, so two deletes at once
    // cannot each write back what the other removed. No other file appears in the Trash.
    let Ok(folder) = rustix::fs::open(
        &entry.trash,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    ) else {
        return;
    };
    if rustix::fs::flock(&folder, rustix::fs::FlockOperation::LockExclusive).is_err() {
        return;
    }
    let Ok(text) = std::fs::read(&cache) else {
        return;
    };
    let Some(name) = entry.file.file_name() else {
        return;
    };
    let encoded = percent_encode(name.as_bytes());
    let kept: Vec<&[u8]> = text
        .split(|byte| *byte == b'\n')
        .filter(|line| {
            !line.is_empty() && line.rsplit(|b| *b == b' ').next() != Some(encoded.as_bytes())
        })
        .collect();
    let mut rewritten = kept.join(&b'\n');
    if !rewritten.is_empty() {
        rewritten.push(b'\n');
    }
    let temporary = entry.trash.join(".directorysizes.new");
    if std::fs::write(&temporary, &rewritten).is_ok() {
        let _ = std::fs::rename(&temporary, &cache);
    }
}

fn home_trash() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|dir| Path::new(dir).is_absolute())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("Trash")
}

/// Whether `folder` is on the same mount and device as the existing ancestor of `home`.
fn same_mount_and_device(folder: &Path, device: u64, home: Option<&Path>) -> bool {
    let Some(home) = home.and_then(|home| home.ancestors().find(|dir| dir.exists())) else {
        return false;
    };
    let mount = |path: &Path| {
        rustix::fs::statx(
            rustix::fs::CWD,
            path,
            rustix::fs::AtFlags::empty(),
            rustix::fs::StatxFlags::MNT_ID,
        )
        .ok()
        .map(|stat| stat.stx_mnt_id)
    };
    std::fs::metadata(home).is_ok_and(|meta| meta.dev() == device)
        && mount(folder).is_some()
        && mount(folder) == mount(home)
}

/// The mount point of the mount `folder` is on, by mount id, so bind and subvolume mounts count.
fn topdir(folder: &Path) -> Option<PathBuf> {
    let id = rustix::fs::statx(
        rustix::fs::CWD,
        folder,
        rustix::fs::AtFlags::empty(),
        rustix::fs::StatxFlags::MNT_ID,
    )
    .ok()?
    .stx_mnt_id;
    MountTable::current()
        .mount_of(id)
        .map(|(_, site)| site.point.clone())
}

fn in_a_trash(item: &Path, top: &Path) -> bool {
    let uid = rustix::process::getuid().as_raw();
    item.starts_with(top.join(".Trash")) || item.starts_with(top.join(format!(".Trash-{uid}")))
}

/// Pseudo, network and automounted filesystems never hold a Trash a listing should open.
fn skipped_mount(table: &MountTable, point: &Path) -> bool {
    const PSEUDO: [&str; 16] = [
        "proc",
        "sysfs",
        "cgroup",
        "cgroup2",
        "devpts",
        "devtmpfs",
        "mqueue",
        "debugfs",
        "tracefs",
        "securityfs",
        "pstore",
        "bpf",
        "configfs",
        "fusectl",
        "autofs",
        "binfmt_misc",
    ];
    table.is_network_path(point)
        || table.mount_at(point).is_none_or(|(mount, _)| {
            PSEUDO.contains(&mount.fs_type.as_str()) || mount.fs_type.starts_with("fuse.")
        })
}

/// The drive's Trash for this user: an administrator's sticky `.Trash/$uid` first, else
/// `.Trash-$uid`. Each must be a real folder on the same drive; ours must be private to us.
fn topdir_trash(top: &Path, create: bool) -> Option<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let top_dev = std::fs::metadata(top).ok()?.dev();
    let real_dir = |path: &Path| {
        std::fs::symlink_metadata(path)
            .ok()
            .filter(|meta| meta.is_dir() && meta.dev() == top_dev)
    };
    let private = |path: &Path, create: bool| {
        if create {
            match std::fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return None,
            }
        }
        real_dir(path)
            .filter(|meta| meta.uid() == uid && meta.mode() & 0o077 == 0)
            .map(|_| path.to_path_buf())
    };
    let admin = top.join(".Trash");
    if real_dir(&admin).is_some_and(|meta| meta.mode() & 0o1000 != 0) {
        if let Some(shared) = private(&admin.join(uid.to_string()), create) {
            return Some(shared);
        }
    }
    private(&top.join(format!(".Trash-{uid}")), create)
}

/// Out of a Trash, never over what holds the name.
fn move_out(from: &Path, to: &Path) -> Result<(), rustix::io::Errno> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    )
}

/// Into a Trash, through the engine's filesystem. An SMB folder refused while a file below it
/// waits for the client's deferred close is tried again until that closes.
fn move_in(from: &Path, to: &Path) -> Result<(), rustix::io::Errno> {
    use rustix::io::Errno;

    use crate::adapters::local_operations::{
        deferred_close,
        fs_ops::{Entry, fs},
        leftovers::Deadline,
        native_fs::open_local_parent_directory,
    };
    let (Some(from_dir), Some(from_name), Some(to_dir), Some(to_name)) =
        (from.parent(), from.file_name(), to.parent(), to.file_name())
    else {
        return Err(Errno::INVAL);
    };
    let from_dir = open_local_parent_directory(from_dir).map_err(|_| Errno::NOENT)?;
    let to_dir = open_local_parent_directory(to_dir).map_err(|_| Errno::NOENT)?;
    let moving = Entry::new(&from_dir, from_name);
    deferred_close::retry_folder_rename(
        fs(),
        moving,
        &Deadline::default(),
        &|| false,
        |error: &Errno| *error == Errno::ACCESS,
        || fs().trash(moving, Entry::new(&to_dir, to_name)),
    )
    .unwrap_or(Err(Errno::INTR))
}

/// Why `item` could not go to the Trash: on SMB, a file still open elsewhere is said so.
fn refused(item: &Path, error: rustix::io::Errno) -> Error {
    use crate::adapters::local_operations::{
        fs_ops::{Entry, fs},
        holders::refused_rename,
        native_fs::open_local_parent_directory,
    };
    let why = item
        .parent()
        .zip(item.file_name())
        .and_then(|(parent, name)| {
            let dir = open_local_parent_directory(parent).ok()?;
            refused_rename(fs(), Entry::new(&dir, name), error)
        });
    match why {
        Some(why) => Error::new(ErrorKind::Busy, &why),
        None => os_io_error(error),
    }
}

fn unsupported(message: &str) -> Error {
    Error::new(ErrorKind::NotSupported, &crate::i18n::tr(message))
}

/// Every byte outside the URI-unreserved set and `/`, as `%XX`.
fn percent_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

/// `YYYY-MM-DDThh:mm:ss` in local time.
fn local_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as libc::time_t);
    // SAFETY: `tm` is plain data that `localtime_r` fills; both pointers are valid for the call.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_bytes_a_path_may_hold() {
        let path = b"/home/u/r\xc3\xa9sum\xc3\xa9 1%\n\xff.txt";
        let encoded = percent_encode(path);
        assert!(encoded.is_ascii() && !encoded.contains(' ') && !encoded.contains('\n'));
        assert_eq!(percent_decode(&encoded).unwrap(), path);
    }

    #[test]
    fn a_record_needs_its_header_and_a_path() {
        assert_eq!(
            parse_record(b"[Trash Info]\nPath=kit.zip\nDeletionDate=2026-11-10T09:12:00\n"),
            Some((PathBuf::from("kit.zip"), "2026-11-10T09:12:00".to_owned()))
        );
        assert_eq!(parse_record(b"Path=/x\n"), None);
        assert_eq!(
            parse_record(b"[Trash Info]\nDeletionDate=2026-11-10T09:12:00\n"),
            None
        );
        assert_eq!(parse_record(b"[Trash Info]\nPath=%zz\n"), None);
    }
}
