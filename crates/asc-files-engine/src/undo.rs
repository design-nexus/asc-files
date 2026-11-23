// SPDX-License-Identifier: MIT
//! Undo and redo: a journal of what each operation did, shared by every Files process in the
//! session and kept across a backend restart.
//!
//! The design is Flea's (`src/backend/undo.rs`, `redo.rs`, `undoshare.rs`; MIT, see
//! `docs/PROVENANCE.md`): an operation is one entry of steps, reversed newest first; each step
//! names the item it left by identity, so undo never acts on something put at that path since;
//! redo repeats an operation from its recorded inputs and never replaces what holds a name; the
//! history lives in one locked file under `$XDG_RUNTIME_DIR`, 50 entries deep. A step that cannot
//! be reversed is reported and dropped, and the rest still run.
//!
//! Undoing a copy moves the copy to the Trash, as legacy did. Where its drive has no Trash, an
//! unchanged file is deleted, and anything else is left in place and reported.

use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{DirBuilderExt, MetadataExt},
    },
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, ErrorKind, translated_io_error},
    ops::{self, Cancel, Conflict, NameMode, Progress},
    trash,
};

/// Entries kept on each of the undo and redo stacks.
pub const DEPTH: usize = 50;
/// Past this many steps an entry is recorded as too large to undo, so the journal stays small.
pub const MAX_STEPS: usize = 10_000;

/// What makes an item the one a step left: device, inode, kind and birth time say which item it
/// is, and size and times say whether it was written to since.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    device: u64,
    inode: u64,
    kind: u32,
    size: u64,
    modified: (i64, u32),
    changed: (i64, u32),
    born: Option<(i64, u32)>,
}

impl Identity {
    /// The item at `path` itself, not what a link there points to.
    pub fn of(path: &Path) -> Option<Self> {
        use rustix::fs::{AtFlags, CWD, StatxFlags, statx};
        let stat = statx(
            CWD,
            path,
            AtFlags::SYMLINK_NOFOLLOW,
            StatxFlags::BASIC_STATS | StatxFlags::BTIME,
        )
        .ok()?;
        let born = StatxFlags::from_bits_retain(stat.stx_mask)
            .contains(StatxFlags::BTIME)
            .then_some((stat.stx_btime.tv_sec, stat.stx_btime.tv_nsec));
        Some(Self {
            device: u64::from(stat.stx_dev_major) << 32 | u64::from(stat.stx_dev_minor),
            inode: stat.stx_ino,
            kind: u32::from(stat.stx_mode) & 0o170_000,
            size: stat.stx_size,
            modified: (stat.stx_mtime.tv_sec, stat.stx_mtime.tv_nsec),
            changed: (stat.stx_ctime.tv_sec, stat.stx_ctime.tv_nsec),
            born,
        })
    }

    /// The same item. An inode number freed and given to a new file is told apart by its birth
    /// time where the filesystem records one.
    pub fn same_item(&self, other: &Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.kind == other.kind
            && !matches!((self.born, other.born), (Some(a), Some(b)) if a != b)
    }

    /// The same item, not written to or changed since.
    pub fn unchanged(&self, other: &Self) -> bool {
        self.same_item(other)
            && self.size == other.size
            && self.modified == other.modified
            && self.changed == other.changed
    }

    fn is_file(&self) -> bool {
        self.kind == 0o100_000
    }
}

/// One thing an operation did, with what undo needs to reverse it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Step {
    /// `copy` was made from `source`.
    Copied {
        #[serde(with = "path_bytes")]
        source: PathBuf,
        #[serde(with = "path_bytes")]
        copy: PathBuf,
        conflict: Conflict,
        identity: Identity,
    },
    /// The item that was at `from` is at `to`. `renamed` holds names inside it the destination
    /// could not store, as (asked, saved) paths relative to `to`'s folder.
    Moved {
        #[serde(with = "path_bytes")]
        from: PathBuf,
        #[serde(with = "path_bytes")]
        to: PathBuf,
        identity: Identity,
        #[serde(with = "path_pairs")]
        renamed: Vec<(PathBuf, PathBuf)>,
    },
    /// The item from `original` is in the Trash, recorded by `info`.
    Trashed {
        #[serde(with = "path_bytes")]
        original: PathBuf,
        #[serde(with = "path_bytes")]
        info: PathBuf,
        identity: Identity,
    },
    /// A copy of `source` took the place of `target`; what held it went to the Trash.
    Replaced {
        #[serde(with = "path_bytes")]
        source: PathBuf,
        #[serde(with = "path_bytes")]
        target: PathBuf,
        identity: Identity,
        original: Option<TrashedOriginal>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TrashedOriginal {
    #[serde(with = "path_bytes")]
    pub info: PathBuf,
    pub identity: Identity,
}

/// One undone thing, with what redo needs to do it again.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Redo {
    Copy {
        #[serde(with = "path_bytes")]
        source: PathBuf,
        #[serde(with = "path_bytes")]
        into: PathBuf,
        conflict: Conflict,
    },
    /// `identity` is the item undo put back at `from`.
    Move {
        #[serde(with = "path_bytes")]
        from: PathBuf,
        #[serde(with = "path_bytes")]
        to: PathBuf,
        identity: Identity,
    },
    Trash {
        #[serde(with = "path_bytes")]
        original: PathBuf,
        identity: Identity,
    },
    /// `original` is what undo put back at `target`, or None when it left the name empty.
    Replace {
        #[serde(with = "path_bytes")]
        source: PathBuf,
        #[serde(with = "path_bytes")]
        target: PathBuf,
        original: Option<Identity>,
    },
}

impl Step {
    /// The step for a copy of `source` published as `name` in `into`. A merge into a folder that
    /// was already there is not undoable as a whole: undo would trash the folder it merged into.
    pub fn copied(
        source: &Path,
        (into, name): (&Path, &std::ffi::OsStr),
        conflict: Conflict,
        merged_into_existing: bool,
    ) -> Option<Self> {
        if merged_into_existing {
            return None;
        }
        let copy = into.join(name);
        Some(Self::Copied {
            source: source.to_path_buf(),
            identity: Identity::of(&copy)?,
            copy,
            conflict,
        })
    }

    pub fn moved(source: &Path, (into, moved): (&Path, &ops::Moved)) -> Option<Self> {
        let to = into.join(&moved.name);
        Some(Self::Moved {
            from: source.to_path_buf(),
            identity: Identity::of(&to)?,
            to,
            renamed: moved
                .changes
                .renamed
                .iter()
                .map(|renamed| (renamed.requested.clone(), renamed.saved_as.clone()))
                .collect(),
        })
    }

    pub fn trashed(source: &Path, trashed: &trash::Trashed) -> Option<Self> {
        Some(Self::Trashed {
            original: source.to_path_buf(),
            info: trashed.info.clone(),
            identity: Identity::of(&trashed.file)?,
        })
    }

    pub fn replaced(source: &Path, (into, replaced): (&Path, &ops::Replaced)) -> Option<Self> {
        let target = into.join(&replaced.name);
        let original = match &replaced.original {
            Some(trashed) => Some(TrashedOriginal {
                info: trashed.info.clone(),
                identity: Identity::of(&trashed.file)?,
            }),
            None => None,
        };
        Some(Self::Replaced {
            source: source.to_path_buf(),
            identity: Identity::of(&target)?,
            target,
            original,
        })
    }

    /// The path a user would recognise this step by.
    pub fn path(&self) -> &Path {
        match self {
            Self::Copied { copy, .. } => copy,
            Self::Moved { to, .. } => to,
            Self::Trashed { original, .. } => original,
            Self::Replaced { target, .. } => target,
        }
    }
}

impl Redo {
    pub fn path(&self) -> &Path {
        match self {
            Self::Copy { source, .. } => source,
            Self::Move { from, .. } => from,
            Self::Trash { original, .. } => original,
            Self::Replace { target, .. } => target,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entry<T> {
    pub id: u64,
    /// What the user saw the operation called, such as "Copy 3 items".
    pub label: String,
    pub steps: Vec<T>,
    /// More steps than the journal keeps: the entry holds its place and says so.
    #[serde(default)]
    pub too_large: bool,
}

/// The labels the Edit menu shows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct History {
    pub undo: Option<(u64, String)>,
    pub redo: Option<(u64, String)>,
}

/// What an undo or redo did.
#[derive(Debug)]
pub struct Outcome {
    pub id: u64,
    pub label: String,
    /// Steps carried out.
    pub done: usize,
    /// Steps left as they were, by the path they concern, with why.
    pub left: Vec<(PathBuf, Error)>,
}

#[derive(Default, Serialize, Deserialize)]
struct Stacks {
    version: u32,
    next_id: u64,
    undo: Vec<Entry<Step>>,
    redo: Vec<Entry<Redo>>,
}

const VERSION: u32 = 1;
const FILE: &str = "undo-journal";
const LOCK: &str = "undo-journal.lock";

/// The journal in one folder, shared with every process that opens the same one.
#[derive(Clone, Debug)]
pub struct Journal {
    dir: PathBuf,
}

impl Journal {
    /// The session's journal, `$XDG_RUNTIME_DIR/asc-files`; an error without a runtime folder.
    pub fn session() -> Result<Self, Error> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|dir| !dir.is_empty())
            .ok_or_else(|| {
                translated_io_error("There is no session folder to keep undo history in")
            })?;
        Self::at(&PathBuf::from(runtime).join("asc-files"))
    }

    /// A journal in `dir`, made private to this user if it is new. A folder that is a link, or
    /// that another user owns or can write, is refused: its journal could steer an undo.
    pub fn at(dir: &Path) -> Result<Self, Error> {
        use std::os::unix::fs::PermissionsExt;
        match fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
        {
            Ok(()) => {}
            Err(error) => return Err(crate::error::os_io_error(error)),
        }
        let meta = fs::symlink_metadata(dir).map_err(crate::error::os_io_error)?;
        let uid = rustix::process::getuid().as_raw();
        if !meta.is_dir() || meta.uid() != uid || meta.permissions().mode() & 0o022 != 0 {
            return Err(translated_io_error(
                "The undo history folder is not private to this user",
            ));
        }
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    /// Files a new entry and clears what could be redone. An operation that changed nothing
    /// records nothing.
    pub fn record(&self, label: &str, steps: Vec<Step>) -> Result<Option<u64>, Error> {
        if steps.is_empty() {
            return Ok(None);
        }
        self.change(|stacks| {
            stacks.redo.clear();
            let id = stacks.next_id_bump();
            Some(push(&mut stacks.undo, id, label, steps))
        })
    }

    pub fn history(&self) -> Result<History, Error> {
        self.change(|stacks| {
            Some(History {
                undo: stacks
                    .undo
                    .last()
                    .map(|entry| (entry.id, entry.label.clone())),
                redo: stacks
                    .redo
                    .last()
                    .map(|entry| (entry.id, entry.label.clone())),
            })
        })
        .map(Option::unwrap_or_default)
    }

    /// Reverses the newest entry, newest step first. It leaves the history while it runs, so two
    /// windows never undo the same entry.
    pub fn undo(&self, cancel: &Cancel, progress: &Progress) -> Result<Outcome, Error> {
        let entry = self
            .change(|stacks| stacks.undo.pop())?
            .ok_or_else(|| nothing("There is nothing to undo"))?;
        if entry.too_large {
            return Err(translated_io_error("That operation was too large to undo"));
        }
        let mut redo = Vec::new();
        let mut left = Vec::new();
        for step in entry.steps.iter().rev() {
            if cancel.is_cancelled() {
                left.push((step.path().to_path_buf(), crate::error::cancelled()));
                continue;
            }
            match reverse(step, cancel, progress) {
                Ok((undone, nested)) => {
                    redo.push(undone);
                    left.extend(nested);
                }
                Err(error) => left.push((step.path().to_path_buf(), error)),
            }
        }
        redo.reverse();
        let done = redo.len();
        if !redo.is_empty() {
            let (id, label) = (entry.id, entry.label.clone());
            self.change(|stacks| {
                stacks.redo.push(Entry {
                    id,
                    label,
                    steps: redo,
                    too_large: false,
                });
                trim(&mut stacks.redo);
                Some(())
            })?;
        }
        Ok(Outcome {
            id: entry.id,
            label: entry.label,
            done,
            left,
        })
    }

    /// Does the newest undone entry again, oldest step first, and files it to be undone again.
    pub fn redo(&self, cancel: &Cancel, progress: &Progress) -> Result<Outcome, Error> {
        let entry = self
            .change(|stacks| stacks.redo.pop())?
            .ok_or_else(|| nothing("There is nothing to redo"))?;
        let mut steps = Vec::new();
        let mut left = Vec::new();
        for redo in &entry.steps {
            if cancel.is_cancelled() {
                left.push((redo.path().to_path_buf(), crate::error::cancelled()));
                continue;
            }
            match replay(redo, cancel, progress) {
                Ok(Some(step)) => steps.push(step),
                Ok(None) => {}
                Err(error) => left.push((redo.path().to_path_buf(), error)),
            }
        }
        let done = steps.len();
        if !steps.is_empty() {
            let (id, label) = (entry.id, entry.label.clone());
            // Redo files its entry without clearing what is left to redo.
            self.change(|stacks| Some(push(&mut stacks.undo, id, &label, steps)))?;
        }
        Ok(Outcome {
            id: entry.id,
            label: entry.label,
            done,
            left,
        })
    }

    /// Reads, changes and writes the journal under its lock. An unreadable journal starts afresh:
    /// losing history is better than refusing every operation.
    fn change<T>(&self, change: impl FnOnce(&mut Stacks) -> Option<T>) -> Result<Option<T>, Error> {
        use rustix::fs::{FlockOperation, flock};
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join(LOCK))
            .map_err(crate::error::os_io_error)?;
        flock(&lock, FlockOperation::LockExclusive).map_err(crate::error::os_io_error)?;
        let path = self.dir.join(FILE);
        let mut stacks = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Stacks>(&bytes)
                .ok()
                .filter(|stacks| stacks.version == VERSION)
                .unwrap_or_else(|| {
                    tracing::warn!("the undo history could not be read; starting afresh");
                    Stacks::default()
                }),
            Err(_) => Stacks::default(),
        };
        stacks.version = VERSION;
        let result = change(&mut stacks);
        let bytes = serde_json::to_vec(&stacks).map_err(crate::error::io_error)?;
        let temporary = self.dir.join(format!("{FILE}.new"));
        let mut file = fs::File::create(&temporary).map_err(crate::error::os_io_error)?;
        file.write_all(&bytes).map_err(crate::error::os_io_error)?;
        drop(file);
        fs::rename(&temporary, &path).map_err(crate::error::os_io_error)?;
        Ok(result)
    }
}

impl Stacks {
    fn next_id_bump(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

fn push(stack: &mut Vec<Entry<Step>>, id: u64, label: &str, steps: Vec<Step>) -> u64 {
    let too_large = steps.len() > MAX_STEPS;
    stack.push(Entry {
        id,
        label: label.to_owned(),
        steps: if too_large { Vec::new() } else { steps },
        too_large,
    });
    trim(stack);
    id
}

fn trim<T>(stack: &mut Vec<T>) {
    if stack.len() > DEPTH {
        stack.drain(..stack.len() - DEPTH);
    }
}

fn nothing(message: &str) -> Error {
    Error::new(ErrorKind::NotFound, &crate::i18n::tr(message))
}

fn changed() -> Error {
    translated_io_error("This item changed since, so it was left as it is")
}

/// Whether the item at `path` is still the one `identity` recorded.
fn still(path: &Path, identity: &Identity) -> Result<(), Error> {
    match Identity::of(path) {
        Some(now) if now.same_item(identity) => Ok(()),
        Some(_) => Err(changed()),
        None => Err(Error::new(
            ErrorKind::NotFound,
            &crate::i18n::tr("This item is no longer where the operation left it"),
        )),
    }
}

fn parent_and_name(path: &Path) -> Result<(&Path, &std::ffi::OsStr), Error> {
    Ok((
        path.parent()
            .ok_or_else(|| translated_io_error("The item has no folder"))?,
        path.file_name()
            .ok_or_else(|| translated_io_error("The item has no name"))?,
    ))
}

/// Takes away an item the operation made: to the Trash, or where its drive has none, deleted
/// if it is a file not written to since.
fn discard(path: &Path, identity: &Identity) -> Result<(), Error> {
    still(path, identity)?;
    match trash::put(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotSupported => {
            let now = Identity::of(path);
            if identity.is_file() && now.is_some_and(|now| now.unchanged(identity)) {
                fs::remove_file(path).map_err(crate::error::os_io_error)
            } else {
                Err(translated_io_error(
                    "This drive has no Trash and the item changed since, so it was left in place",
                ))
            }
        }
        Err(error) => Err(error),
    }
}

fn trash_entry(info: &Path, identity: &Identity) -> Result<trash::Entry, Error> {
    let entry = trash::list()
        .into_iter()
        .find(|entry| entry.info == info)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                &crate::i18n::tr("The item is no longer in the Trash"),
            )
        })?;
    still(&entry.file, identity)?;
    Ok(entry)
}

/// Reverses one step, giving what redo needs and any names inside it that stayed as they were.
fn reverse(
    step: &Step,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<(Redo, Vec<(PathBuf, Error)>), Error> {
    match step {
        Step::Copied {
            source,
            copy,
            conflict,
            identity,
        } => {
            discard(copy, identity)?;
            let (into, _) = parent_and_name(copy)?;
            Ok((
                Redo::Copy {
                    source: source.clone(),
                    into: into.to_path_buf(),
                    conflict: *conflict,
                },
                Vec::new(),
            ))
        }
        Step::Moved {
            from,
            to,
            identity,
            renamed,
        } => {
            still(to, identity)?;
            let (into, name) = parent_and_name(from)?;
            // The name it had: it was valid where it came from.
            ops::move_as(to, (into, name), NameMode::Keep, cancel, progress)?;
            let nested = give_names_back(from, to, renamed);
            let identity = Identity::of(from).ok_or_else(changed)?;
            Ok((
                Redo::Move {
                    from: from.clone(),
                    to: to.clone(),
                    identity,
                },
                nested,
            ))
        }
        Step::Trashed {
            original,
            info,
            identity,
        } => {
            let entry = trash_entry(info, identity)?;
            trash::restore(&entry)?;
            let identity = Identity::of(original).ok_or_else(changed)?;
            Ok((
                Redo::Trash {
                    original: original.clone(),
                    identity,
                },
                Vec::new(),
            ))
        }
        Step::Replaced {
            source,
            target,
            identity,
            original,
        } => {
            let Some(original) = original else {
                discard(target, identity)?;
                return Ok((
                    Redo::Replace {
                        source: source.clone(),
                        target: target.clone(),
                        original: None,
                    },
                    Vec::new(),
                ));
            };
            let entry = trash_entry(&original.info, &original.identity)?;
            still(target, identity)?;
            let copy = trash::put(target)?;
            if let Err(error) = trash::restore(&entry) {
                // Put the copy back, so the name is not left empty.
                let back = trash::list()
                    .into_iter()
                    .find(|listed| listed.info == copy.info)
                    .map(|listed| trash::restore(&listed));
                return Err(match back {
                    Some(Ok(_)) => error,
                    _ => crate::error::after_cleanup(
                        error,
                        Err(translated_io_error(
                            "The copy that replaced it is in the Trash",
                        )),
                    ),
                });
            }
            Ok((
                Redo::Replace {
                    source: source.clone(),
                    target: target.clone(),
                    original: Identity::of(target),
                },
                Vec::new(),
            ))
        }
    }
}

/// Renames, deepest first, the names a move into a stricter drive changed, now that the item is
/// back where they were valid. `renamed` is relative to `to`'s folder, with each saved path as
/// written; each name is changed back inside its folder as it now is, and a folder's own name
/// after its contents.
fn give_names_back(
    from: &Path,
    to: &Path,
    renamed: &[(PathBuf, PathBuf)],
) -> Vec<(PathBuf, Error)> {
    let Some(moved_name) = to.file_name() else {
        return Vec::new();
    };
    let back = |path: &Path| -> Option<PathBuf> {
        let mut components = path.components();
        match components.next() {
            Some(Component::Normal(first)) if first == moved_name => {}
            _ => return None,
        }
        let rest = components.as_path();
        (!rest.as_os_str().is_empty()).then(|| from.join(rest))
    };
    let mut pairs: Vec<(PathBuf, PathBuf)> = renamed
        .iter()
        .filter_map(|(asked, saved)| Some((back(asked)?, back(saved)?)))
        .collect();
    pairs.sort_by_key(|(_, saved)| std::cmp::Reverse(saved.components().count()));
    let mut left = Vec::new();
    for (asked, saved) in pairs {
        let Some(name) = asked.file_name() else {
            continue;
        };
        let given_back = saved.with_file_name(name);
        if let Err(error) = rename_no_replace(&saved, &given_back) {
            left.push((asked, error));
        }
    }
    left
}

fn rename_no_replace(from: &Path, to: &Path) -> Result<(), Error> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE).map_err(crate::error::os_io_error)
}

/// Does one undone thing again. A copy merged into a folder that is there now is done but not
/// recorded, as when it was first done.
fn replay(redo: &Redo, cancel: &Cancel, progress: &Progress) -> Result<Option<Step>, Error> {
    match redo {
        Redo::Copy {
            source,
            into,
            conflict,
        } => {
            let name = source
                .file_name()
                .ok_or_else(|| translated_io_error("The item has no name"))?;
            let merged_into_existing =
                *conflict == Conflict::Merge && fs::symlink_metadata(into.join(name)).is_ok();
            let copied = ops::copy(source, into, *conflict, NameMode::Fix, cancel, progress)?;
            Ok(Step::copied(
                source,
                (into, &copied.name),
                *conflict,
                merged_into_existing,
            ))
        }
        Redo::Move { from, to, identity } => {
            still(from, identity)?;
            let (into, name) = parent_and_name(to)?;
            let moved = ops::move_as(from, (into, name), NameMode::Fix, cancel, progress)?;
            Ok(Step::moved(from, (into, &moved)))
        }
        Redo::Trash { original, identity } => {
            still(original, identity)?;
            let trashed = trash::put(original)?;
            Ok(Step::trashed(original, &trashed))
        }
        Redo::Replace {
            source,
            target,
            original,
        } => {
            match (original, Identity::of(target)) {
                (None, None) => {}
                (Some(was), Some(now)) if now.same_item(was) => {}
                _ => return Err(changed()),
            }
            let (into, _) = parent_and_name(target)?;
            let replaced = ops::replace(source, into, NameMode::Fix, cancel, progress)?;
            Ok(Step::replaced(source, (into, &replaced)))
        }
    }
}

/// Paths as their bytes when they are not UTF-8, so no name is lost on the way to the file.
mod path_bytes {
    use super::*;

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    pub(super) enum Stored {
        Text(String),
        Bytes { bytes: Vec<u8> },
    }

    pub(super) fn stored(path: &Path) -> Stored {
        match path.to_str() {
            Some(text) => Stored::Text(text.to_owned()),
            None => Stored::Bytes {
                bytes: path.as_os_str().as_bytes().to_vec(),
            },
        }
    }

    pub(super) fn loaded(stored: Stored) -> PathBuf {
        match stored {
            Stored::Text(text) => PathBuf::from(text),
            Stored::Bytes { bytes } => PathBuf::from(OsString::from_vec(bytes)),
        }
    }

    pub(super) fn serialize<S: serde::Serializer>(path: &Path, to: S) -> Result<S::Ok, S::Error> {
        stored(path).serialize(to)
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        from: D,
    ) -> Result<PathBuf, D::Error> {
        Stored::deserialize(from).map(loaded)
    }
}

mod path_pairs {
    use super::{path_bytes::Stored, *};

    pub(super) fn serialize<S: serde::Serializer>(
        pairs: &[(PathBuf, PathBuf)],
        to: S,
    ) -> Result<S::Ok, S::Error> {
        pairs
            .iter()
            .map(|(a, b)| (path_bytes::stored(a), path_bytes::stored(b)))
            .collect::<Vec<_>>()
            .serialize(to)
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        from: D,
    ) -> Result<Vec<(PathBuf, PathBuf)>, D::Error> {
        Vec::<(Stored, Stored)>::deserialize(from).map(|pairs| {
            pairs
                .into_iter()
                .map(|(a, b)| (path_bytes::loaded(a), path_bytes::loaded(b)))
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_drive_changed_are_given_back_deepest_first() {
        let root = tempfile::tempdir().unwrap();
        // Moved back to `trip`; on the FAT drive it was `trip`, with these names changed.
        let from = root.path().join("trip");
        fs::create_dir_all(from.join("x_y/inner")).unwrap();
        fs::write(from.join("x_y/c_d"), b"c").unwrap();
        fs::write(from.join("a_b.txt"), b"a").unwrap();
        let to = root.path().join("fat/trip");
        let renamed = [
            ("trip/a:b.txt", "trip/a_b.txt"),
            ("trip/x:y", "trip/x_y"),
            ("trip/x:y/c:d", "trip/x_y/c_d"),
        ]
        .map(|(asked, saved)| (PathBuf::from(asked), PathBuf::from(saved)));

        let left = give_names_back(&from, &to, &renamed);

        assert!(left.is_empty(), "{left:?}");
        assert_eq!(fs::read(from.join("x:y/c:d")).unwrap(), b"c");
        assert!(from.join("x:y/inner").is_dir());
        assert_eq!(fs::read(from.join("a:b.txt")).unwrap(), b"a");
    }

    #[test]
    fn a_step_survives_the_journal_with_a_name_that_is_not_utf8() {
        let root = tempfile::tempdir().unwrap();
        let name = OsString::from_vec(b"caf\xe9".to_vec());
        let file = root.path().join(&name);
        fs::write(&file, b"x").unwrap();
        let step = Step::Copied {
            source: file.clone(),
            copy: file.clone(),
            conflict: Conflict::Fail,
            identity: Identity::of(&file).unwrap(),
        };

        let journal = Journal::at(&root.path().join("journal")).unwrap();
        journal.record("Copy", vec![step.clone()]).unwrap();
        let stored: Stacks =
            serde_json::from_slice(&fs::read(root.path().join("journal").join(FILE)).unwrap())
                .unwrap();

        assert_eq!(stored.undo[0].steps, [step]);
    }
}
