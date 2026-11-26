// SPDX-License-Identifier: MIT
//! Legacy's `portal_setup/tests.rs`, for what still applies (its Strata, Omarchy, systemd-refresh
//! and one-time-offer tests are for parts not carried over).

use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

use super::*;

const FILE_CHOOSER: &str = "org.freedesktop.impl.portal.FileChooser";

fn enable_config(contents: &str) -> Result<String, String> {
    update_config(contents, true)
}

fn disable_config(contents: &str) -> Result<String, String> {
    update_config(contents, false)
}

fn context(root: &Path) -> Context {
    let data_home = root.join("data");
    let config_home = root.join("config");
    Context {
        search_roots: vec![config_home.clone(), root.join("system")],
        data_home,
        config_home,
        config_names: vec![
            "hyprland-portals.conf".to_owned(),
            "portals.conf".to_owned(),
        ],
    }
}

fn executable(root: &Path) -> PathBuf {
    let path = root.join("bin/asc-files");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"binary").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// In the crate's folder, as legacy's were in `target`: activation refuses a world-writable
/// folder such as /tmp above the executable.
fn fixture() -> tempfile::TempDir {
    let fixture = tempfile::Builder::new()
        .prefix(".asc-files-integrations-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o755)).unwrap();
    fixture
}

/// Whether this environment lets activation name an executable in the fixture (where tests run
/// as root over a checkout another user owns, it refuses, as it should).
fn usable(fixture: &tempfile::TempDir) -> bool {
    let usable = activation_executable(&executable(fixture.path())).is_ok();
    if !usable {
        eprintln!("skipped: activation refuses executables in this checkout's folders");
    }
    usable
}

fn read_fm_state(context: &Context) -> Option<FileManagerState> {
    read_file_manager_state(&context.data_home.join(FILE_MANAGER_STATE_DIRECTORY)).unwrap()
}

fn uninstall_fm(context: &Context) -> Result<Option<String>, String> {
    uninstall_file_manager(context, |previous| Ok(previous.map(str::to_owned)))
}

#[test]
fn chooser_preference_uses_existing_backends_and_round_trips() {
    let original = "[preferred]\ndefault=hyprland;gtk;\norg.example.Other=gtk;\n";
    let enabled = enable_config(original).unwrap();
    assert!(
        enabled.contains(&format!("{FILE_CHOOSER}=asc_files;hyprland;gtk;")),
        "{enabled}"
    );
    assert_eq!(enable_config(&enabled).unwrap(), enabled);
    assert_eq!(disable_config(&enabled).unwrap(), original);
}

#[test]
fn chooser_preference_preserves_explicit_fallbacks() {
    let original = format!("[preferred]\ndefault=gnome;gtk;\n{FILE_CHOOSER}=kde;gtk;\n");
    let enabled = enable_config(&original).unwrap();
    assert!(enabled.contains(&format!("{FILE_CHOOSER}=asc_files;kde;gtk;")));
    assert_eq!(disable_config(&enabled).unwrap(), original);
}

#[test]
fn install_and_uninstall_restore_an_existing_user_configuration() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    let config = context.portal_directory().join("portals.conf");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    let original = "[preferred]\ndefault=gtk;\n";
    fs::write(&config, original).unwrap();
    let exe = executable(fixture.path());

    assert_eq!(install_chooser(&context, &exe).unwrap(), config);
    assert!(
        fs::read_to_string(&config)
            .unwrap()
            .contains("asc_files;gtk;")
    );
    assert!(
        context
            .data_home
            .join("xdg-desktop-portal/portals/asc_files.portal")
            .is_file()
    );
    let service = fs::read_to_string(context.services().join(SERVICE_FILE)).unwrap();
    assert!(
        service.contains(&format!(
            "Exec={} --chooser",
            fs::canonicalize(&exe).unwrap().display()
        )),
        "{service}"
    );

    install_chooser(&context, &exe).unwrap();
    assert!(!uninstall_chooser(&context).unwrap());
    assert_eq!(fs::read_to_string(config).unwrap(), original);
}

#[test]
fn uninstall_keeps_later_configuration_edits() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    let config = context.portal_directory().join("portals.conf");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "[preferred]\ndefault=gtk;\n").unwrap();
    install_chooser(&context, &executable(fixture.path())).unwrap();
    fs::write(
        &config,
        format!(
            "[preferred]\ndefault=gtk;\n{FILE_CHOOSER}=asc_files;gtk;\norg.example.Other=custom;\n"
        ),
    )
    .unwrap();

    assert!(uninstall_chooser(&context).unwrap());
    let remaining = fs::read_to_string(config).unwrap();
    assert!(!remaining.contains("asc_files"));
    assert!(remaining.contains("org.example.Other=custom;"));
}

#[test]
fn generated_override_is_removed_on_uninstall() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    let system = context.search_roots[1].join("xdg-desktop-portal/hyprland-portals.conf");
    fs::create_dir_all(system.parent().unwrap()).unwrap();
    fs::write(&system, "[preferred]\ndefault=hyprland;gtk;\n").unwrap();

    let generated = install_chooser(&context, &executable(fixture.path())).unwrap();
    assert!(generated.ends_with("hyprland-portals.conf"));
    assert!(generated.starts_with(&context.config_home));
    assert!(
        fs::read_to_string(&generated)
            .unwrap()
            .contains("asc_files;")
    );
    assert!(!uninstall_chooser(&context).unwrap());
    assert!(!generated.exists());
}

#[test]
fn portal_activation_rejects_replaceable_executables() {
    let fixture = fixture();
    let exe = executable(fixture.path());
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o775)).unwrap();
    assert!(activation_executable(&exe).is_err());
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(exe.parent().unwrap(), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(activation_executable(&exe).is_err());
    fs::set_permissions(exe.parent().unwrap(), fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(activation_executable(&exe).is_err());
    fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o755)).unwrap();
    if usable(&fixture) {
        assert!(activation_executable(&exe).is_ok());
    }
}

#[test]
fn status_uses_the_active_configuration_and_requires_activation_files() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    let config = install_chooser(&context, &executable(fixture.path())).unwrap();
    assert_eq!(
        chooser_status(&context).unwrap(),
        ChooserStatus {
            preferred: true,
            activatable: true
        }
    );
    fs::write(&config, "[preferred]\ndefault=gtk;\n").unwrap();
    assert!(!chooser_status(&context).unwrap().preferred);
    fs::write(&config, "[preferred]\ndefault=asc_files;gtk;\n").unwrap();
    assert!(chooser_status(&context).unwrap().preferred);
    fs::remove_file(context.services().join(SERVICE_FILE)).unwrap();
    assert!(!chooser_status(&context).unwrap().activatable);
}

#[test]
fn file_manager_service_installs_and_reports_status() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    let exe = executable(fixture.path());
    install_file_manager(&context, &exe, Some("thunar.desktop")).unwrap();
    let service = context.services().join(FILE_MANAGER_SERVICE);
    let contents = fs::read_to_string(&service).unwrap();
    assert!(contents.contains("Name=org.freedesktop.FileManager1"));
    assert!(contents.contains(&fs::canonicalize(&exe).unwrap().display().to_string()));
    assert_eq!(
        read_fm_state(&context).unwrap().previous_default.as_deref(),
        Some("thunar.desktop")
    );
    let status =
        file_manager_status(&context, &fs::canonicalize(&exe).unwrap(), Some(DESKTOP_ID)).unwrap();
    assert_eq!(
        status,
        FileManagerStatus {
            default: true,
            has_service: true,
            has_installation: true
        }
    );
}

#[test]
fn file_manager_repair_preserves_restore_state() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    let exe = executable(fixture.path());
    install_file_manager(&context, &exe, Some("thunar.desktop")).unwrap();
    let service = context.services().join(FILE_MANAGER_SERVICE);
    fs::remove_file(&service).unwrap();
    install_file_manager(&context, &exe, None).unwrap();
    assert!(service.exists());
    assert_eq!(
        uninstall_fm(&context).unwrap().as_deref(),
        Some("thunar.desktop")
    );
}

#[test]
fn file_manager_restore_failure_keeps_recovery_state() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    install_file_manager(
        &context,
        &executable(fixture.path()),
        Some("thunar.desktop"),
    )
    .unwrap();
    let result = uninstall_file_manager(&context, |previous| {
        assert_eq!(previous, Some("thunar.desktop"));
        Err("association failure".into())
    });
    assert_eq!(result.unwrap_err(), "association failure");
    assert!(context.services().join(FILE_MANAGER_SERVICE).exists());
    assert_eq!(
        uninstall_fm(&context).unwrap().as_deref(),
        Some("thunar.desktop")
    );
}

#[test]
fn folder_restore_uses_nautilus_only_without_a_saved_handler() {
    for (current, previous, detected, expected) in [
        (
            Some(DESKTOP_ID),
            Some("thunar.desktop"),
            Some("org.gnome.Nautilus.desktop"),
            Some("thunar.desktop"),
        ),
        (
            Some(DESKTOP_ID),
            None,
            Some("org.gnome.Nautilus.desktop"),
            Some("org.gnome.Nautilus.desktop"),
        ),
        (
            None,
            None,
            Some("nautilus.desktop"),
            Some("nautilus.desktop"),
        ),
        (
            Some("dolphin.desktop"),
            None,
            Some("org.gnome.Nautilus.desktop"),
            None,
        ),
        (Some("dolphin.desktop"), Some("thunar.desktop"), None, None),
    ] {
        let mut applied = None;
        let result = restore_folder_handler(
            current,
            previous,
            || {
                assert!(previous.is_none());
                detected.map(str::to_owned)
            },
            |id| {
                applied = Some(id.to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result.as_deref(), expected);
        assert_eq!(applied.as_deref(), expected);
    }
}

#[test]
fn nautilus_fallback_failure_preserves_recovery_until_retry() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    install_file_manager(&context, &executable(fixture.path()), None).unwrap();
    for detected in [None, Some("org.gnome.Nautilus.desktop")] {
        let result = uninstall_file_manager(&context, |previous| {
            restore_folder_handler(
                Some(DESKTOP_ID),
                previous,
                || detected.map(str::to_owned),
                |_| Err("association failure".into()),
            )
        });
        assert!(result.is_err());
        assert!(read_fm_state(&context).is_some());
        assert!(context.services().join(FILE_MANAGER_SERVICE).exists());
    }
    let restored = uninstall_file_manager(&context, |previous| {
        restore_folder_handler(
            Some(DESKTOP_ID),
            previous,
            || Some("org.gnome.Nautilus.desktop".into()),
            |id| {
                assert_eq!(id, "org.gnome.Nautilus.desktop");
                Ok(())
            },
        )
    })
    .unwrap();
    assert_eq!(restored.as_deref(), Some("org.gnome.Nautilus.desktop"));
    assert!(!context.services().join(FILE_MANAGER_SERVICE).exists());
    assert!(read_fm_state(&context).is_none());
}

#[test]
fn file_manager_conflict_is_rejected() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    fs::create_dir_all(context.services()).unwrap();
    fs::write(
        context.services().join("org.other.FileManager1.service"),
        "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/other\n",
    )
    .unwrap();
    let error = install_file_manager(&context, &executable(fixture.path()), None).unwrap_err();
    assert!(error.contains("Another per-user FileManager1 provider"));
}

#[test]
fn file_manager_uninstall_removes_service_and_state() {
    let fixture = fixture();
    if !usable(&fixture) {
        return;
    }
    let context = context(fixture.path());
    install_file_manager(
        &context,
        &executable(fixture.path()),
        Some("thunar.desktop"),
    )
    .unwrap();
    let service = context.services().join(FILE_MANAGER_SERVICE);
    assert!(service.is_file());
    uninstall_fm(&context).unwrap();
    assert!(!service.exists());
    assert!(read_fm_state(&context).is_none());
}

// New: comments and other sections survive the edit.
#[test]
fn editing_keeps_comments_and_other_sections() {
    let original =
        "# chosen by hand\n[preferred]\n# the default\ndefault=gtk;\n\n[other]\nkey=value\n";
    let enabled = enable_config(original).unwrap();
    assert!(
        enabled.starts_with("# chosen by hand\n[preferred]\n# the default\ndefault=gtk;\n"),
        "{enabled}"
    );
    assert!(enabled.contains("[other]\nkey=value"));
    assert_eq!(disable_config(&enabled).unwrap(), original);
    assert!(enable_config("not a key file").is_err());
}
