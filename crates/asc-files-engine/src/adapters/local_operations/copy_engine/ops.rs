// SPDX-License-Identifier: MIT
//! The engine's public copy API: synchronous, for a caller's own worker thread. It is the legacy
//! `following`/`copy_local` path without GLib: a cancel flag and progress counters are shared
//! with the caller instead of a `gio::Cancellable` and a main-loop progress follower.

use std::{
    ffi::{OsStr, OsString},
    os::fd::AsFd,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{Copy, Destination, Options, SharedProgress, Source, copy_new, fs, merge, split};
use crate::{
    adapters::local_operations::{leftovers::Deadline, names::Taken},
    error::Error,
};

/// Stops an operation at its next check; every copy step checks between system calls.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Counters a running copy updates and any thread may read.
#[derive(Clone, Default)]
pub struct Progress(Arc<SharedProgress>);

/// One reading of [`Progress`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgressSnapshot {
    pub bytes: u64,
    pub files: usize,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        let reported = self.0.reported();
        ProgressSnapshot {
            bytes: reported.bytes,
            files: reported.files,
        }
    }
}

/// What a copy does when the name it wants is taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Conflict {
    /// Refuse, leaving what holds the name untouched.
    Fail,
    /// Take the next free "name (n)".
    KeepBoth,
    /// Copy a folder's contents into the folder already there. A file whose name is taken inside
    /// it is kept beside the existing one as "name (n)"; nothing in the folder is replaced.
    Merge,
}

/// A finished copy.
#[derive(Debug)]
pub struct Copied {
    /// The name the copy was published under inside the destination folder.
    pub name: OsString,
    pub progress: ProgressSnapshot,
}

/// Copies `source` into the folder `into`, under the source's own name.
///
/// Runs on the calling thread until the copy is published, refused or cancelled. A copy appears
/// under its final name only once it is complete, and nothing that holds the name is replaced.
pub fn copy(
    source: &Path,
    into: &Path,
    conflict: Conflict,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Copied, Error> {
    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?;
    if lands_within(source, into) {
        return Err(crate::adapters::local_operations::placement::place_error(
            crate::adapters::local_operations::placement::PlaceError::IntoItself,
        ));
    }
    let target = into.join(name);
    let destination = match conflict {
        Conflict::Fail => Destination::New(target, Taken::Fail),
        Conflict::KeepBoth => Destination::New(target, Taken::Number),
        Conflict::Merge => Destination::Merge(target, None),
    };
    let (parent, item) = split(source)?;
    let source = Source {
        parent,
        name: item,
        recorded_in: None,
    };
    let copy = Copy::new(
        fs(),
        source.name.clone(),
        cancel.0.clone(),
        progress.0.clone(),
        true,
        Options::default(),
        Deadline::default(),
    );
    let published = match destination {
        Destination::New(target, taken) => {
            let (parent, name) = split(&target)?;
            copy_new(
                &copy,
                &source,
                (&parent, &name),
                super::NewItem::Published(taken),
                None,
            )
            .map(|(name, _ownership)| name)
        }
        Destination::Merge(folder, placing) => {
            let name = folder.file_name().unwrap_or_default().to_os_string();
            merge(&copy, &source, &folder, None, placing).map(|_| name)
        }
    }
    .map_err(|failure| failure.into_error(&source.name));
    Ok(Copied {
        name: published?,
        progress: progress.snapshot(),
    })
}

/// Whether `into` is `source` or beneath it, by device and inode, so bind mounts, symlinked
/// paths and case-insensitive aliases are caught. The legacy app checked this while planning a
/// paste; the engine checks it itself, since a copy into itself never ends.
fn lands_within(source: &Path, into: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(source) = std::fs::symlink_metadata(source) else {
        return false;
    };
    if !source.is_dir() {
        return false;
    }
    let identity = (source.dev(), source.ino());
    into.ancestors().any(|ancestor| {
        std::fs::metadata(ancestor).is_ok_and(|meta| (meta.dev(), meta.ino()) == identity)
    })
}

/// A finished move.
#[derive(Debug)]
pub struct Moved {
    /// The name the item has inside the destination folder.
    pub name: OsString,
    /// Whether the move had to copy because the destination is another filesystem.
    pub copied: bool,
    /// Source entries kept because they changed after the copy read them, or could not be
    /// proven to match it. They stay where they were; the copy holds what was read.
    pub kept: Vec<(std::path::PathBuf, crate::services::KeptCause)>,
    pub progress: ProgressSnapshot,
}

/// Moves `source` into the folder `into` under its own name, never replacing what holds it.
///
/// Within one filesystem this is a rename. Across filesystems it copies, puts the copy on the
/// destination's device, and only then deletes the source entries that are still what the
/// copy read; anything else is kept and reported in [`Moved::kept`].
pub fn move_item(
    source: &Path,
    into: &Path,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    use super::{Barrier, Published, manifest};
    use crate::adapters::local_operations::{
        fs_ops::Entry,
        placement::{self, PlaceError},
    };

    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?
        .to_os_string();
    if lands_within(source, into) {
        return Err(placement::place_error(PlaceError::IntoItself));
    }
    let (source_parent, source_name) = split(source)?;
    let (target_parent, target_name) = split(&into.join(&name))?;
    let expected = fs()
        .stat(Entry::new(&*source_parent, &source_name), false)
        .map_err(crate::error::os_io_error)?
        .identity;
    match placement::place(
        fs(),
        Entry::new(&*source_parent, &source_name),
        Entry::new(&*target_parent, &target_name),
        Some(expected),
    ) {
        Ok(_) => {
            return Ok(Moved {
                name: target_name,
                copied: false,
                kept: Vec::new(),
                progress: progress.snapshot(),
            });
        }
        Err(PlaceError::CrossDevice) => {}
        Err(error) => return Err(placement::place_error(error)),
    }

    // Opened before anything is written beneath `into`, so reaching it covers the whole copy.
    let barrier = Barrier::open(fs(), into)?;
    let root = manifest::Folder::root();
    let source = Source {
        parent: source_parent,
        name: source_name,
        recorded_in: Some(root.clone()),
    };
    let options = Options {
        manifest: true,
        durable: true,
        ..Options::default()
    };
    let copy = Copy::new(
        fs(),
        source.name.clone(),
        cancel.0.clone(),
        progress.0.clone(),
        true,
        options,
        Deadline::default(),
    );
    let (published_name, ownership) = copy_new(
        &copy,
        &source,
        (&target_parent, &target_name),
        super::NewItem::Published(Taken::Fail),
        None,
    )
    .map_err(|failure| failure.into_error(&source.name))?;
    barrier.reach(fs(), false).map_err(|error| {
        crate::error::io_error(rust_i18n::t!(
            "The copy could not be confirmed on its drive, so the original was kept: %{error}",
            error = crate::adapters::local_operations::native_fs::os_error_text(error)
        ))
    })?;
    let mut sources = manifest::of(source.parent.clone(), root);
    sources.copied_to(Published::proven(
        target_parent,
        published_name.clone(),
        ownership,
    ));
    let (kept, deleted) = manifest::delete(
        fs(),
        sources,
        Arc::new(super::Unlinks::default()),
        cancel.0.clone(),
    );
    deleted?;
    Ok(Moved {
        name: published_name,
        copied: true,
        kept,
        progress: progress.snapshot(),
    })
}

pub use crate::trash::Trashed;

/// Moves `item` to the Trash on its own drive; see [`crate::trash::put`].
pub fn trash(item: &Path) -> Result<Trashed, Error> {
    crate::trash::put(item)
}

/// A finished replace.
#[derive(Debug)]
pub struct Replaced {
    pub name: OsString,
    /// Where the replaced original went, or None when nothing held the name.
    pub original: Option<Trashed>,
    pub progress: ProgressSnapshot,
}

/// Copies `source` into `into`, replacing the item that holds its name.
///
/// The copy is built in a hidden stage beside the target. Where the drive can swap two names
/// atomically (`RENAME_EXCHANGE`), the stage and the target trade places, so the name never
/// holds a partial copy or nothing. Elsewhere (FAT, exFAT, many FUSE and network drives) the
/// original is first set aside under a hidden name and the copy then takes the name, which is
/// empty for that moment. Either way the original goes to the Trash only once the copy holds
/// the name, recorded under its own path so it restores to where it was. If any step fails,
/// the steps before it are undone in reverse and the original is back at its name; if even an
/// undo fails, the error says where the original is. Nothing is replaced that is not the item
/// found at the name when the copy began.
pub fn replace(
    source: &Path,
    into: &Path,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Replaced, Error> {
    use crate::{
        adapters::local_operations::{
            fs_ops::Entry,
            placement::{self, PlaceError},
        },
        error::os_io_error,
        model::hidden_names::HiddenName,
    };

    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?
        .to_os_string();
    if lands_within(source, into) {
        return Err(placement::place_error(PlaceError::IntoItself));
    }
    let (parent, target) = split(&into.join(&name))?;
    let found = match fs().stat(Entry::new(&*parent, &target), false) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => {
            let copied = copy(source, into, Conflict::Fail, cancel, progress)?;
            return Ok(Replaced {
                name: copied.name,
                original: None,
                progress: copied.progress,
            });
        }
        Err(error) => return Err(os_io_error(error)),
    };
    let original = found.identity;

    let stage = HiddenName::Replacement.fresh();
    let (source_parent, source_name) = split(source)?;
    let source = Source {
        parent: source_parent,
        name: source_name,
        recorded_in: None,
    };
    let copy = Copy::new(
        fs(),
        source.name.clone(),
        cancel.0.clone(),
        progress.0.clone(),
        true,
        Options::default(),
        Deadline::default(),
    );
    copy_new(
        &copy,
        &source,
        (&parent, &stage),
        super::NewItem::Published(Taken::Fail),
        None,
    )
    .map_err(|failure| failure.into_error(&source.name))?;
    let staged = fs()
        .stat(Entry::new(&*parent, &stage), false)
        .map_err(os_io_error)?
        .identity;
    let discard_stage = || discard(&parent, &stage, staged);
    let target_path = into.join(&target);

    let still_original = fs()
        .stat(Entry::new(&*parent, &target), false)
        .is_ok_and(|stat| stat.identity == original);
    if !still_original {
        discard_stage();
        return Err(placement::place_error(PlaceError::Changed));
    }
    let exchanged = if placement::exchange_allowed(fs(), parent.as_fd()) {
        match placement::exchange(
            fs(),
            Entry::new(&*parent, &stage),
            Entry::new(&*parent, &target),
        ) {
            Ok(swapped) => swapped,
            Err(error) => {
                discard_stage();
                return Err(placement::place_error(error));
            }
        }
    } else {
        false
    };
    let trashed = if exchanged {
        // The original now sits at the stage name. Prove it is still the item that was confirmed.
        let swapped_is_original = fs()
            .stat(Entry::new(&*parent, &stage), false)
            .is_ok_and(|stat| stat.identity == original);
        let trashed = if swapped_is_original {
            crate::trash::put_recording(&into.join(&stage), Some(&target_path))
        } else {
            Err(placement::place_error(PlaceError::Changed))
        };
        if trashed.is_err()
            && placement::exchange(
                fs(),
                Entry::new(&*parent, &stage),
                Entry::new(&*parent, &target),
            )
            .is_ok_and(|swapped| swapped)
        {
            // The original is back at its name; the copy returned to the stage.
            discard_stage();
        }
        trashed?
    } else {
        let is_folder = found.kind == rustix::fs::FileType::Directory;
        replace_by_setting_aside(
            (&parent, into),
            (&stage, staged),
            (&target, original),
            is_folder,
            &discard_stage,
        )?
    };
    Ok(Replaced {
        name: target,
        original: Some(trashed),
        progress: progress.snapshot(),
    })
}

/// The replace for a drive without `RENAME_EXCHANGE`: set the original aside, publish the
/// stage at its name, trash the original. Undoes in reverse on any failure.
fn replace_by_setting_aside(
    (parent, into): (&Arc<std::os::fd::OwnedFd>, &Path),
    (stage, staged): (
        &OsString,
        crate::adapters::local_operations::native_fs::LocalFileIdentity,
    ),
    (target, original): (
        &OsString,
        crate::adapters::local_operations::native_fs::LocalFileIdentity,
    ),
    is_folder: bool,
    discard_stage: &dyn Fn(),
) -> Result<Trashed, Error> {
    use crate::adapters::local_operations::{
        fs_ops::Entry,
        placement::{self, Feasibility, RenameFailure},
    };
    let refuse = |message: &str| {
        Error::new(
            crate::error::ErrorKind::NotSupported,
            &crate::i18n::tr(message),
        )
    };
    match placement::can_set_aside(fs(), parent.as_fd(), is_folder) {
        Ok(Feasibility::Yes) => {}
        Ok(Feasibility::No) => {
            discard_stage();
            return Err(refuse(
                "This drive cannot replace an item without risking it",
            ));
        }
        Err(error) => {
            discard_stage();
            return Err(placement::place_error(error));
        }
    }
    let aside = match placement::set_aside(fs(), Entry::new(&**parent, target), original, None) {
        Ok(aside) => aside,
        Err(RenameFailure::Place(error)) => {
            discard_stage();
            return Err(placement::place_error(error));
        }
        Err(RenameFailure::Stranded { temporary, error }) => {
            discard_stage();
            return Err(kept_at(
                &into.join(temporary),
                &placement::place_error(error),
            ));
        }
    };
    let aside_path = into.join(&aside.originals.name).join(&aside.name);
    let put_back = || placement::put_back(fs(), parent.as_fd(), &aside, target).is_ok();

    if let Err(error) = placement::publish_staged(
        fs(),
        Entry::new(&**parent, stage),
        Entry::new(&**parent, target),
        staged,
    ) {
        let error = placement::place_error(error);
        if !put_back() {
            return Err(kept_at(&aside_path, &error));
        }
        discard_stage();
        return Err(error);
    }
    match crate::trash::put_recording(&aside_path, Some(&into.join(target))) {
        Ok(trashed) => {
            let _ = std::fs::remove_dir(into.join(&aside.originals.name));
            Ok(trashed)
        }
        Err(error) => {
            // The copy goes back to its stage, then the original back to its name.
            let copy_back = placement::place(
                fs(),
                Entry::new(&**parent, target),
                Entry::new(&**parent, stage),
                Some(staged),
            )
            .is_ok();
            if !(copy_back && put_back()) {
                return Err(kept_at(&aside_path, &error));
            }
            discard_stage();
            Err(error)
        }
    }
}

/// An error that leaves the original somewhere other than its name says where it is.
fn kept_at(path: &Path, error: &Error) -> Error {
    Error::new(
        error.kind(),
        &rust_i18n::t!(
            "%{error} The original was kept at %{path}",
            error = error.message(),
            path = path.display().to_string()
        ),
    )
}

/// Removes a stage this module made, only while it is still the item it made.
fn discard(
    parent: &Arc<std::os::fd::OwnedFd>,
    name: &OsStr,
    identity: crate::adapters::local_operations::native_fs::LocalFileIdentity,
) {
    use crate::adapters::local_operations::{
        deletion::{
            PinnedEntry, local_delete_worker_count, parallel_delete_local_blocking_with_workers,
        },
        native_fs::Recorded,
    };
    let roots = vec![PinnedEntry {
        parent: parent.clone(),
        name: name.to_os_string(),
        identity: Some(Recorded {
            identity,
            attributes: None,
        }),
    }];
    let workers = local_delete_worker_count(&roots);
    if let Err(error) = parallel_delete_local_blocking_with_workers(
        roots,
        Arc::new(AtomicBool::new(false)),
        workers,
        &Deadline::default(),
    ) {
        tracing::warn!(?name, %error, "could not remove a replace stage");
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::MetadataExt, path::PathBuf, sync::Mutex};

    use super::*;
    use crate::adapters::local_operations::tests::fs_quirks::{FsQuirks, Op, Quirks};

    /// Serialises the tests here: one of them makes the test home's Trash unusable for a while.
    static TRASH: Mutex<()> = Mutex::new(());

    fn test_home() -> PathBuf {
        let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME set by scripts/test.sh");
        assert!(
            data.contains("asc-files-test-home"),
            "refusing to touch a real Trash: {data}"
        );
        fs::create_dir_all(&data).unwrap();
        PathBuf::from(data)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn fixture(home: &Path, existing_is_folder: bool) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir_in(home).unwrap();
        let (source, dest) = (root.path().join("source"), root.path().join("dest"));
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&dest).unwrap();
        if existing_is_folder {
            fs::create_dir_all(source.join("item/new")).unwrap();
            fs::create_dir_all(dest.join("item/old")).unwrap();
        } else {
            fs::write(source.join("item"), b"new").unwrap();
            fs::write(dest.join("item"), b"old").unwrap();
        }
        (root, source.join("item"), dest)
    }

    #[test]
    fn replace_on_a_drive_without_exchange_sets_the_original_aside_then_trashes_it() {
        let _serial = TRASH.lock().unwrap_or_else(|poison| poison.into_inner());
        let home = test_home();
        for folder in [false, true] {
            let (root, source, dest) = fixture(&home, folder);
            let _fat = FsQuirks::install(root.path(), Quirks::vfat());
            let old_inode = fs::symlink_metadata(dest.join("item")).unwrap().ino();

            let replaced =
                replace(&source, &dest, &Cancel::new(), &Progress::new()).expect("replace");

            if folder {
                assert_eq!(names(&dest.join("item")), ["new"]);
            } else {
                assert_eq!(fs::read(dest.join("item")).unwrap(), b"new");
            }
            assert_eq!(
                names(&dest),
                ["item"],
                "no stage or originals folder is left"
            );
            let trashed = replaced.original.expect("the original went to the Trash");
            assert_eq!(
                fs::symlink_metadata(&trashed.file).unwrap().ino(),
                old_inode
            );
            assert_eq!(
                trashed.file.file_name().unwrap(),
                "item",
                "trashed under its own name"
            );
            let entry = crate::trash::list()
                .into_iter()
                .find(|entry| entry.info == trashed.info)
                .expect("listed");
            assert_eq!(
                entry.original,
                dest.join("item"),
                "it restores to where it was"
            );
            crate::trash::delete(&entry).unwrap();
        }
    }

    #[test]
    fn a_publish_that_fails_after_the_set_aside_puts_the_original_back() {
        let _serial = TRASH.lock().unwrap_or_else(|poison| poison.into_inner());
        let (root, source, dest) = fixture(&test_home(), false);
        let _fat = FsQuirks::install(
            root.path(),
            Quirks {
                fail: vec![(
                    Op::Rename,
                    ".asc-files-replacement-*",
                    0,
                    rustix::io::Errno::IO,
                )],
                ..Quirks::vfat()
            },
        );
        let old_inode = fs::metadata(dest.join("item")).unwrap().ino();

        let result = replace(&source, &dest, &Cancel::new(), &Progress::new());

        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read(dest.join("item")).unwrap(), b"old");
        assert_eq!(fs::metadata(dest.join("item")).unwrap().ino(), old_inode);
        assert_eq!(
            names(&dest),
            ["item"],
            "the stage and the originals folder are gone"
        );
    }

    #[test]
    fn a_trash_that_fails_after_the_publish_puts_both_back() {
        let _serial = TRASH.lock().unwrap_or_else(|poison| poison.into_inner());
        let home = test_home();
        let (root, source, dest) = fixture(&home, false);
        let trash = home.join("Trash");
        let _ = fs::remove_dir_all(&trash);
        fs::write(&trash, b"not a folder").unwrap();
        let _fat = FsQuirks::install(root.path(), Quirks::vfat());
        let old_inode = fs::metadata(dest.join("item")).unwrap().ino();

        let result = replace(&source, &dest, &Cancel::new(), &Progress::new());
        fs::remove_file(&trash).unwrap();

        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read(dest.join("item")).unwrap(), b"old");
        assert_eq!(fs::metadata(dest.join("item")).unwrap().ino(), old_inode);
        assert_eq!(
            names(&dest),
            ["item"],
            "the copy went back to its stage and was removed"
        );
    }
}
