// SPDX-License-Identifier: MIT
//! Merging a folder into one of the same name, with a choice for each clash.
//!
//! [`clashes`] lists, for a conflict dialog, every incoming item whose name is taken inside the
//! folder; a [`MergePlan`] then says for each whether to keep both, replace what is there (its
//! original goes to the Trash once the copy holds the name, as [`ops::replace`] does), or leave
//! the incoming item out. Folders on both sides merge in turn. Everything else is copied in.
//!
//! The merge is built from the engine's own copy and replace, item by item, so each item it makes
//! or replaces is one undoable step, and a failure ends that item only: the merge goes on and
//! reports it. The folder merged into is never itself replaced or removed.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, ErrorKind, translated_io_error},
    ops::{self, Cancel, Conflict, NameMode, Progress, ProgressSnapshot, Renamed, Skipped},
    trash::Trashed,
};

/// What to do with an incoming item whose name is taken.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Clash {
    /// Copy it in beside the one there, as "name (1)".
    #[default]
    KeepBoth,
    /// Copy it in over the one there, whose original goes to the Trash.
    Replace,
    /// Leave it out; the one there stays.
    Skip,
}

/// The choices for a merge, by path relative to the merged folder (`photos/2024/a.jpg` is
/// `2024/a.jpg`). A clash not named takes `otherwise`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MergePlan {
    #[serde(with = "choices")]
    pub choices: BTreeMap<PathBuf, Clash>,
    pub otherwise: Clash,
}

impl MergePlan {
    /// Whether anything is to be left out.
    fn skips(&self) -> bool {
        self.otherwise == Clash::Skip || self.choices.values().any(|clash| *clash == Clash::Skip)
    }

    pub fn choice(&self, relative: &Path) -> Clash {
        self.choices
            .get(relative)
            .copied()
            .unwrap_or(self.otherwise)
    }
}

/// An incoming item whose name is taken inside the merged folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Clashing {
    /// Relative to the merged folder.
    pub path: PathBuf,
    pub incoming: Side,
    pub existing: Side,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Side {
    pub is_folder: bool,
    pub size: u64,
    pub modified: i64,
}

impl Side {
    fn of(meta: &fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            is_folder: meta.is_dir(),
            size: meta.len(),
            modified: meta.mtime(),
        }
    }
}

/// One thing a merge did, in the order it did them. Paths are absolute.
#[derive(Debug)]
pub enum Did {
    /// A whole new file or folder copied in from `source`.
    Copied {
        copy: PathBuf,
        source: PathBuf,
        conflict: Conflict,
    },
    /// A copy of `source` took the place of what was at `target`.
    Replaced {
        target: PathBuf,
        source: PathBuf,
        original: Option<Trashed>,
    },
    /// The item at `from` was moved to `to`; `renamed` as [`ops::Changes`] reports it, relative to
    /// `to`'s folder.
    Moved {
        from: PathBuf,
        to: PathBuf,
        renamed: Vec<(PathBuf, PathBuf)>,
    },
    /// The item at `from` was moved over what was at `to`.
    MovedOver {
        from: PathBuf,
        to: PathBuf,
        original: Option<Trashed>,
    },
    /// A source folder a merging move emptied was removed.
    RemovedFolder { path: PathBuf, mode: u32 },
}

/// A finished merge. Paths are absolute unless said otherwise.
#[derive(Debug, Default)]
pub struct Merged {
    /// The merged folder's name inside the destination.
    pub name: OsString,
    pub done: Vec<Did>,
    /// Incoming items the plan left out, relative to the merged folder.
    pub left_out: Vec<PathBuf>,
    /// Names the destination could not store, relative to the destination folder.
    pub renamed: Vec<Renamed>,
    /// Entries the destination could not hold, relative to the destination folder.
    pub skipped: Vec<Skipped>,
    /// Source entries a merging move left in place, and why.
    pub kept: Vec<ops::Kept>,
    /// Incoming items that failed, the merge going on past each.
    pub failed: Vec<(PathBuf, Error)>,
    pub progress: ProgressSnapshot,
}

impl Merged {
    /// Items copied in whole, as opposed to merged into or replacing one.
    pub fn created(&self) -> usize {
        self.done
            .iter()
            .filter(|did| matches!(did, Did::Copied { .. } | Did::Moved { .. }))
            .count()
    }

    pub fn replaced(&self) -> usize {
        self.done
            .iter()
            .filter(|did| matches!(did, Did::Replaced { .. } | Did::MovedOver { .. }))
            .count()
    }
}

/// Merges the folder `source` into the folder of the same name in `into`; where there is none,
/// this is a plain copy. Refused when what holds the name is not a folder.
pub fn merge(
    source: &Path,
    into: &Path,
    plan: &MergePlan,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Merged, Error> {
    merging(source, (into, None), plan, (names, false), cancel, progress)
}

/// Merges `source` into the folder called `name` in `into`, as [`merge`] does into the one of its
/// own name: the name a conflict prompt showed, say.
pub fn merge_as(
    source: &Path,
    (into, name): (&Path, &std::ffi::OsStr),
    plan: &MergePlan,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Merged, Error> {
    merging(
        source,
        (into, Some(name)),
        plan,
        (names, false),
        cancel,
        progress,
    )
}

/// Merges as [`merge`] does, moving instead of copying: each incoming item is moved in (a clash
/// replaced by moving over it), and each source folder left empty is removed. What the plan left
/// out stays where it was, and so does any folder still holding it.
pub fn merge_moving(
    source: &Path,
    into: &Path,
    plan: &MergePlan,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Merged, Error> {
    merging(source, (into, None), plan, (names, true), cancel, progress)
}

pub fn merge_moving_as(
    source: &Path,
    (into, name): (&Path, &std::ffi::OsStr),
    plan: &MergePlan,
    names: NameMode,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Merged, Error> {
    merging(
        source,
        (into, Some(name)),
        plan,
        (names, true),
        cancel,
        progress,
    )
}

fn merging(
    source: &Path,
    (into, named): (&Path, Option<&std::ffi::OsStr>),
    plan: &MergePlan,
    (names, moving): (NameMode, bool),
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Merged, Error> {
    let name = named
        .or_else(|| source.file_name())
        .ok_or_else(|| translated_io_error("The item has no name"))?
        .to_os_string();
    if ops::lands_within(source, into) {
        return Err(Error::new(
            ErrorKind::InvalidArgument,
            &crate::i18n::tr("A folder cannot be copied into itself"),
        ));
    }
    let target = into.join(&name);
    let mut merged = Merged {
        name: name.clone(),
        ..Merged::default()
    };
    match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let changes = if moving {
                let moved =
                    ops::move_as(source, (into, &name), names, cancel, progress)?.finished()?;
                merged.done.push(Did::Moved {
                    from: source.to_path_buf(),
                    to: into.join(&moved.name),
                    renamed: pairs(&moved.changes),
                });
                merged.name = moved.name;
                moved.changes
            } else {
                let copied = ops::copy_as(
                    source,
                    (into, &name),
                    Conflict::Fail,
                    names,
                    cancel,
                    progress,
                )?;
                merged.done.push(Did::Copied {
                    copy: into.join(&copied.name),
                    source: source.to_path_buf(),
                    conflict: Conflict::Fail,
                });
                merged.name = copied.name;
                copied.changes
            };
            merged.renamed = changes.renamed;
            merged.skipped = changes.skipped;
            merged.kept = changes.kept;
        }
        Err(error) => return Err(crate::error::os_io_error(error)),
        Ok(existing)
            if moving
                && existing.is_dir()
                && fs::symlink_metadata(source).is_ok_and(|meta| meta.is_dir())
                && !ops::one_mount(source.parent().unwrap_or(source), &target)
                && !plan.skips() =>
        {
            merge_across_drives(
                source,
                (into, &name),
                plan,
                (names, cancel, progress),
                &mut merged,
            )?;
        }
        Ok(existing)
            if existing.is_dir()
                && fs::symlink_metadata(source).is_ok_and(|meta| meta.is_dir()) =>
        {
            let into_name = Path::new(&name);
            merge_folder(
                (source, &target),
                (Path::new(""), into_name),
                plan,
                (names, moving, cancel, progress),
                &mut merged,
            );
        }
        Ok(_) => {
            return Err(Error::new(
                ErrorKind::Exists,
                &crate::i18n::tr("Only a folder can be merged into a folder of the same name"),
            ));
        }
    }
    merged.progress = progress.snapshot();
    Ok(merged)
}

/// Merges the contents of `source` into `target`. `relative` is the folder's path inside the
/// merged folder, `shown` inside the destination folder.
fn merge_folder(
    (source, target): (&Path, &Path),
    (relative, shown): (&Path, &Path),
    plan: &MergePlan,
    (names, moving, cancel, progress): (NameMode, bool, &Cancel, &Progress),
    merged: &mut Merged,
) {
    let mut entries = match fs::read_dir(source) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
            .collect::<Vec<_>>(),
        Err(error) => {
            merged
                .failed
                .push((source.to_path_buf(), crate::error::os_io_error(error)));
            return;
        }
    };
    entries.sort();
    for name in entries {
        let incoming = source.join(&name);
        let relative = relative.join(&name);
        if cancel.is_cancelled() {
            merged.failed.push((incoming, crate::error::cancelled()));
            continue;
        }
        let into = |result: Result<_, Error>, merged: &mut Merged| {
            if let Err(error) = result {
                merged.failed.push((incoming.clone(), error));
            }
        };
        let incoming_meta = match fs::symlink_metadata(&incoming) {
            Ok(meta) => meta,
            Err(error) => {
                into(Err(crate::error::os_io_error(error)), merged);
                continue;
            }
        };
        let existing = match fs::symlink_metadata(target.join(&name)) {
            Ok(existing) => Some(existing),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                into(Err(crate::error::os_io_error(error)), merged);
                continue;
            }
        };
        let clash = match &existing {
            None => None,
            Some(existing) if existing.is_dir() && incoming_meta.is_dir() => {
                merge_folder(
                    (&incoming, &target.join(&name)),
                    (&relative, &shown.join(&name)),
                    plan,
                    (names, moving, cancel, progress),
                    merged,
                );
                if moving {
                    remove_if_empty(&incoming, &incoming_meta, merged);
                }
                continue;
            }
            Some(_) => Some(plan.choice(&relative)),
        };
        let result = match (clash, moving) {
            (Some(Clash::Skip), _) => {
                merged.left_out.push(relative);
                Ok(())
            }
            (Some(Clash::Replace), false) => {
                ops::replace(&incoming, target, names, cancel, progress).map(|replaced| {
                    note_changes(merged, shown, &replaced.changes);
                    merged.done.push(Did::Replaced {
                        target: target.join(&replaced.name),
                        source: incoming.clone(),
                        original: replaced.original,
                    });
                })
            }
            (Some(Clash::Replace), true) => {
                ops::move_replacing(&incoming, target, names, cancel, progress).map(|replaced| {
                    note_changes(merged, shown, &replaced.changes);
                    merged.done.push(Did::MovedOver {
                        from: incoming.clone(),
                        to: target.join(&replaced.name),
                        original: replaced.original,
                    });
                })
            }
            (None | Some(Clash::KeepBoth), true) => {
                let moved = if clash.is_some() {
                    ops::move_keeping_both(&incoming, target, names, cancel, progress)
                } else {
                    ops::move_item(&incoming, target, names, cancel, progress)
                };
                moved.and_then(|moved| {
                    note_changes(merged, shown, &moved.changes);
                    if let Some(error) = moved.stopped {
                        return Err(error);
                    }
                    merged.done.push(Did::Moved {
                        from: incoming.clone(),
                        to: target.join(&moved.name),
                        renamed: pairs(&moved.changes),
                    });
                    Ok(())
                })
            }
            (None | Some(Clash::KeepBoth), false) => {
                let conflict = if clash.is_some() {
                    Conflict::KeepBoth
                } else {
                    Conflict::Fail
                };
                ops::copy(&incoming, target, conflict, names, cancel, progress).map(|copied| {
                    note_changes(merged, shown, &copied.changes);
                    merged.done.push(Did::Copied {
                        copy: target.join(&copied.name),
                        source: incoming.clone(),
                        conflict,
                    });
                })
            }
        };
        into(result, merged);
    }
}

/// A merging move to another drive, as legacy did it: originals a Replace takes the place of go
/// to the Trash, then the engine's merge copies the folder in (keeping both where a name is taken)
/// and deletes the sources it copied once they are proven unchanged. What it did is read from the
/// two trees before and after, for undo; an item it numbered is not filed.
fn merge_across_drives(
    source: &Path,
    (into, name): (&Path, &std::ffi::OsStr),
    plan: &MergePlan,
    (names, cancel, progress): (NameMode, &Cancel, &Progress),
    merged: &mut Merged,
) -> Result<(), Error> {
    let target = into.join(name);
    let mut replaced = std::collections::HashMap::new();
    for clash in clashes(source, into)? {
        if !clash.existing.is_folder && plan.choice(&clash.path) == Clash::Replace {
            let original = crate::trash::put(&target.join(&clash.path))?;
            replaced.insert(clash.path, original);
        }
    }
    let (sources, before) = (tree(source), tree(&target));
    let folders: Vec<(PathBuf, u32)> = std::iter::once(PathBuf::new())
        .chain(sources.iter().cloned())
        .filter_map(|path| {
            use std::os::unix::fs::PermissionsExt;
            let at = source.join(&path);
            let meta = fs::symlink_metadata(&at).ok()?;
            meta.is_dir()
                .then(|| (at, meta.permissions().mode() & 0o7777))
        })
        .collect();
    let moved = ops::merge_by_moving(source, (into, name), names, cancel, progress);
    // An original whose incoming item was left out (a link the drive cannot hold, say) comes
    // back, so nothing it held is lost to a replacement that never came.
    replaced.retain(|path, original| {
        if fs::symlink_metadata(target.join(path)).is_ok() {
            return true;
        }
        if let Some(entry) = crate::trash::list()
            .into_iter()
            .find(|entry| entry.info == original.info)
        {
            let _ = crate::trash::restore(&entry);
        }
        false
    });
    let moved = moved?;
    let after = tree(&target);
    for path in after.difference(&before) {
        let topmost = path
            .parent()
            .is_none_or(|parent| parent.as_os_str().is_empty() || before.contains(parent));
        if !topmost || !sources.contains(path) {
            continue;
        }
        let (from, to) = (source.join(path), target.join(path));
        merged.done.push(match replaced.remove(path) {
            Some(original) => Did::MovedOver {
                from,
                to,
                original: Some(original),
            },
            None => Did::Moved {
                from,
                to,
                renamed: Vec::new(),
            },
        });
    }
    // Undo makes again the source folders the deletion emptied, before moving back into them.
    let mut gone: Vec<_> = folders
        .into_iter()
        .filter(|(path, _)| fs::symlink_metadata(path).is_err())
        .collect();
    gone.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    merged.done.extend(
        gone.into_iter()
            .map(|(path, mode)| Did::RemovedFolder { path, mode }),
    );
    merged.renamed = moved.changes.renamed;
    merged.skipped = moved.changes.skipped;
    merged.kept = moved.changes.kept;
    if let Some(error) = moved.stopped {
        merged.failed.push((source.to_path_buf(), error));
    }
    Ok(())
}

/// Every path beneath `root`, relative to it, without following links.
fn tree(root: &Path) -> std::collections::BTreeSet<PathBuf> {
    fn walk(root: &Path, relative: &Path, found: &mut std::collections::BTreeSet<PathBuf>) {
        let Ok(entries) = fs::read_dir(root.join(relative)) else {
            return;
        };
        for entry in entries.flatten() {
            let path = relative.join(entry.file_name());
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                walk(root, &path, found);
            }
            found.insert(path);
        }
    }
    let mut found = std::collections::BTreeSet::new();
    walk(root, Path::new(""), &mut found);
    found
}

/// Removes a source folder a merging move emptied, remembering its mode for undo. A folder that
/// still holds anything (an item the plan left out, or one that failed) stays.
fn remove_if_empty(folder: &Path, meta: &fs::Metadata, merged: &mut Merged) {
    use std::os::unix::fs::PermissionsExt;
    if fs::remove_dir(folder).is_ok() {
        merged.done.push(Did::RemovedFolder {
            path: folder.to_path_buf(),
            mode: meta.permissions().mode() & 0o7777,
        });
    }
}

fn pairs(changes: &ops::Changes) -> Vec<(PathBuf, PathBuf)> {
    changes
        .renamed
        .iter()
        .map(|renamed| (renamed.requested.clone(), renamed.saved_as.clone()))
        .collect()
}

/// Changes an item's copy reports are relative to its folder; the merge's are relative to the
/// destination folder, which holds the merged folder at `shown`'s first component.
fn note_changes(merged: &mut Merged, shown: &Path, changes: &ops::Changes) {
    merged
        .renamed
        .extend(changes.renamed.iter().map(|renamed| Renamed {
            requested: shown.join(&renamed.requested),
            saved_as: shown.join(&renamed.saved_as),
            cause: renamed.cause,
        }));
    merged
        .skipped
        .extend(changes.skipped.iter().map(|skipped| Skipped {
            path: shown.join(&skipped.path),
            kind: skipped.kind,
        }));
    merged.kept.extend(changes.kept.iter().cloned());
}

/// Every incoming item whose name is taken inside the folder `source` would merge into, without
/// changing anything. Empty when there is no such folder yet.
pub fn clashes(source: &Path, into: &Path) -> Result<Vec<Clashing>, Error> {
    let name = source
        .file_name()
        .ok_or_else(|| translated_io_error("The item has no name"))?;
    let target = into.join(name);
    let mut found = Vec::new();
    match fs::symlink_metadata(&target) {
        Ok(existing) if existing.is_dir() => {
            find_clashes(source, &target, Path::new(""), &mut found)?;
        }
        Ok(_) | Err(_) => {}
    }
    Ok(found)
}

fn find_clashes(
    source: &Path,
    target: &Path,
    relative: &Path,
    found: &mut Vec<Clashing>,
) -> Result<(), Error> {
    let mut entries = fs::read_dir(source)
        .map_err(crate::error::os_io_error)?
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect::<Vec<_>>();
    entries.sort();
    for name in entries {
        let (Ok(incoming), Ok(existing)) = (
            fs::symlink_metadata(source.join(&name)),
            fs::symlink_metadata(target.join(&name)),
        ) else {
            continue;
        };
        if incoming.is_dir() && existing.is_dir() {
            find_clashes(
                &source.join(&name),
                &target.join(&name),
                &relative.join(&name),
                found,
            )?;
        } else {
            found.push(Clashing {
                path: relative.join(&name),
                incoming: Side::of(&incoming),
                existing: Side::of(&existing),
            });
        }
    }
    Ok(())
}

mod choices {
    use super::*;
    use crate::serde_path::{Stored, loaded, stored};

    pub(super) fn serialize<S: serde::Serializer>(
        choices: &BTreeMap<PathBuf, Clash>,
        to: S,
    ) -> Result<S::Ok, S::Error> {
        choices
            .iter()
            .map(|(path, clash)| (stored(path), *clash))
            .collect::<Vec<_>>()
            .serialize(to)
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        from: D,
    ) -> Result<BTreeMap<PathBuf, Clash>, D::Error> {
        Vec::<(Stored, Clash)>::deserialize(from).map(|choices| {
            choices
                .into_iter()
                .map(|(path, clash)| (loaded(path), clash))
                .collect()
        })
    }
}
