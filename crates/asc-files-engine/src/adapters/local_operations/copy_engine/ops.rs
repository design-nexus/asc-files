// SPDX-License-Identifier: MIT
//! The engine's public copy API: synchronous, for a caller's own worker thread. It is the legacy
//! `following`/`copy_local` path without GLib: a cancel flag and progress counters are shared
//! with the caller instead of a `gio::Cancellable` and a main-loop progress follower.

use std::{
    ffi::OsString,
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
/// The copy is built in a hidden stage beside the target and swapped in atomically
/// (`RENAME_EXCHANGE`), so the name never holds a partial copy or nothing. Only then does the
/// original go to the Trash; if it cannot, the swap is undone and the original stays. Nothing
/// is replaced that is not the item found at the name when the copy began.
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
        error::{ErrorKind, os_io_error},
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
    let original = match fs().stat(Entry::new(&*parent, &target), false) {
        Ok(stat) => stat.identity,
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
    let stage_path = into.join(&stage);
    let discard_stage = || remove_tree(&stage_path);

    let still_original = fs()
        .stat(Entry::new(&*parent, &target), false)
        .is_ok_and(|stat| stat.identity == original);
    if !still_original {
        discard_stage();
        return Err(placement::place_error(PlaceError::Changed));
    }
    match placement::exchange(
        fs(),
        Entry::new(&*parent, &stage),
        Entry::new(&*parent, &target),
    ) {
        Ok(true) => {}
        Ok(false) => {
            discard_stage();
            return Err(Error::new(
                ErrorKind::NotSupported,
                &crate::i18n::tr("This drive cannot swap items in place"),
            ));
        }
        Err(error) => {
            discard_stage();
            return Err(placement::place_error(error));
        }
    }
    // The original now sits at the stage name. Prove it is still the item that was confirmed.
    let swapped_is_original = fs()
        .stat(Entry::new(&*parent, &stage), false)
        .is_ok_and(|stat| stat.identity == original);
    let trashed = if swapped_is_original {
        trash(&stage_path)
    } else {
        Err(placement::place_error(PlaceError::Changed))
    };
    match trashed {
        Ok(trashed) => Ok(Replaced {
            name: target,
            original: Some(trashed),
            progress: progress.snapshot(),
        }),
        Err(error) => {
            // Put the original back at its name; the copy returns to the stage and goes.
            if placement::exchange(
                fs(),
                Entry::new(&*parent, &stage),
                Entry::new(&*parent, &target),
            )
            .is_ok_and(|swapped| swapped)
            {
                discard_stage();
            }
            Err(error)
        }
    }
}

/// Spike: removes a stage this module made. Production uses the engine's pinned deletion.
fn remove_tree(path: &Path) {
    let removed = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(_) => Ok(()),
    };
    if let Err(error) = removed {
        tracing::warn!(?path, %error, "could not remove a replace stage");
    }
}
