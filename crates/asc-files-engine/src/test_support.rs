// SPDX-License-Identifier: MIT
//! Spike: the engine's test helpers, without GTK or a GLib main loop.

use std::sync::{LockResult, Mutex, MutexGuard};

use rustix::fs::{Mode, OFlags};

pub(crate) struct TestMutex(Mutex<()>);

impl TestMutex {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(()))
    }

    pub(crate) fn lock(&self) -> LockResult<MutexGuard<'_, ()>> {
        match self.0.lock() {
            Ok(guard) => Ok(guard),
            Err(error) => {
                self.0.clear_poison();
                Ok(error.into_inner())
            }
        }
    }
}

/// GTK initialization is thread-affine; each UI test gets a process and disposable preferences.
pub(crate) fn gtk_test(name: &str, run: impl FnOnce()) {
    gtk_test_with_env(name, std::iter::empty::<(&str, &std::ffi::OsStr)>(), run);
}

pub(crate) fn gtk_test_with_env(
    name: &str,
    extra_env: impl IntoIterator<Item = (impl AsRef<std::ffi::OsStr>, impl AsRef<std::ffi::OsStr>)>,
    run: impl FnOnce(),
) {
    const CHILD: &str = "STRATA_ISOLATED_GTK_TEST";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        // Spike: the core has no GTK, assets or icon theme to prepare.
        crate::adapters::volume::mount_watch::start();
        crate::util::ui_thread::mark();
        run();
        return;
    }
    let extra_env: Vec<(std::ffi::OsString, std::ffi::OsString)> = extra_env
        .into_iter()
        .map(|(key, value)| (key.as_ref().to_owned(), value.as_ref().to_owned()))
        .collect();
    // Child processes still share the display's clipboard and pointer grabs.
    static DISPLAY: TestMutex = TestMutex::new();
    let _display = DISPLAY.lock().expect("GTK display lock");
    let sandbox = tempfile::tempdir().expect("isolated preferences");
    let home = sandbox.path().join("home");
    std::fs::create_dir_all(&home).expect("isolated home");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        // The parent already selected this exact case, including explicit --ignored runs.
        .args(["--exact", name, "--nocapture", "--include-ignored"])
        .env(CHILD, name)
        .env("HOME", home)
        .env("XDG_STATE_HOME", sandbox.path().join("state"))
        .env("XDG_CONFIG_HOME", sandbox.path().join("config"))
        .env("XDG_CACHE_HOME", sandbox.path().join("cache"))
        .env("XDG_DATA_HOME", sandbox.path().join("data"));
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let status = command.status().expect("isolated GTK test starts");
    assert!(status.success(), "{name} failed");
}

/// Opens the new file read-only: vfat mounted with `flush` sleeps 100 ms at
/// every close of a file opened for writing.
pub(crate) fn create_empty_file(path: &std::path::Path) -> std::io::Result<()> {
    use rustix::fs::{Mode, OFlags};
    rustix::fs::open(
        path,
        OFlags::CREATE | OFlags::EXCL | OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o644),
    )
    .map(drop)
    .map_err(std::io::Error::from)
}

pub(crate) fn distinct_device_dirs(name: &str) -> Option<(tempfile::TempDir, tempfile::TempDir)> {
    use std::os::unix::fs::MetadataExt;
    let dirs = (|| {
        let first = tempfile::tempdir().ok()?;
        let shm = std::path::Path::new("/dev/shm");
        if !shm.is_dir() {
            return None;
        }
        let second = tempfile::TempDir::new_in(shm).ok()?;
        let first_dev = std::fs::metadata(first.path()).ok()?.dev();
        let second_dev = std::fs::metadata(second.path()).ok()?.dev();
        (first_dev != second_dev).then_some((first, second))
    })();
    if dirs.is_none() {
        assert!(
            std::env::var_os("STRATA_REQUIRE_DEVICE_TESTS").is_none(),
            "{name} requires two filesystems: /dev/shm must be a distinct device from the temp dir"
        );
        eprintln!("Skipping {name}: /dev/shm is not a distinct device from the temp dir");
    }
    dirs
}
