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

/// A finished merge. Paths are absolute unless said otherwise.
#[derive(Debug, Default)]
pub struct Merged {
    /// The merged folder's name inside the destination.
    pub name: OsString,
    /// Items the merge made, each a whole new file or folder, with the incoming item each came from.
    pub created: Vec<(PathBuf, PathBuf, Conflict)>,
    /// Items that took the place of one already there, with the incoming item each came from and
    /// where the original went.
    pub replaced: Vec<(PathBuf, PathBuf, Option<Trashed>)>,
    /// Incoming items the plan left out, relative to the merged folder.
    pub left_out: Vec<PathBuf>,
    /// Names the destination could not store, relative to the destination folder.
    pub renamed: Vec<Renamed>,
    /// Entries the destination could not hold, relative to the destination folder.
    pub skipped: Vec<Skipped>,
    /// Incoming items that failed, the merge going on past each.
    pub failed: Vec<(PathBuf, Error)>,
    pub progress: ProgressSnapshot,
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
    let name = source
        .file_name()
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
            let copied = ops::copy(source, into, Conflict::Fail, names, cancel, progress)?;
            merged.created.push((
                into.join(&copied.name),
                source.to_path_buf(),
                Conflict::Fail,
            ));
            merged.name = copied.name;
            merged.renamed = copied.changes.renamed;
            merged.skipped = copied.changes.skipped;
        }
        Err(error) => return Err(crate::error::os_io_error(error)),
        Ok(existing)
            if existing.is_dir()
                && fs::symlink_metadata(source).is_ok_and(|meta| meta.is_dir()) =>
        {
            let into_name = Path::new(&name);
            merge_folder(
                (source, &target),
                (Path::new(""), into_name),
                plan,
                (names, cancel, progress),
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
    (names, cancel, progress): (NameMode, &Cancel, &Progress),
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
                    (names, cancel, progress),
                    merged,
                );
                continue;
            }
            Some(_) => Some(plan.choice(&relative)),
        };
        let result = match clash {
            Some(Clash::Skip) => {
                merged.left_out.push(relative);
                Ok(())
            }
            Some(Clash::Replace) => {
                ops::replace(&incoming, target, names, cancel, progress).map(|replaced| {
                    note_changes(merged, shown, &replaced.changes);
                    merged.replaced.push((
                        target.join(&replaced.name),
                        incoming.clone(),
                        replaced.original,
                    ));
                })
            }
            None | Some(Clash::KeepBoth) => {
                let conflict = if clash.is_some() {
                    Conflict::KeepBoth
                } else {
                    Conflict::Fail
                };
                ops::copy(&incoming, target, conflict, names, cancel, progress).map(|copied| {
                    note_changes(merged, shown, &copied.changes);
                    merged
                        .created
                        .push((target.join(&copied.name), incoming.clone(), conflict));
                })
            }
        };
        into(result, merged);
    }
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
