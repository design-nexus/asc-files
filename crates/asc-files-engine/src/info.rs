// SPDX-License-Identifier: MIT
//! What Properties shows of an item beyond its listing: how much a folder holds, measured as
//! legacy Files measured it, and the item's permissions, which it can change.
//!
//! A folder is walked depth first without following links: every entry counts as an item,
//! regular files' sizes add up, and files and folders are counted unless hidden (a dot name)
//! or inside a hidden folder. A folder that can't be read, the depth limit (64) or the time
//! limit (five minutes) leave the totals incomplete, never failed; a cancel stops the walk.

use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    error::{Error, ErrorKind, os_io_error},
    ops::Cancel,
};

/// Why a measurement's totals are less than the whole.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Issues {
    pub unreadable: bool,
    pub timed_out: bool,
    pub depth_limited: bool,
}

/// What a folder holds, so far or in all.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Summary {
    pub items: u64,
    pub size: u64,
    pub files: u64,
    pub folders: u64,
    pub issues: Issues,
    pub cancelled: bool,
}

impl Summary {
    pub fn incomplete(&self) -> bool {
        self.issues.unreadable
            || self.issues.timed_out
            || self.issues.depth_limited
            || self.cancelled
    }
}

/// How far a measurement goes.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub depth: usize,
    pub time: Duration,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            depth: 64,
            time: Duration::from_secs(300),
        }
    }
}

/// Entries between reports of the totals so far.
const PROGRESS_EVERY: u64 = 64;

/// Measures the folder `root`. `progress` gets the totals as they grow. Err only when `root`
/// itself can't be looked at.
pub fn measure(
    root: &Path,
    budget: Budget,
    cancel: &Cancel,
    mut progress: impl FnMut(&Summary),
) -> Result<Summary, Error> {
    let meta = std::fs::symlink_metadata(root).map_err(os_io_error)?;
    if !meta.is_dir() {
        return Err(Error::new(ErrorKind::NotDirectory, "Not a folder"));
    }
    let deadline = Instant::now() + budget.time;
    let mut total = Summary::default();
    progress(&total);
    // Folders to read: where, how deep their entries are, whether a folder above is hidden.
    let mut pending: Vec<(PathBuf, usize, bool)> = vec![(root.to_path_buf(), 1, false)];
    'walk: while let Some((folder, depth, hidden_above)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            total.issues.unreadable = true;
            continue;
        };
        for entry in entries {
            if cancel.is_cancelled() {
                total.cancelled = true;
                break 'walk;
            }
            if Instant::now() >= deadline {
                total.issues.timed_out = true;
                break 'walk;
            }
            let Ok(entry) = entry else {
                total.issues.unreadable = true;
                continue;
            };
            // Not followed: a link is an entry of its own.
            let Ok(meta) = entry.metadata() else {
                total.issues.unreadable = true;
                continue;
            };
            let hidden = hidden_above || entry.file_name().as_encoded_bytes().starts_with(b".");
            let is_folder = meta.is_dir();
            total.items += 1;
            if meta.is_file() {
                total.size = total.size.saturating_add(meta.len());
            }
            if !hidden {
                if is_folder {
                    total.folders += 1;
                } else {
                    total.files += 1;
                }
            }
            if is_folder {
                if depth >= budget.depth {
                    total.issues.depth_limited = true;
                } else {
                    pending.push((entry.path(), depth + 1, hidden));
                }
            }
            if total.items % PROGRESS_EVERY == 0 {
                progress(&total);
            }
        }
    }
    progress(&total);
    Ok(total)
}

/// The permission bits of `path` (as `ls -l` reads them, setuid and sticky included), not
/// following a link.
pub fn mode(path: &Path) -> Result<u32, Error> {
    Ok(std::fs::symlink_metadata(path).map_err(os_io_error)?.mode() & 0o7777)
}

/// Sets the read, write and run bits of `path` for owner, group and others to `bits`; the
/// others (setuid, setgid, sticky) stay. A link is refused, not followed: the item is opened
/// without following one and changed through its own descriptor, so it can't be swapped for a
/// link meanwhile.
pub fn set_mode(path: &Path, bits: u32) -> Result<u32, Error> {
    use rustix::fs::{FileType, Mode, OFlags};
    let item = rustix::fs::open(
        path,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(os_io_error)?;
    let stat = rustix::fs::fstat(&item).map_err(os_io_error)?;
    if FileType::from_raw_mode(stat.st_mode) == FileType::Symlink {
        return Err(Error::new(
            ErrorKind::NotSupported,
            "A link's own permissions can't be changed",
        ));
    }
    let mode = (stat.st_mode & 0o7000) | (bits & 0o777);
    // An O_PATH descriptor can't be fchmod'ed; its /proc entry names the item itself.
    let held = PathBuf::from(format!(
        "/proc/self/fd/{}",
        std::os::fd::AsRawFd::as_raw_fd(&item)
    ));
    std::fs::set_permissions(&held, std::fs::Permissions::from_mode(mode)).map_err(os_io_error)?;
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::create_dir_all(dir.join(".hidden/inner")).unwrap();
        fs::write(dir.join("one.txt"), b"12345").unwrap();
        fs::write(dir.join("a/two.txt"), b"123").unwrap();
        fs::write(dir.join("a/b/.dot"), b"12").unwrap();
        fs::write(dir.join(".hidden/inner/three"), b"1").unwrap();
        std::os::unix::fs::symlink(dir.join("a"), dir.join("link")).unwrap();
        root
    }

    #[test]
    fn a_folder_is_measured_as_legacy_measured_it() {
        let root = tree();
        let mut reports = 0;
        let summary = measure(root.path(), Budget::default(), &Cancel::new(), |_| {
            reports += 1
        })
        .unwrap();
        // Items: one.txt a a/two.txt a/b a/b/.dot .hidden .hidden/inner .hidden/inner/three link.
        assert_eq!(summary.items, 9);
        assert_eq!(summary.size, 5 + 3 + 2 + 1);
        // Shown: one.txt, a/two.txt, link; a, a/b. Hidden ones and those under them aren't.
        assert_eq!(summary.files, 3);
        assert_eq!(summary.folders, 2);
        assert!(!summary.incomplete());
        assert!(reports >= 2);
        assert!(
            measure(
                &root.path().join("one.txt"),
                Budget::default(),
                &Cancel::new(),
                |_| {}
            )
            .is_err()
        );
    }

    // Legacy's measuring tests (`adapters/directory_summary/tests.rs`).
    #[test]
    fn a_summary_is_incomplete_once_the_time_budget_is_spent() {
        let root = tree();
        let budget = Budget {
            time: Duration::ZERO,
            ..Budget::default()
        };
        let summary = measure(root.path(), budget, &Cancel::new(), |_| {}).unwrap();
        assert!(summary.issues.timed_out);
        assert!(summary.incomplete());
    }

    #[test]
    fn a_summary_does_not_go_past_the_depth_budget_and_siblings_still_count() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("deep/deeper/deepest")).unwrap();
        fs::write(root.path().join("deep/deeper/deepest/file"), b"x").unwrap();
        fs::create_dir(root.path().join("sibling")).unwrap();
        fs::write(root.path().join("sibling/file"), b"x").unwrap();
        let budget = Budget {
            depth: 2,
            ..Budget::default()
        };
        let summary = measure(root.path(), budget, &Cancel::new(), |_| {}).unwrap();
        assert!(summary.issues.depth_limited);
        // deep, deep/deeper, sibling, sibling/file; nothing below deep/deeper.
        assert_eq!(summary.items, 4);
        assert_eq!(summary.files, 1);
    }

    #[test]
    fn an_unreadable_folder_leaves_the_summary_incomplete_not_failed() {
        let root = tree();
        let locked = root.path().join("a/b");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable = fs::read_dir(&locked).is_ok();
        let summary = measure(root.path(), Budget::default(), &Cancel::new(), |_| {});
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let summary = summary.unwrap();
        // Root reads everything anyway.
        if !readable {
            assert!(summary.issues.unreadable);
            assert_eq!(summary.items, 8);
        }
    }

    #[test]
    fn a_folder_removed_before_it_is_read_leaves_the_summary_incomplete() {
        // The root holds 128 entries, so a report comes just as its listing ends: the folder
        // in it is known then, not yet read.
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("gone");
        fs::create_dir(&gone).unwrap();
        fs::write(gone.join("inside"), b"x").unwrap();
        for index in 0..127 {
            fs::write(root.path().join(format!("f{index}")), b"x").unwrap();
        }
        let summary = measure(root.path(), Budget::default(), &Cancel::new(), |so_far| {
            if so_far.items == 2 * PROGRESS_EVERY && gone.exists() {
                fs::remove_dir_all(&gone).unwrap();
            }
        })
        .unwrap();
        assert!(summary.issues.unreadable, "{summary:?}");
        assert_eq!(summary.items, 128);
    }

    #[test]
    fn a_cancel_stops_a_measurement_mid_flight() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..500 {
            fs::write(root.path().join(format!("f{index}")), b"x").unwrap();
        }
        let cancel = Cancel::new();
        let summary = measure(root.path(), Budget::default(), &cancel, |so_far| {
            if so_far.items >= PROGRESS_EVERY {
                cancel.cancel();
            }
        })
        .unwrap();
        assert!(summary.cancelled);
        assert!(summary.items < 500);
    }

    #[test]
    fn permissions_change_without_following_links_or_losing_special_bits() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("script");
        fs::write(&file, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(set_mode(&file, 0o755).unwrap(), 0o755);
        assert_eq!(mode(&file).unwrap(), 0o755);
        // Sticky stays.
        let folder = root.path().join("shared");
        fs::create_dir(&folder).unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o1777)).unwrap();
        assert_eq!(set_mode(&folder, 0o755).unwrap(), 0o1755);
        // A link is refused, and what it points to is untouched.
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(
            set_mode(&link, 0o600)
                .unwrap_err()
                .matches(ErrorKind::NotSupported)
        );
        assert_eq!(mode(&file).unwrap(), 0o755);
        assert!(
            set_mode(&root.path().join("missing"), 0o600)
                .unwrap_err()
                .matches(ErrorKind::NotFound)
        );
    }
}
