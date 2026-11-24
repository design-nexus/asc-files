// SPDX-License-Identifier: MIT
//! The engine's public copy API: synchronous, for a caller's own worker thread. It is the legacy
//! `following`/`copy_local` path without GLib: a cancel flag and progress counters are shared
//! with the caller instead of a `gio::Cancellable` and a main-loop progress follower.

use std::{
    ffi::{OsStr, OsString},
    os::fd::AsFd,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{Copy, Destination, Options, SharedProgress, Source, copy_new, fs, lock, merge, split};
pub use crate::services::{NameMode, RenameCause, SpecialKind};
use crate::{
    adapters::local_operations::{leftovers::Deadline, mount_profile::NamePolicy, names::Taken},
    error::Error,
};

/// Stops an operation at its next check; every copy step checks between system calls.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// A cancel that reads `flag`, which the caller may set.
    pub(crate) fn sharing(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Conflict {
    /// Refuse, leaving what holds the name untouched.
    Fail,
    /// Take the next free "name (n)".
    KeepBoth,
    /// Copy a folder's contents into the folder already there. A file whose name is taken inside
    /// it is kept beside the existing one as "name (n)"; nothing in the folder is replaced.
    Merge,
}

/// A name the destination could not hold, and the name written instead. Paths are relative to
/// the destination folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Renamed {
    pub requested: PathBuf,
    pub saved_as: PathBuf,
    pub cause: RenameCause,
}

/// An entry the destination could not hold at all, so it was left out of the copy. The path is
/// relative to the destination folder, where it would have been.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Skipped {
    pub path: PathBuf,
    pub kind: SpecialKind,
}

/// A source entry a move left in place, with why: it changed after its copy read it, it could
/// not be deleted, or nothing proved it matched its copy. The path is absolute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Kept {
    pub path: PathBuf,
    pub cause: crate::services::KeptCause,
}

/// What an operation did otherwise than it was asked to: empty unless the destination refused a
/// name or an entry, or a move kept part of its source.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Changes {
    pub renamed: Vec<Renamed>,
    pub skipped: Vec<Skipped>,
    pub kept: Vec<Kept>,
    /// A replace deleted the original, its drive having no Trash; see [`Replaced`].
    pub original_deleted: bool,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.renamed.is_empty()
            && self.skipped.is_empty()
            && self.kept.is_empty()
            && !self.original_deleted
    }

    /// What `copy` noted, for the item published as `published` in the destination folder.
    fn of(copy: &Copy, published: &OsStr) -> Self {
        let left = std::mem::take(&mut *lock(&copy.left));
        let root = Path::new(published);
        let item = left.item.map(|(requested, cause)| Renamed {
            requested: PathBuf::from(requested),
            saved_as: root.to_path_buf(),
            cause,
        });
        Self {
            renamed: item
                .into_iter()
                .chain(
                    left.renamed
                        .into_iter()
                        .map(|(requested, saved_as, cause)| Renamed {
                            requested: root.join(requested),
                            saved_as: root.join(saved_as),
                            cause,
                        }),
                )
                .collect(),
            skipped: left
                .skipped
                .into_iter()
                .map(|(path, kind)| Skipped {
                    path: root.join(path),
                    kind,
                })
                .collect(),
            kept: Vec::new(),
            original_deleted: false,
        }
    }
}

fn with_names(names: NameMode) -> Options {
    Options {
        names: NamePolicy::new(names),
        ..Options::default()
    }
}

/// A finished copy.
#[derive(Debug)]
pub struct Copied {
    /// The name the copy was published under inside the destination folder.
    pub name: OsString,
    pub changes: Changes,
    pub progress: ProgressSnapshot,
}

/// Copies `source` into the folder `into`, under the source's own name.
///
/// Runs on the calling thread until the copy is published, refused or cancelled. A copy appears
/// under its final name only once it is complete, and nothing that holds the name is replaced.
///
/// A name the destination drive cannot store (FAT's `:`, a trailing dot, `CON`, one too long) is
/// written as the closest name it can, and listed in [`Copied::changes`]. The rules come from the
/// drive's own mount (mountinfo, udev and `statfs`). That holds for everything inside a copied
/// folder; for the item's own name it holds with [`NameMode::Fix`], the mode for ordinary pastes.
/// [`NameMode::Keep`] is for giving a name back, as undo does: there a refused name fails the item
/// rather than changing it.
///
/// A copy into the folder the source is in is a duplicate: it takes the next free "name (n)"
/// whatever `conflict` says, since its own name is always taken there.
pub fn copy(
    source: &Path,
    into: &Path,
    conflict: Conflict,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Copied, Error> {
    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?;
    let conflict = if conflict == Conflict::Fail && in_its_own_folder(source, into) {
        Conflict::KeepBoth
    } else {
        conflict
    };
    copy_as(source, (into, name), conflict, names, cancel, progress)
}

/// A name given for an item must be one plain name: not empty, `.` or `..`, and without `/` or
/// NUL. Worded as legacy's rename and new-item prompts said it.
fn plain_name(name: &OsStr) -> Result<(), Error> {
    let message = match name.to_str() {
        Some(text) => crate::services::validate_basename(text).err(),
        None => {
            use std::os::unix::ffi::OsStrExt;
            let bytes = name.as_bytes();
            (bytes.contains(&b'/') || bytes.contains(&0)).then_some("Names cannot contain /")
        }
    };
    match message {
        Some(message) => Err(Error::new(
            crate::error::ErrorKind::InvalidFilename,
            &crate::i18n::tr(message),
        )),
        None => Ok(()),
    }
}

/// Makes a new, empty folder or file called `name` in `into`, never over anything, and returns
/// where. With `unique`, as for a default name such as "New Folder", a taken name takes the next
/// free "name (n)"; without, as for a name the user typed, a taken name fails, and a name the
/// drive would store otherwise is refused before anything is made.
pub fn create(into: &Path, name: &str, folder: bool, unique: bool) -> Result<PathBuf, Error> {
    use crate::adapters::local_operations::{
        fs_ops::Entry, mount_profile::name_limit, names::numbered_name,
        native_fs::open_local_parent_directory, typed_names,
    };
    plain_name(name.as_ref())?;
    if !unique && let Some(reason) = typed_names::refused_in(into, name, folder) {
        return Err(Error::new(
            crate::error::ErrorKind::InvalidFilename,
            &reason,
        ));
    }
    let dir = open_local_parent_directory(into).map_err(crate::error::io_error)?;
    let mut limit = None;
    for number in 0u64.. {
        let candidate = match number {
            0 => OsString::from(name),
            _ => {
                let limit = *limit.get_or_insert_with(|| name_limit(fs(), dir.as_fd()));
                numbered_name(name.as_ref(), number, OsStr::new(""), limit, folder)
            }
        };
        let at = Entry::new(&dir, &candidate);
        let made = if folder {
            fs().mkdir(at, rustix::fs::Mode::from_raw_mode(0o777))
        } else {
            fs().create_exclusive(at, rustix::fs::Mode::from_raw_mode(0o666))
                .map(drop)
        };
        // SMB with POSIX extensions fails an exclusive file create over a folder with EISDIR.
        let taken = |errno: rustix::io::Errno| {
            errno == rustix::io::Errno::EXIST || (!folder && errno == rustix::io::Errno::ISDIR)
        };
        match made {
            Ok(()) => return Ok(into.join(candidate)),
            Err(errno) if taken(errno) && unique && number < u64::MAX => {}
            Err(errno) if taken(errno) => {
                return Err(Error::new(
                    crate::error::ErrorKind::Exists,
                    &rust_i18n::t!(
                        "“%{name}” already exists",
                        name = candidate.to_string_lossy()
                    ),
                ));
            }
            Err(errno) => {
                let error = crate::error::os_io_error(errno);
                return Err(Error::new(
                    error.kind(),
                    &rust_i18n::t!(
                        "Could not create “%{name}”: %{error}",
                        name = candidate.to_string_lossy(),
                        error = crate::services::error_detail(error.message().to_owned())
                    ),
                ));
            }
        }
    }
    unreachable!("a free name is found or the loop fails")
}

/// Whether two paths name the same item, by identity, without following a link, as the engine's
/// filesystem sees them: a case-insensitive drive takes `photo.txt` for its `Photo.txt`.
fn same_item(one: &Path, other: &Path) -> bool {
    use crate::adapters::local_operations::{
        fs_ops::Entry, native_fs::open_local_parent_directory,
    };
    let identity = |path: &Path| {
        let (folder, name) = (path.parent()?, path.file_name()?);
        let dir = open_local_parent_directory(folder).ok()?;
        fs().stat(Entry::new(&dir, name), false)
            .ok()
            .map(|stat| stat.identity)
    };
    // A hard link elsewhere has the same identity but is another name: only the same folder and
    // a name differing at most in case is the item itself.
    let folder = |path: &Path| path.parent().and_then(|parent| identity_of_folder(parent));
    let spelled_alike = match (one.file_name(), other.file_name()) {
        (Some(a), Some(b)) => {
            a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
        }
        _ => false,
    };
    spelled_alike
        && folder(one).is_some_and(|one| folder(other) == Some(one))
        && identity(one).is_some_and(|one| identity(other) == Some(one))
}

fn identity_of_folder(folder: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(folder)
        .ok()
        .map(|meta| (meta.dev(), meta.ino()))
}

/// Whether `into` is the folder `source` is in, by identity, so another spelling of the same
/// folder counts.
fn in_its_own_folder(source: &Path, into: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let identity = |path: &Path| {
        std::fs::metadata(path)
            .ok()
            .map(|meta| (meta.dev(), meta.ino()))
    };
    source
        .parent()
        .and_then(identity)
        .is_some_and(|parent| identity(into) == Some(parent))
}

/// Copies `source` into the folder `into` under `name`, as [`copy`] does under its own: the name a
/// conflict prompt offered, say.
pub fn copy_as(
    source: &Path,
    (into, name): (&Path, &OsStr),
    conflict: Conflict,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Copied, Error> {
    plain_name(name)?;
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
        with_names(names),
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
    .map_err(|failure| failure.into_error(&source.name))?;
    Ok(Copied {
        changes: Changes::of(&copy, &published),
        name: published,
        progress: progress.snapshot(),
    })
}

/// Whether `into` is `source` or beneath it, by device and inode, so bind mounts, symlinked
/// paths and case-insensitive aliases are caught. The legacy app checked this while planning a
/// paste; the engine checks it itself, since a copy into itself never ends.
pub(crate) fn lands_within(source: &Path, into: &Path) -> bool {
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
    /// What the copy across filesystems wrote otherwise than asked, and what of the source it
    /// left in place; see [`copy`].
    pub changes: Changes,
    pub progress: ProgressSnapshot,
    /// Set when the copy is in place but deleting the sources stopped (cancelled, or failed):
    /// what is left of them stays, and [`Moved::changes`] says what was kept on purpose.
    pub stopped: Option<Error>,
}

impl Moved {
    /// The move as a whole: an error where deleting its sources stopped.
    pub fn finished(self) -> Result<Self, Error> {
        match self.stopped {
            Some(error) => Err(error),
            None => Ok(self),
        }
    }
}

/// Moves `source` into the folder `into` under its own name, never replacing what holds it.
///
/// Within one filesystem this is a rename. Across filesystems it copies, puts the copy on the
/// destination's device, and only then deletes the source entries that are still what the
/// copy read; anything else is kept and reported in [`Moved::kept`].
pub fn move_item(
    source: &Path,
    into: &Path,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?;
    move_as(source, (into, name), names, cancel, progress)
}

/// Moves `source` into the folder `into`, under its own name if that is free and otherwise the
/// next free "name (n)", as Keep Both does for a copy.
pub fn move_keeping_both(
    source: &Path,
    into: &Path,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?;
    move_keeping_both_as(source, (into, name), names, cancel, progress)
}

/// Moves `source` into `into` as `name`, or the next free "name (n)" where that is taken.
pub fn move_keeping_both_as(
    source: &Path,
    (into, name): (&Path, &OsStr),
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    match move_as(source, (into, name), names, cancel, progress) {
        Err(error) if error.kind() == crate::error::ErrorKind::Exists => {}
        moved => return moved,
    }
    move_numbered_as(source, (into, name), names, cancel, progress)
}

/// Moves `source` into the folder `into` under the first free "name (n)", where its own name is
/// known to be taken.
pub fn move_numbered(
    source: &Path,
    into: &Path,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?;
    move_numbered_as(source, (into, name), names, cancel, progress)
}

/// Moves `source` into `into` under the first free "name (n)" after `name`.
pub fn move_numbered_as(
    source: &Path,
    (into, name): (&Path, &OsStr),
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    use crate::adapters::local_operations::{mount_profile, names::numbered_names};
    let exists = |error: &Error| error.kind() == crate::error::ErrorKind::Exists;
    let folder = crate::adapters::local_operations::native_fs::open_local_parent_directory(into)
        .map_err(crate::error::io_error)?;
    let limit = mount_profile::name_limit(fs(), folder.as_fd());
    let is_directory = std::fs::symlink_metadata(source).is_ok_and(|meta| meta.is_dir());
    let mut last = None;
    for candidate in numbered_names(name, is_directory, limit) {
        match move_as(source, (into, &candidate), names, cancel, progress) {
            Err(error) if exists(&error) => last = Some(error),
            moved => return moved,
        }
    }
    Err(last.unwrap_or_else(|| crate::error::translated_io_error("No free name was left")))
}

/// Moves `source` into the folder `into` under `name`, as [`move_item`] does under its own; undo
/// uses it to give an item back the name it had.
pub fn move_as(
    source: &Path,
    (into, name): (&Path, &OsStr),
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    moving(source, (into, name), (names, false), cancel, progress)
}

/// Renames `item` to `name` within its folder, never over what holds that name. A failure gives
/// its reason, as a rename prompt shows it.
pub fn rename(
    item: &Path,
    name: &OsStr,
    _names: NameMode,
    cancel: &Cancel,
) -> Result<Moved, Error> {
    use crate::adapters::local_operations::{
        fs_ops::Entry,
        placement::{self, PlaceError, RenameFailure},
    };
    plain_name(name)?;
    let (parent, from_name) = split(item)?;
    let from = Entry::new(&*parent, &from_name);
    let to = Entry::new(&*parent, name);
    // A name differing only in case on a drive that ignores case is the item itself: it is set
    // aside and placed back under the new spelling. An SMB folder refused while a file below it
    // waits for the client's deferred close is tried again until that closes.
    let renamed = crate::adapters::local_operations::deferred_close::retry_folder_rename(
        fs(),
        from,
        &Deadline::default(),
        &|| cancel.is_cancelled(),
        |failure: &RenameFailure| {
            matches!(
                failure,
                RenameFailure::Place(PlaceError::Io(rustix::io::Errno::ACCESS))
            )
        },
        || placement::rename_in_place(fs(), from, to),
    )
    .map_err(|_| crate::error::cancelled())?;
    match renamed {
        Ok(_) => Ok(Moved {
            name: name.to_os_string(),
            copied: false,
            kept: Vec::new(),
            changes: Changes::default(),
            progress: ProgressSnapshot::default(),
            stopped: None,
        }),
        Err(RenameFailure::Place(PlaceError::Exists)) => Err(Error::new(
            crate::error::ErrorKind::Exists,
            &rust_i18n::t!("“%{name}” already exists", name = name.to_string_lossy()),
        )),
        Err(RenameFailure::Place(error)) => Err(move_failure(item, true, from, error)),
        Err(RenameFailure::Stranded { temporary, error }) => {
            Err(crate::error::io_error(rust_i18n::t!(
                "Could not rename “%{name}”; it was left as “%{temporary}”: %{error}",
                name = from_name.to_string_lossy(),
                temporary = temporary.to_string_lossy(),
                error = crate::adapters::local_operations::placement_reason(error)
            )))
        }
    }
}

/// Renames `item` to `name` as the user typed it: refused, before anything changes, where its
/// folder's drive would store another name (a character FAT cannot hold, a name too long for the
/// share, a reserved word); otherwise [`rename`], keeping the name exactly.
pub fn rename_typed(item: &Path, name: &str, cancel: &Cancel) -> Result<Moved, Error> {
    let folder = item
        .parent()
        .ok_or_else(|| crate::error::translated_io_error("The item has no folder"))?;
    // An item renamed is not a new folder, which some shares make under a longer name first.
    if let Some(reason) =
        crate::adapters::local_operations::typed_names::refused_in(folder, name, false)
    {
        return Err(Error::new(
            crate::error::ErrorKind::InvalidFilename,
            &reason,
        ));
    }
    rename(item, OsStr::new(name), NameMode::Keep, cancel)
}

fn moving(
    source: &Path,
    (into, name): (&Path, &OsStr),
    (names, renaming): (NameMode, bool),
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    use super::{Barrier, Published, manifest};
    use crate::adapters::local_operations::{
        fs_ops::Entry,
        placement::{self, PlaceError},
    };
    let source_path = source.to_path_buf();

    plain_name(name)?;
    let name = name.to_os_string();
    // Moving an item to where it already is changes nothing, as cutting and pasting in one folder.
    if same_item(source, &into.join(&name)) && source.file_name() == Some(name.as_os_str()) {
        return Ok(Moved {
            name,
            copied: false,
            kept: Vec::new(),
            changes: Changes::default(),
            progress: progress.snapshot(),
            stopped: None,
        });
    }
    if lands_within(source, into) {
        return Err(placement::place_error(PlaceError::IntoItself));
    }
    let (source_parent, source_name) = split(source)?;
    let (target_parent, target_name) = split(&into.join(&name))?;
    let expected = fs()
        .stat(Entry::new(&*source_parent, &source_name), false)
        .map_err(crate::error::os_io_error)?
        .identity;
    let from = Entry::new(&*source_parent, &source_name);
    // An SMB folder is refused while a file below it waits for the client's deferred close; the
    // rename is tried again until that closes.
    let placed = crate::adapters::local_operations::deferred_close::retry_folder_rename(
        fs(),
        from,
        &Deadline::default(),
        &|| cancel.is_cancelled(),
        |error: &PlaceError| matches!(error, PlaceError::Io(rustix::io::Errno::ACCESS)),
        || {
            placement::place(
                fs(),
                from,
                Entry::new(&*target_parent, &target_name),
                Some(expected),
            )
        },
    )
    .map_err(|_| crate::error::cancelled())?;
    match placed {
        Ok(_) => {
            return Ok(Moved {
                name: target_name,
                copied: false,
                kept: Vec::new(),
                changes: Changes::default(),
                progress: progress.snapshot(),
                stopped: None,
            });
        }
        // No rename can place it here: it is copied, and the source deleted once proven.
        Err(PlaceError::CrossDevice | PlaceError::Unsupported) => {}
        Err(error) => return Err(move_failure(source, renaming, from, error)),
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
        ..with_names(names)
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
            "The device could not finish writing: %{error}. Earlier writes may still be pending; wait for safe eject before unplugging.",
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
    let mut changes = Changes::of(&copy, &published_name);
    let folder = source_path.parent().unwrap_or(Path::new("/"));
    changes.kept = kept
        .iter()
        .map(|(path, cause)| Kept {
            path: if path.is_absolute() {
                path.clone()
            } else {
                folder.join(path)
            },
            cause: cause.clone(),
        })
        .collect();
    Ok(Moved {
        changes,
        stopped: deleted.err(),
        name: published_name,
        copied: true,
        kept,
        progress: progress.snapshot(),
    })
}

/// What [`put_back`] did.
pub(crate) struct PutBack {
    /// What the item was in the Trash, read before it moved.
    pub trashed: crate::services::ItemIdentity,
    /// Entries left in the Trash because they changed after the copy read them, or could not
    /// be deleted, with why. Empty when the item was moved back whole.
    pub kept: Vec<Kept>,
}

/// Puts `item` back at `original`, never over what holds that name now and never onto another
/// drive. The original's folder is opened beneath `root` (the drive's mount point), so a
/// symlink swapped into its path cannot lead the item elsewhere. Where no rename can place it (a
/// drive without no-replace renames for a folder, say), it is copied, the copy reaches the drive,
/// and only then is what was copied deleted from the Trash; a cancel after the copy is complete
/// no longer stops it.
pub(crate) fn put_back(
    item: &Path,
    original: &Path,
    root: &Path,
    cancel: &Cancel,
) -> Result<PutBack, Error> {
    use super::{Barrier, Published, manifest};
    use crate::adapters::local_operations::{
        fs_ops::Entry,
        mount_profile::identity_at,
        native_fs::{open_local_parent_beneath, os_error_detail, trashed_identity},
        placement::{self, PlaceError},
    };
    let display = item
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let failed = |error: &str| {
        crate::error::io_error(rust_i18n::t!(
            "Could not restore %{name}: %{error}",
            name = display,
            error = error
        ))
    };
    let exists = || {
        Error::new(
            crate::error::ErrorKind::Exists,
            &rust_i18n::t!(
                "Could not restore %{name}: something already exists at the destination",
                name = display
            ),
        )
    };
    if cancel.is_cancelled() {
        return Err(crate::error::cancelled());
    }
    let (source_parent, source_name) = split(item)?;
    let (Some(target_folder), Some(target_name)) = (original.parent(), original.file_name()) else {
        return Err(crate::error::translated_io_error(
            "Invalid restore destination",
        ));
    };
    let target_parent =
        Arc::new(open_local_parent_beneath(target_folder, root).map_err(|error| failed(&error))?);
    let from = Entry::new(&*source_parent, &source_name);
    let stat = fs()
        .stat(from, false)
        .map_err(|error| failed(&os_error_detail(error)))?;
    let trashed = trashed_identity(&stat, identity_at(fs(), source_parent.as_fd()));
    if cancel.is_cancelled() {
        return Err(crate::error::cancelled());
    }
    match placement::place(
        fs(),
        from,
        Entry::new(&*target_parent, target_name),
        Some(stat.identity),
    ) {
        Ok(_) => {
            return Ok(PutBack {
                trashed,
                kept: Vec::new(),
            });
        }
        Err(PlaceError::Unsupported) => {}
        Err(PlaceError::CrossDevice) => {
            return Err(Error::new(
                crate::error::ErrorKind::NotSupported,
                &rust_i18n::t!("Could not restore %{name} across volumes", name = display),
            ));
        }
        Err(PlaceError::Exists) => return Err(exists()),
        Err(error) => {
            return Err(failed(
                &crate::adapters::local_operations::placement_reason(error),
            ));
        }
    }

    // No rename can place it: copied, then what was copied deleted from the Trash once proven.
    let barrier = Barrier::open(fs(), target_folder)?;
    let root_folder = manifest::Folder::root();
    let source = Source {
        parent: source_parent,
        name: source_name,
        recorded_in: Some(root_folder.clone()),
    };
    let options = Options {
        manifest: true,
        durable: true,
        ..with_names(NameMode::Keep)
    };
    let progress = Progress::new();
    let copy = Copy::new(
        fs(),
        source.name.clone(),
        cancel.0.clone(),
        progress.0.clone(),
        true,
        options,
        Deadline::default(),
    );
    let target_name = target_name.to_os_string();
    // Written at its name, created exclusively, and only while the item is the one read above.
    let (published, ownership) = copy_new(
        &copy,
        &source,
        (&target_parent, &target_name),
        super::NewItem::InPlace(stat.identity),
        None,
    )
    .map_err(|failure| {
        let error = failure.into_error(&source.name);
        match error.kind() {
            crate::error::ErrorKind::Cancelled => error,
            crate::error::ErrorKind::Exists => exists(),
            _ => failed(error.message()),
        }
    })?;
    barrier.reach(fs(), false).map_err(|error| {
        crate::error::io_error(rust_i18n::t!(
            "The device could not finish writing: %{error}. Earlier writes may still be pending; wait for safe eject before unplugging.",
            error = crate::adapters::local_operations::native_fs::os_error_text(error)
        ))
    })?;
    let mut sources = manifest::of(source.parent.clone(), root_folder);
    sources.copied_to(Published::proven(target_parent, published, ownership));
    // The copy is complete, so a cancel no longer stops this restore.
    let (kept, deleted) = manifest::delete(
        fs(),
        sources,
        Arc::new(super::Unlinks::default()),
        Arc::default(),
    );
    deleted.map_err(|error| {
        crate::error::io_error(rust_i18n::t!(
            "The item was restored, but its trash copy could not be removed: %{error}",
            error = error.message()
        ))
    })?;
    let folder = item.parent().unwrap_or(Path::new("/"));
    Ok(PutBack {
        trashed,
        kept: kept
            .into_iter()
            .map(|(path, cause)| Kept {
                path: if path.is_absolute() {
                    path
                } else {
                    folder.join(path)
                },
                cause,
            })
            .collect(),
    })
}

/// Why a move failed, as legacy worded it: a move says what it moved, a rename gives the reason
/// alone. The kind stays the placement's.
fn move_failure(
    source: &Path,
    renaming: bool,
    from: crate::adapters::local_operations::fs_ops::Entry<'_>,
    error: crate::adapters::local_operations::placement::PlaceError,
) -> Error {
    use crate::adapters::local_operations::{
        holders::refused_rename,
        placement::{self, PlaceError},
        placement_error_kind, placement_reason,
    };
    let name = source.file_name().unwrap_or_default().to_string_lossy();
    let (kind, reason) = match error {
        PlaceError::Io(errno) => match refused_rename(fs(), from, errno) {
            Some(why) => (crate::error::ErrorKind::Busy, why),
            None => (placement_error_kind(error), placement_reason(error)),
        },
        PlaceError::IntoItself => return placement::place_error(error),
        PlaceError::Changed => {
            return Error::new(
                crate::error::ErrorKind::Failed,
                &rust_i18n::t!(
                    "%{name} changed while the operation was in progress",
                    name = name
                ),
            );
        }
        error => (placement_error_kind(error), placement_reason(error)),
    };
    if renaming {
        return Error::new(kind, &reason);
    }
    Error::new(
        kind,
        &rust_i18n::t!(
            "Could not move %{name}: %{error}",
            name = name,
            error = crate::services::error_detail(reason)
        ),
    )
}

/// Whether a rename could take an item from folder `one` to folder `other`: both on one mount,
/// as the engine's filesystem sees them. Linux refuses a rename between two mounts even of one
/// filesystem.
pub(crate) fn one_mount(one: &Path, other: &Path) -> bool {
    use crate::adapters::local_operations::native_fs::open_local_parent_directory;
    let key = |folder: &Path| {
        open_local_parent_directory(folder)
            .ok()
            .map(|dir| fs().mount_key(dir.as_fd()))
    };
    key(one).is_some_and(|one| key(other) == Some(one))
}

/// Merges the folder `source` into the existing folder `name` in `into` with the engine's merge:
/// each entry is staged and appears whole, hard links stay links within what is copied together,
/// a folder of the target swapped for a link is not followed, and a name that is taken keeps both.
/// What it placed before a failure stays, and the error says what failed.
pub(crate) fn merge_by_copying(
    source: &Path,
    (into, name): (&Path, &OsStr),
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> (Result<(), Error>, Changes) {
    let (parent, item) = match split(source) {
        Ok(split) => split,
        Err(error) => return (Err(error), Changes::default()),
    };
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
        with_names(names),
        Deadline::default(),
    );
    let merged = merge(&copy, &source, &into.join(name), None, None)
        .map(drop)
        .map_err(|failure| failure.into_error(&source.name));
    (merged, Changes::of(&copy, name))
}

/// Merges the folder `source` into the folder `name` in `into` on another drive by moving: the
/// engine's merge copies it in, keeping both where a name is taken, and once the copy is on the
/// drive the source entries it copied are deleted while they are still what it read. Anything
/// changed, added or left out meanwhile stays, and is reported in [`Changes::kept`].
pub(crate) fn merge_by_moving(
    source: &Path,
    (into, name): (&Path, &OsStr),
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Moved, Error> {
    use super::{Barrier, manifest};
    let folder = source.parent().unwrap_or(Path::new("/")).to_path_buf();
    let barrier = Barrier::open(fs(), into)?;
    let root = manifest::Folder::root();
    let (source_parent, source_name) = split(source)?;
    let source = Source {
        parent: source_parent,
        name: source_name,
        recorded_in: Some(root.clone()),
    };
    let options = Options {
        manifest: true,
        durable: true,
        ..with_names(names)
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
    let (nested, published) = merge(&copy, &source, &into.join(name), None, None)
        .map_err(|failure| failure.into_error(&source.name))?;
    for barrier in std::iter::once(&barrier).chain(&nested) {
        barrier.reach(fs(), false).map_err(|error| {
            crate::error::io_error(rust_i18n::t!(
                "The device could not finish writing: %{error}. Earlier writes may still be pending; wait for safe eject before unplugging.",
                error = crate::adapters::local_operations::native_fs::os_error_text(error)
            ))
        })?;
    }
    let mut sources = manifest::of(source.parent.clone(), root);
    for copied in std::mem::take(&mut *lock(&copy.copies)) {
        sources.copied_to(copied);
    }
    if let Some(published) = published {
        sources.copied_to(published);
    }
    let (kept, deleted) = manifest::delete(
        fs(),
        sources,
        Arc::new(super::Unlinks::default()),
        cancel.0.clone(),
    );
    let mut changes = Changes::of(&copy, name);
    changes.kept = kept
        .iter()
        .map(|(path, cause)| Kept {
            path: if path.is_absolute() {
                path.clone()
            } else {
                folder.join(path)
            },
            cause: cause.clone(),
        })
        .collect();
    Ok(Moved {
        name: name.to_os_string(),
        copied: true,
        kept,
        changes,
        progress: progress.snapshot(),
        stopped: deleted.err(),
    })
}

/// A sync point for everything a batch writes into one folder's drive. Opened before the batch
/// writes; reaching it returns once the drive says all of it is stored, so a removable drive can
/// then be unplugged.
pub struct Settle(super::Barrier);

impl Settle {
    pub fn open(into: &Path) -> Result<Self, Error> {
        super::Barrier::open(fs(), into).map(Self)
    }

    pub fn reach(&self) -> Result<(), Error> {
        self.0.reach(fs(), true).map_err(crate::error::os_io_error)
    }
}

pub use crate::trash::Trashed;

/// Deletes `item` for good, a folder with everything in it, without the Trash. The walk holds
/// each folder open by identity, so a folder renamed or swapped for a link meanwhile is never
/// followed out of the tree; links are removed, not followed; another program's leftover (a file
/// still open on NFS or SMB) is waited for, within the mount's wait, and never removed. Folders are
/// emptied by parallel workers, one on a spinning disk. A cancel stops at the next entry, keeping
/// what is left whole.
pub fn delete(item: &Path, cancel: &Cancel) -> Result<(), Error> {
    delete_within(
        item,
        cancel,
        &crate::adapters::local_operations::leftovers::Deadline::default(),
    )
}

/// [`delete`], waiting for files still open elsewhere within `deadline`, which a batch shares so
/// that one wait covers every item of it.
pub(crate) fn delete_within(
    item: &Path,
    cancel: &Cancel,
    deadline: &crate::adapters::local_operations::leftovers::Deadline,
) -> Result<(), Error> {
    use crate::adapters::local_operations::deletion::{self, PinnedEntry};
    let (Some(parent), Some(name)) = (
        item.parent()
            .filter(|parent| !parent.as_os_str().is_empty()),
        item.file_name(),
    ) else {
        return Err(crate::error::translated_io_error(
            "Cannot permanently delete the filesystem root",
        ));
    };
    let parent = crate::adapters::local_operations::native_fs::open_local_parent_directory(parent)
        .map_err(crate::error::io_error)?;
    let roots = vec![PinnedEntry {
        parent: Arc::new(parent),
        name: name.to_os_string(),
        identity: None,
    }];
    let workers = deletion::local_delete_worker_count(&roots);
    match deletion::parallel_delete_local_blocking_with_workers(
        roots,
        cancel.0.clone(),
        workers,
        deadline,
    ) {
        Ok(()) => Ok(()),
        Err(error) if cancel.is_cancelled() || error == "Delete cancelled" => {
            Err(crate::error::cancelled())
        }
        Err(error) => Err(Error::new(crate::error::ErrorKind::Failed, &error)),
    }
}

/// Moves `item` to the Trash on its own drive; see [`crate::trash::put`].
pub fn trash(item: &Path) -> Result<Trashed, Error> {
    crate::trash::put(item)
}

/// Moves `source` into `into`, replacing the item that holds its name; the original goes to the
/// Trash, recorded under its own path.
///
/// On one filesystem the moved item and the original trade names in one step
/// (`RENAME_EXCHANGE`), and the original then goes to the Trash; if the Trash refuses, they
/// trade back. Elsewhere the original goes to the Trash first and the item is then moved in; if
/// that move fails, the original is restored, and if even that fails, the error says it is in
/// the Trash. An item moved onto itself stays as it is.
pub fn move_replacing(
    source: &Path,
    into: &Path,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Replaced, Error> {
    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?;
    move_replacing_as(source, (into, name), names, cancel, progress)
}

/// Moves `source` into `into` over the item called `name`, as [`move_replacing`] does over the
/// one of its own name.
pub fn move_replacing_as(
    source: &Path,
    (into, name): (&Path, &OsStr),
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Replaced, Error> {
    use crate::adapters::local_operations::placement;
    plain_name(name)?;
    let name = name.to_os_string();
    let target = into.join(&name);
    if same_item(source, &target) {
        return Ok(Replaced {
            name,
            original: None,
            original_deleted: false,
            changes: Changes::default(),
            progress: progress.snapshot(),
        });
    }
    if lands_within(source, into) {
        return Err(placement::place_error(placement::PlaceError::IntoItself));
    }
    if std::fs::symlink_metadata(&target).is_err() {
        let moved = move_as(source, (into, &name), names, cancel, progress)?.finished()?;
        return Ok(Replaced {
            name: moved.name,
            original: None,
            original_deleted: false,
            changes: moved.changes,
            progress: moved.progress,
        });
    }
    let (source_parent, source_name) = split(source)?;
    let (target_parent, target_name) = split(&target)?;
    if one_mount(source.parent().unwrap_or(source), into) {
        return replace_by_moving(
            (&source_parent, &source_name),
            (&target_parent, into, &target_name),
            progress,
        );
    }
    // Across mounts, as legacy: the item is copied into a stage beside the target, the stage takes
    // the target's place within its mount, and only then are the sources deleted, while they are
    // still what the copy read. Until that last step the source is whole.
    copy_then_replace(
        (source, &source_parent, &source_name),
        (into, &target_parent, &target_name),
        names,
        cancel,
        progress,
    )
}

/// The replacing move across mounts; see [`move_replacing_as`].
fn copy_then_replace(
    (source_path, source_parent, source_name): (&Path, &Arc<std::os::fd::OwnedFd>, &OsString),
    (into, target_parent, target_name): (&Path, &Arc<std::os::fd::OwnedFd>, &OsString),
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Replaced, Error> {
    use super::{Barrier, Published, manifest};
    let barrier = Barrier::open(fs(), into)?;
    let root = manifest::Folder::root();
    let source = Source {
        parent: source_parent.clone(),
        name: source_name.clone(),
        recorded_in: Some(root.clone()),
    };
    let options = Options {
        manifest: true,
        durable: true,
        ..with_names(names)
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
    let stage = crate::model::hidden_names::HiddenName::Copy.fresh();
    let (stage, ownership) = copy_new(
        &copy,
        &source,
        (target_parent, &stage),
        super::NewItem::Published(Taken::Fail),
        None,
    )
    .map_err(|failure| failure.into_error(&source.name))?;
    let staged = fs()
        .stat(
            crate::adapters::local_operations::fs_ops::Entry::new(&**target_parent, &stage),
            false,
        )
        .map_err(crate::error::os_io_error)?
        .identity;
    let discard_stage = || discard(target_parent, &stage, staged);
    if cancel.is_cancelled() {
        discard_stage();
        return Err(crate::error::cancelled());
    }
    let replaced = match replace_by_moving(
        (target_parent, &stage),
        (target_parent, into, target_name),
        progress,
    ) {
        Ok(replaced) => replaced,
        Err(error) => {
            discard_stage();
            return Err(error);
        }
    };
    barrier.reach(fs(), false).map_err(|error| {
        crate::error::io_error(rust_i18n::t!(
            "The device could not finish writing: %{error}. Earlier writes may still be pending; wait for safe eject before unplugging.",
            error = crate::adapters::local_operations::native_fs::os_error_text(error)
        ))
    })?;
    let mut sources = manifest::of(source.parent.clone(), root);
    sources.copied_to(Published::proven(
        target_parent.clone(),
        target_name.clone(),
        ownership,
    ));
    let (kept, deleted) = manifest::delete(
        fs(),
        sources,
        Arc::new(super::Unlinks::default()),
        cancel.0.clone(),
    );
    deleted?;
    let mut changes = Changes::of(&copy, target_name);
    let folder = source_path.parent().unwrap_or(Path::new("/"));
    changes.kept = kept
        .into_iter()
        .map(|(path, cause)| Kept {
            path: if path.is_absolute() {
                path
            } else {
                folder.join(path)
            },
            cause,
        })
        .collect();
    // What the replacing step kept (an original it could not delete) is reported with the rest.
    changes.kept.extend(replaced.changes.kept.iter().cloned());
    Ok(Replaced {
        changes,
        ..replaced
    })
}

/// The replacing move within one mount: the item itself is the replacement, so nothing is
/// copied. Where the drive can trade two names it does, and the original then goes from where the
/// item was; elsewhere the original is set aside and the item takes its name. Either way the
/// original goes to the Trash, or is deleted where the drive has none, and every step is undone
/// if a later one fails.
fn replace_by_moving(
    (source_dir, source_name): (&Arc<std::os::fd::OwnedFd>, &OsString),
    (parent, into, target): (&Arc<std::os::fd::OwnedFd>, &Path, &OsString),
    progress: &Progress,
) -> Result<Replaced, Error> {
    use crate::adapters::local_operations::{
        fs_ops::Entry,
        placement::{self, PlaceError},
    };
    let stat = |dir: &Arc<std::os::fd::OwnedFd>, name: &OsStr| {
        fs().stat(Entry::new(&**dir, name), false)
            .map_err(crate::error::os_io_error)
    };
    let moving = stat(source_dir, source_name)?.identity;
    let found = stat(parent, target)?;
    let original = found.identity;
    let barrier = super::Barrier::open(fs(), into)?;
    let target_path = into.join(target);
    let source_path = crate::adapters::local_operations::native_fs::opened_path(source_dir.as_fd())
        .map_err(crate::error::io_error)?
        .join(source_name);
    let gone = if placement::exchange_allowed(fs(), parent.as_fd())
        && fs().can_trash(&target_path)
        && placement::exchange(
            fs(),
            Entry::new(&**source_dir, source_name),
            Entry::new(&**parent, target),
        )
        .is_ok_and(|swapped| swapped)
    {
        // Undoes the swap. Where the name was emptied meanwhile there is nothing to trade with,
        // so the original is moved straight back to it; if even that fails, the error says where
        // it is.
        let swap_back = |error: Error| -> Error {
            let traded = placement::exchange(
                fs(),
                Entry::new(&**source_dir, source_name),
                Entry::new(&**parent, target),
            )
            .is_ok_and(|swapped| swapped);
            let restored = traded
                || placement::place(
                    fs(),
                    Entry::new(&**source_dir, source_name),
                    Entry::new(&**parent, target),
                    Some(original),
                )
                .is_ok();
            if restored {
                error
            } else {
                kept_at(&source_path, &error)
            }
        };
        // The original now sits where the item was, and the item at the name: prove both are
        // the ones confirmed, so a source swapped in meanwhile is never what replaces it.
        if !stat(source_dir, source_name).is_ok_and(|stat| stat.identity == original)
            || !stat(parent, target).is_ok_and(|stat| stat.identity == moving)
        {
            return Err(swap_back(placement::place_error(PlaceError::Changed)));
        }
        let folder = source_path.parent().unwrap_or(Path::new("/"));
        match dispose(
            (source_dir, folder),
            (source_name, original),
            &target_path,
            &barrier,
        ) {
            Ok(gone) => gone,
            Err(error) => return Err(swap_back(error)),
        }
    } else {
        let is_folder = found.kind == rustix::fs::FileType::Directory;
        replace_by_setting_aside(
            (parent, into),
            (source_dir, source_name, moving),
            (target, original, found.file_attributes()),
            (is_folder, true),
            (&|| {}, &barrier),
        )?
    };
    let mut changes = Changes::default();
    let (original, original_deleted) = match gone {
        Gone::Trashed(trashed) => (Some(trashed), false),
        Gone::Deleted => (None, true),
        Gone::Kept(error) => return Err(error),
        Gone::NotRemoved(path, why) => {
            changes.kept.push(Kept {
                path,
                cause: crate::services::KeptCause::OriginalNotRemoved(why),
            });
            (None, false)
        }
    };
    Ok(Replaced {
        name: target.clone(),
        original,
        original_deleted,
        changes,
        progress: progress.snapshot(),
    })
}

/// A finished replace.
#[derive(Debug)]
pub struct Replaced {
    pub name: OsString,
    /// Where the replaced original went, or None when nothing held the name or it was deleted.
    pub original: Option<Trashed>,
    /// The drive has no Trash, so the original was deleted once its replacement was on the
    /// drive. Undo cannot bring it back, so a replace like this is not filed.
    pub original_deleted: bool,
    /// What the copy wrote otherwise than asked; see [`copy`].
    pub changes: Changes,
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
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Replaced, Error> {
    let name = source
        .file_name()
        .ok_or_else(|| crate::error::translated_io_error("The item has no name"))?;
    replace_as(source, (into, name), names, cancel, progress)
}

/// Copies `source` into `into` over the item called `name`, as [`replace`] does over the one of
/// its own name: the name a conflict prompt showed, say.
pub fn replace_as(
    source: &Path,
    (into, name): (&Path, &OsStr),
    names: NameMode,
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

    plain_name(name)?;
    let name = name.to_os_string();
    if lands_within(source, into) {
        return Err(placement::place_error(PlaceError::IntoItself));
    }
    if same_item(source, &into.join(&name)) {
        return Ok(Replaced {
            name,
            original: None,
            original_deleted: false,
            changes: Changes::default(),
            progress: progress.snapshot(),
        });
    }
    let (parent, target) = split(&into.join(&name))?;
    let found = match fs().stat(Entry::new(&*parent, &target), false) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => {
            let copied = copy_as(
                source,
                (into, &name),
                Conflict::Fail,
                names,
                cancel,
                progress,
            )?;
            return Ok(Replaced {
                name: copied.name,
                original: None,
                original_deleted: false,
                changes: copied.changes,
                progress: copied.progress,
            });
        }
        Err(error) => return Err(os_io_error(error)),
    };
    let original = found.identity;

    // Refused before anything is copied where neither strategy is safe on this drive.
    let is_folder = found.kind == rustix::fs::FileType::Directory;
    if !placement::exchange_allowed(fs(), parent.as_fd())
        && placement::can_set_aside(fs(), parent.as_fd(), is_folder)
            .is_ok_and(|feasible| feasible == placement::Feasibility::No)
    {
        return Err(Error::new(
            crate::error::ErrorKind::NotSupported,
            &rust_i18n::t!(
                "“%{name}” can’t be replaced safely at this location. Delete or rename it first, then try again.",
                name = name.to_string_lossy()
            ),
        ));
    }
    // Opened before the copy is written, so reaching it covers the replacement; it is reached
    // only where an original has to be deleted for want of a Trash.
    let barrier = super::Barrier::open(fs(), into)?;
    let stage = HiddenName::Copy.fresh();
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
        with_names(names),
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
    // A cancel that came as the copy finished is honoured before anything is replaced.
    if cancel.is_cancelled() {
        discard_stage();
        return Err(crate::error::cancelled());
    }
    let target_path = into.join(&target);

    // Where the mount renumbers an evicted entry (FAT, some FUSE and SMB), it is still the original
    // while the mount cannot tell it apart; one it can tell apart has changed.
    let mount = super::Mount::new(fs(), parent.as_fd());
    let still_original = fs()
        .stat(Entry::new(&*parent, &target), false)
        .is_ok_and(|stat| mount.is_still(parent.as_fd(), &found, &stat));
    if !still_original {
        discard_stage();
        return Err(placement::place_error(PlaceError::Changed));
    }
    // Without a Trash the original is swapped out into an originals folder under its own name, as
    // legacy did, and deleted there once the copy is on the drive; if it cannot be, it waits there.
    let trashes = fs().can_trash(&target_path);
    let swapped_out = if placement::exchange_allowed(fs(), parent.as_fd()) && !trashes {
        match placement::exchange_through(
            fs(),
            Entry::new(&*parent, &stage),
            Entry::new(&*parent, &target),
            staged,
            None,
        ) {
            Ok(placement::Exchanged::Swapped(originals)) => {
                let waiting = into.join(&originals.name);
                let gone = dispose(
                    (&originals.handle(), &waiting),
                    (&target, original),
                    &target_path,
                    &barrier,
                )
                .map_err(|error| kept_at(&waiting.join(&target), &error))?;
                Some(clear_originals(
                    (&parent, into),
                    (&originals.name, &target),
                    gone,
                ))
            }
            Ok(placement::Exchanged::Refused) => None,
            Err(error) => {
                discard_stage();
                return Err(placement::place_error(error));
            }
        }
    } else {
        None
    };
    let exchanged =
        if swapped_out.is_none() && trashes && placement::exchange_allowed(fs(), parent.as_fd()) {
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
    let trashed = if let Some(gone) = swapped_out {
        gone
    } else if exchanged {
        // The original now sits at the stage name. Prove it is still the item that was confirmed.
        let swapped_is_original = fs()
            .stat(Entry::new(&*parent, &stage), false)
            .is_ok_and(|stat| mount.is_still(parent.as_fd(), &found, &stat));
        let trashed = if swapped_is_original {
            dispose((&parent, into), (&stage, original), &target_path, &barrier)
        } else {
            Err(placement::place_error(PlaceError::Changed))
        };
        match trashed {
            Ok(gone) => gone,
            Err(error) => {
                let swapped_back = placement::exchange(
                    fs(),
                    Entry::new(&*parent, &stage),
                    Entry::new(&*parent, &target),
                )
                .is_ok_and(|swapped| swapped);
                if swapped_back {
                    // The original is back at its name; the copy returned to the stage.
                    discard_stage();
                    return Err(error);
                }
                // Where the name was emptied meanwhile, the original goes straight back to it.
                let restored = placement::place(
                    fs(),
                    Entry::new(&*parent, &stage),
                    Entry::new(&*parent, &target),
                    Some(original),
                )
                .is_ok();
                return Err(if restored {
                    error
                } else {
                    kept_at(&into.join(&stage), &error)
                });
            }
        }
    } else {
        replace_by_setting_aside(
            (&parent, into),
            (&parent, &stage, staged),
            (&target, original, found.file_attributes()),
            (is_folder, false),
            (&discard_stage, &barrier),
        )?
    };
    let mut kept = Vec::new();
    let (original, original_deleted) = match trashed {
        Gone::Trashed(trashed) => (Some(trashed), false),
        Gone::Deleted => (None, true),
        Gone::Kept(error) => return Err(error),
        Gone::NotRemoved(path, why) => {
            kept.push(Kept {
                path,
                cause: crate::services::KeptCause::OriginalNotRemoved(why),
            });
            (None, false)
        }
    };
    let mut changes = Changes::of(&copy, &target);
    changes.kept.extend(kept);
    Ok(Replaced {
        changes,
        name: target,
        original,
        original_deleted,
        progress: progress.snapshot(),
    })
}

/// The replace for a drive without `RENAME_EXCHANGE`: set the original aside, publish the
/// stage at its name, trash the original. Undoes in reverse on any failure.
fn replace_by_setting_aside(
    (parent, into): (&Arc<std::os::fd::OwnedFd>, &Path),
    (stage_dir, stage, staged): (
        &Arc<std::os::fd::OwnedFd>,
        &OsString,
        crate::adapters::local_operations::native_fs::LocalFileIdentity,
    ),
    (target, original, attributes): (
        &OsString,
        crate::adapters::local_operations::native_fs::LocalFileIdentity,
        Option<crate::services::Attributes>,
    ),
    (is_folder, moving): (bool, bool),
    (discard_stage, barrier): (&dyn Fn(), &super::Barrier),
) -> Result<Gone, Error> {
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
    // With its size and times, so a mount that renumbers an evicted entry still knows it.
    let aside =
        match placement::set_aside(fs(), Entry::new(&**parent, target), original, attributes) {
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

    // A moved item is anyone's to swap meanwhile, so it is placed by its identity; a stage is
    // this operation's own.
    let published = if moving {
        placement::place(
            fs(),
            Entry::new(&**stage_dir, stage),
            Entry::new(&**parent, target),
            Some(staged),
        )
    } else {
        placement::publish_staged(
            fs(),
            Entry::new(&**stage_dir, stage),
            Entry::new(&**parent, target),
            staged,
        )
    };
    if let Err(error) = published {
        let error = placement::place_error(error);
        if !put_back() {
            // The original stays aside, said where; the copy in the stage is only a copy.
            discard_stage();
            return Err(kept_at(&aside_path, &error));
        }
        discard_stage();
        return Err(error);
    }
    let originals = crate::adapters::local_operations::native_fs::open_local_parent_directory(
        &into.join(&aside.originals.name),
    )
    .map(Arc::new)
    .map_err(crate::error::io_error);
    let gone = originals.and_then(|originals| {
        dispose(
            (&originals, &into.join(&aside.originals.name)),
            (&aside.name, original),
            &into.join(target),
            barrier,
        )
    });
    match gone {
        Ok(gone) => Ok(clear_originals(
            (parent, into),
            (&aside.originals.name, &aside.name),
            gone,
        )),
        Err(error) => {
            // The copy goes back to its stage, then the original back to its name.
            let copy_back = placement::place(
                fs(),
                Entry::new(&**parent, target),
                Entry::new(&**stage_dir, stage),
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
    if let Err(error) = remove_pinned(parent, name, identity) {
        tracing::warn!(?name, %error, "could not remove a replace stage");
    }
}

/// Deletes `name` in `parent`, a whole tree, only while it is still the item `identity` names.
fn remove_pinned(
    parent: &Arc<std::os::fd::OwnedFd>,
    name: &OsStr,
    identity: crate::adapters::local_operations::native_fs::LocalFileIdentity,
) -> Result<(), Error> {
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
    parallel_delete_local_blocking_with_workers(
        roots,
        Arc::new(AtomicBool::new(false)),
        workers,
        &Deadline::default(),
    )
    .map_err(crate::error::io_error)
}

/// Removes the originals folder an original waited in once it is gone. On NFS or SMB a file still
/// open elsewhere lingers under a hidden name once deleted, so the folder waits for it to close,
/// within the mount's wait; if it stays open, the original is reported kept, with why.
fn clear_originals(
    (parent, into): (&Arc<std::os::fd::OwnedFd>, &Path),
    (originals, name): (&OsStr, &OsStr),
    gone: Gone,
) -> Gone {
    use crate::adapters::local_operations::{fs_ops::Entry, leftovers};
    let folder = Entry::new(&**parent, originals);
    match leftovers::remove_folder(fs(), folder, &Deadline::default(), &|| false, &|| true) {
        Ok(()) => gone,
        Err(_) => match (gone, leftovers::still_open(fs(), folder)) {
            (Gone::Deleted, Some(why)) => Gone::NotRemoved(into.join(originals).join(name), why),
            (gone, _) => gone,
        },
    }
}

/// What became of an original a replace took the place of.
enum Gone {
    Trashed(Trashed),
    Deleted,
    /// Its drive has no Trash and could not confirm the replacement, so the original was kept
    /// where it waited; the error says where. The replacement stays: undoing it would only write
    /// more to a drive that just failed.
    Kept(Error),
    /// Its drive has no Trash and deleting it failed (it is still open in a program, say): the
    /// replacement stands, and the original stays where it waited, with why.
    NotRemoved(PathBuf, String),
}

/// Sends the original at `name` in `parent` to the Trash, recorded under `recorded` (the path it
/// had). Where its drive has no Trash it is deleted, as legacy did, once `barrier` says its
/// replacement is on the drive, and only while it is still the original.
fn dispose(
    (parent, into): (&Arc<std::os::fd::OwnedFd>, &Path),
    (name, identity): (
        &OsStr,
        crate::adapters::local_operations::native_fs::LocalFileIdentity,
    ),
    recorded: &Path,
    barrier: &super::Barrier,
) -> Result<Gone, Error> {
    match crate::trash::put_recording(&into.join(name), Some(recorded)) {
        Ok(trashed) => Ok(Gone::Trashed(trashed)),
        Err(error) if error.kind() == crate::error::ErrorKind::NotSupported => {
            if let Err(error) = barrier.reach(fs(), false) {
                let error = crate::error::io_error(rust_i18n::t!(
                    "The device could not finish writing: %{error}. Earlier writes may still be pending; wait for safe eject before unplugging.",
                    error = crate::adapters::local_operations::native_fs::os_error_text(error)
                ));
                return Ok(Gone::Kept(kept_at(&into.join(name), &error)));
            }
            match remove_pinned(parent, name, identity) {
                Ok(()) => Ok(Gone::Deleted),
                Err(error) => {
                    use crate::adapters::local_operations::{fs_ops::Entry, leftovers};
                    let why = leftovers::still_open(fs(), Entry::new(&**parent, name))
                        .unwrap_or_else(|| error.message().to_owned());
                    Ok(Gone::NotRemoved(into.join(name), why))
                }
            }
        }
        Err(error) => Err(error),
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

            let replaced = replace(
                &source,
                &dest,
                NameMode::Keep,
                &Cancel::new(),
                &Progress::new(),
            )
            .expect("replace");

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
                fail: vec![(Op::Rename, ".asc-files-copy-*", 0, rustix::io::Errno::IO)],
                ..Quirks::vfat()
            },
        );
        let old_inode = fs::metadata(dest.join("item")).unwrap().ino();

        let result = replace(
            &source,
            &dest,
            NameMode::Keep,
            &Cancel::new(),
            &Progress::new(),
        );

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

        let result = replace(
            &source,
            &dest,
            NameMode::Keep,
            &Cancel::new(),
            &Progress::new(),
        );
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

    /// A folder with names no Windows-format drive can store, and one it can.
    fn awkward_tree(home: &Path) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir_in(home).unwrap();
        let (source, dest) = (root.path().join("source"), root.path().join("dest"));
        fs::create_dir_all(source.join("trip")).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(source.join("trip/a:b.txt"), b"colon").unwrap();
        fs::write(source.join("trip/why?.txt"), b"question").unwrap();
        fs::write(source.join("trip/plain.txt"), b"plain").unwrap();
        (root, source.join("trip"), dest)
    }

    fn windows_format_drives() -> [(&'static str, Quirks); 4] {
        [
            ("vfat", Quirks::vfat()),
            ("exfat", Quirks::exfat()),
            ("ntfs3 windows_names", Quirks::ntfs3_windows_names()),
            ("ntfs-3g", Quirks::ntfs_3g()),
        ]
    }

    #[test]
    fn names_a_windows_format_drive_cannot_store_are_fixed_and_reported_when_asked() {
        let home = test_home();
        for (drive, quirks) in windows_format_drives() {
            let (_root, source, dest) = awkward_tree(&home);
            let _drive = FsQuirks::install(&dest, quirks);

            let copied = copy(
                &source,
                &dest,
                Conflict::Fail,
                NameMode::Fix,
                &Cancel::new(),
                &Progress::new(),
            )
            .unwrap_or_else(|error| panic!("{drive}: {error:?}"));

            assert_eq!(copied.name, "trip", "{drive}");
            let mut renamed = copied.changes.renamed.clone();
            renamed.sort_by(|a, b| a.requested.cmp(&b.requested));
            let requested: Vec<_> = renamed.iter().map(|r| r.requested.clone()).collect();
            assert_eq!(
                requested,
                [
                    PathBuf::from("trip/a:b.txt"),
                    PathBuf::from("trip/why?.txt")
                ],
                "{drive}: {renamed:?}"
            );
            for (renamed, content) in renamed.iter().zip([&b"colon"[..], b"question"]) {
                assert_eq!(renamed.cause, RenameCause::Invalid, "{drive}");
                assert_eq!(
                    renamed.saved_as.parent(),
                    Some(Path::new("trip")),
                    "{drive}"
                );
                let saved = renamed.saved_as.file_name().unwrap().as_encoded_bytes();
                assert!(
                    !saved.iter().any(|byte| b"\\:*?\"<>|".contains(byte)),
                    "{drive}: {renamed:?}"
                );
                assert_eq!(
                    fs::read(dest.join(&renamed.saved_as)).unwrap(),
                    content,
                    "{drive}"
                );
            }
            assert_eq!(
                fs::read(dest.join("trip/plain.txt")).unwrap(),
                b"plain",
                "{drive}"
            );
            assert!(copied.changes.skipped.is_empty(), "{drive}");
        }
    }

    #[test]
    fn an_item_whose_own_name_a_drive_refuses_fails_when_its_name_must_be_kept() {
        let home = test_home();
        for (drive, quirks) in windows_format_drives() {
            let root = tempfile::tempdir_in(&home).unwrap();
            let (source, dest) = (root.path().join("source"), root.path().join("dest"));
            fs::create_dir_all(source.join("a:b")).unwrap();
            fs::write(source.join("a:b/inside.txt"), b"x").unwrap();
            fs::create_dir_all(&dest).unwrap();
            let _drive = FsQuirks::install(&dest, quirks);

            let result = copy(
                &source.join("a:b"),
                &dest,
                Conflict::Fail,
                NameMode::Keep,
                &Cancel::new(),
                &Progress::new(),
            );

            assert!(result.is_err(), "{drive}: {result:?}");
            assert!(names(&dest).is_empty(), "{drive}: nothing is left behind");
        }
    }

    #[test]
    fn contents_are_fixed_for_the_drive_even_when_the_items_own_name_is_kept() {
        let home = test_home();
        let (_root, source, dest) = awkward_tree(&home);
        let _fat = FsQuirks::install(&dest, Quirks::vfat());

        let copied = copy(
            &source,
            &dest,
            Conflict::Fail,
            NameMode::Keep,
            &Cancel::new(),
            &Progress::new(),
        )
        .expect("copy");

        assert_eq!(copied.name, "trip");
        assert_eq!(copied.changes.renamed.len(), 2, "{:?}", copied.changes);
        assert_eq!(names(&dest.join("trip")).len(), 3);
    }

    #[test]
    fn an_item_whose_own_name_is_fixed_is_published_and_reported_under_the_new_name() {
        let home = test_home();
        let root = tempfile::tempdir_in(&home).unwrap();
        let (source, dest) = (root.path().join("source"), root.path().join("dest"));
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(source.join("x|y.txt"), b"pipe").unwrap();
        let _fat = FsQuirks::install(&dest, Quirks::vfat());

        let copied = copy(
            &source.join("x|y.txt"),
            &dest,
            Conflict::Fail,
            NameMode::Fix,
            &Cancel::new(),
            &Progress::new(),
        )
        .expect("copy");

        assert_ne!(copied.name, "x|y.txt");
        assert_eq!(names(&dest), [copied.name.to_string_lossy()]);
        assert_eq!(fs::read(dest.join(&copied.name)).unwrap(), b"pipe");
        assert_eq!(
            copied.changes.renamed,
            [Renamed {
                requested: PathBuf::from("x|y.txt"),
                saved_as: PathBuf::from(&copied.name),
                cause: RenameCause::Invalid,
            }]
        );
    }

    #[test]
    fn a_move_onto_a_fat_drive_fixes_names_and_still_deletes_its_source() {
        let home = test_home();
        let (_root, source, dest) = awkward_tree(&home);
        let _fat = FsQuirks::install(&dest, Quirks::vfat());

        let moved = move_item(
            &source,
            &dest,
            NameMode::Fix,
            &Cancel::new(),
            &Progress::new(),
        )
        .expect("move");

        assert!(moved.copied, "a FAT drive is another filesystem");
        assert_eq!(moved.changes.renamed.len(), 2, "{:?}", moved.changes);
        assert!(moved.kept.is_empty(), "{:?}", moved.kept);
        assert!(
            !source.exists(),
            "the source is deleted once its copy is on the drive"
        );
        assert_eq!(names(&dest.join("trip")).len(), 3);
    }
}
