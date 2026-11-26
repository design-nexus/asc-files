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

use crate::adapters::local_operations::{leftovers, merge_aside};

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
    /// Relative to the merged folder, by the incoming names: what a plan's choices name.
    pub path: PathBuf,
    /// Where it meets what is there: relative to the folder merged into, by the names its drive
    /// stores (`nested_/a_.txt` for `nested?/a:.txt` on a FAT stick).
    pub stored: PathBuf,
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
    // Merging a folder into itself, or into a folder inside it, would feed on its own output.
    if ops::lands_within(source, into) || ops::lands_within(source, &into.join(&name)) {
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
        // Copies, and moves that cannot rename across, go through the engine's merge; moves within
        // a mount rename item by item, and a plan that leaves items out walks them one by one.
        Ok(existing)
            if existing.is_dir()
                && fs::symlink_metadata(source).is_ok_and(|meta| meta.is_dir())
                && (!moving || !ops::one_mount(source.parent().unwrap_or(source), &target))
                && !plan.skips() =>
        {
            merge_via_tree(
                source,
                (into, &name),
                plan,
                (names, moving, cancel, progress),
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
            if moving && let Ok(meta) = fs::symlink_metadata(source) {
                remove_if_empty(source, &meta, &mut merged);
            }
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
        // What holds the name the drive would store it under (a FAT stick keeps `a:b` as `a_b`).
        let stored = stored_name(target, &name, incoming_meta.is_dir());
        let existing = match fs::symlink_metadata(target.join(&stored)) {
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
                    (&incoming, &target.join(&stored)),
                    (&relative, &shown.join(&stored)),
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

/// A merge through the engine's own merge, as legacy did it: originals a Replace takes the place
/// of go to the Trash (or aside, where the drive has none), then the engine copies the folder in,
/// keeping both where a name is taken, so each entry appears whole, hard links stay links and a
/// target folder swapped for a link is not followed. Moving, it then deletes the sources it
/// copied once they are proven unchanged. What it did is read from the two trees before and
/// after, for undo.
fn merge_via_tree(
    source: &Path,
    (into, name): (&Path, &std::ffi::OsStr),
    plan: &MergePlan,
    (names, moving, cancel, progress): (NameMode, bool, &Cancel, &Progress),
    merged: &mut Merged,
) -> Result<(), Error> {
    let target = into.join(name);
    let mut replaced = std::collections::HashMap::new();
    // By where they land, what the incoming item kept beside it was.
    let mut keeping_both = std::collections::HashMap::new();
    // Where the drive has no Trash, originals wait in one hidden folder in the target, pinned by
    // identity, and are deleted once their replacements are on the drive (legacy `merge_aside`).
    let mut aside: Option<merge_aside::MergeAside> = None;
    // Opened before anything is written, so reaching it covers the copy.
    let settle = ops::Settle::open(into)?;
    // Against the folder merged into, which under a given name is not the source's own name.
    let mut found = Vec::new();
    // Folders that merge, by where they are in the target: where they are in the source.
    let mut merging_folders = std::collections::HashMap::new();
    find_clashes(
        source,
        &target,
        (Path::new(""), Path::new("")),
        (&mut found, &mut merging_folders),
    )?;
    // Items that land on one stored name (`a:.txt` and `a?.txt` on a FAT stick): the one whose
    // own name needs no fix keeps it, else the first; the copy numbers the others beside it, and
    // where such a number is taken already, that is what their choice meets.
    found.sort_by_key(|clash| {
        (
            clash.stored.clone(),
            clash.path.file_name() != clash.stored.file_name(),
            clash.path.clone(),
        )
    });
    let mut aliases = std::collections::HashMap::<PathBuf, usize>::new();
    for mut clash in found {
        let count = aliases.entry(clash.stored.clone()).or_default();
        *count += 1;
        if *count > 1 {
            let numbered = numbered(&clash.stored, *count - 1);
            match fs::symlink_metadata(target.join(&numbered)) {
                Ok(existing) if !existing.is_dir() => clash.stored = numbered,
                // Free (or a folder): the copy numbers it as it goes.
                _ => continue,
            }
        }
        match plan.choice(&clash.path) {
            Clash::Replace if !clash.existing.is_folder => {
                let at = target.join(&clash.stored);
                let original = match crate::trash::put(&at) {
                    Ok(trashed) => Ok(Original::Trashed(trashed)),
                    Err(error) if error.kind() == ErrorKind::NotSupported => {
                        merge_aside::set_aside(
                            &mut aside,
                            &target,
                            &at,
                            clash.path.display().to_string(),
                        )
                        .map(|()| Original::Aside)
                    }
                    Err(error) => Err(Error::new(
                        error.kind(),
                        &rust_i18n::t!(
                            "Could not move %{name} to Trash before merging: %{error}",
                            name = clash.path.display(),
                            error = error.message()
                        ),
                    )),
                };
                match original {
                    Ok(original) => {
                        replaced.insert(clash.stored, original);
                    }
                    // Nothing is merged: every original set aside so far goes back first.
                    Err(error) => {
                        let mut not_back = Vec::new();
                        for (path, original) in &replaced {
                            if let Err(left) = bring_back(path, original, &target) {
                                not_back.push(left.message().to_owned());
                            }
                        }
                        let (error, _) = merge_aside::abandon_keeping(aside, error);
                        if not_back.is_empty() {
                            return Err(error);
                        }
                        return Err(crate::error::io_error(format!(
                            "{}\n{}",
                            error.message(),
                            not_back.join("\n")
                        )));
                    }
                }
            }
            _ => {
                keeping_both.insert(clash.stored, clash.path);
            }
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
    let (result, changes, stopped) = if moving {
        match ops::merge_by_moving(source, (into, name), names, cancel, progress) {
            Ok(moved) => (Ok(()), moved.changes, moved.stopped),
            Err(error) => (Err(error), ops::Changes::default(), None),
        }
    } else {
        let (result, changes) =
            ops::merge_by_copying(source, (into, name), names, cancel, progress);
        (result, changes, None)
    };
    // The copy must be on the drive before an original set aside for it is deleted. A move has
    // reached its drive's barrier before deleting any source, so its copies already are.
    let unconfirmed = !moving && aside.is_some() && settle.reach().is_err();
    // An original whose incoming item was left out (a link the drive cannot hold, say) comes
    // back, so nothing it held is lost to a replacement that never came.
    // One that cannot come back is a failure that says where it is.
    let mut not_back = Vec::new();
    replaced.retain(|path, original| {
        let at = target.join(path);
        let placed = fs::symlink_metadata(&at).is_ok();
        match original {
            Original::Trashed(_) if !placed => {
                if let Err(error) = bring_back(path, original, &target) {
                    not_back.push((at.clone(), error));
                }
            }
            Original::Trashed(_) | Original::Aside => {}
        }
        placed
    });
    // Originals set aside whose names the merge left free go back; the rest are deleted once
    // the merge is whole and on the drive, or, if it is not, wait where the error says.
    let mut still_open = Vec::new();
    let failure = stopped.as_ref().or(result.as_ref().err());
    match merge_aside::finish(aside) {
        Err(error) => not_back.push((target.clone(), error)),
        Ok(None) => {}
        Ok(Some(aside)) if failure.is_some() || unconfirmed => {
            let error = failure.cloned().unwrap_or_else(|| {
                crate::error::io_error(crate::i18n::tr(
                    "The device could not finish writing. Earlier writes may still be pending; wait for safe eject before unplugging.",
                ))
            });
            let (error, open) = merge_aside::abandon_keeping(Some(aside), error);
            still_open.extend(open);
            not_back.push((target.clone(), error));
        }
        Ok(Some(aside)) => match aside.remove(&leftovers::Deadline::default()) {
            Ok(open) => still_open.extend(open),
            Err(error) => not_back.push((target.clone(), error)),
        },
    }
    let after = tree(&target);
    for path in after.difference(&before) {
        let topmost = path
            .parent()
            .is_none_or(|parent| parent.as_os_str().is_empty() || before.contains(parent));
        if !topmost {
            continue;
        }
        // A name the engine numbered to keep both came from the incoming item of that name.
        // A name the copy changed (cut, fixed, or numbered as it met another) came from the source
        // entry it was asked for.
        let renamed_from = || {
            changes.renamed.iter().find_map(|renamed| {
                (renamed.saved_as.strip_prefix(name).ok()? == path)
                    .then(|| {
                        renamed
                            .requested
                            .strip_prefix(name)
                            .ok()
                            .map(Path::to_path_buf)
                    })
                    .flatten()
            })
        };
        let (from, conflict) = if sources.contains(path) {
            (source.join(path), Conflict::Fail)
        } else if let Some(asked) = unnumbered(path).and_then(|kept| keeping_both.get(&kept)) {
            (source.join(asked), Conflict::KeepBoth)
        } else if let Some(asked) = renamed_from() {
            (source.join(asked), Conflict::Fail)
        } else if let Some(asked) = path
            .parent()
            .and_then(|parent| merging_folders.get(parent))
            .zip(path.file_name())
            .map(|(folder, name)| folder.join(name))
            .filter(|asked| sources.contains(asked))
        {
            // In a folder merged under the name its drive stores (`nested_` for `nested?`).
            (source.join(asked), Conflict::Fail)
        } else {
            continue;
        };
        let to = target.join(path);
        let original = replaced.remove(path).map(|original| match original {
            Original::Trashed(trashed) => Some(trashed),
            Original::Aside => None,
        });
        // A copy over an original deleted for want of a Trash is not undoable: undo would take
        // the copy away with nothing to put back, as legacy left it out.
        if !moving && matches!(original, Some(None)) {
            continue;
        }
        merged.done.push(match (moving, original) {
            (true, Some(original)) => Did::MovedOver { from, to, original },
            (true, None) => Did::Moved {
                from,
                to,
                renamed: Vec::new(),
            },
            (false, Some(original)) => Did::Replaced {
                target: to,
                source: from,
                original,
            },
            (false, None) => Did::Copied {
                copy: to,
                source: from,
                conflict,
            },
        });
    }
    if moving {
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
    }
    merged.renamed = changes.renamed;
    merged.skipped = changes.skipped;
    merged.kept = changes.kept;
    if let Some(error) = stopped {
        merged.failed.push((source.to_path_buf(), error));
    }
    merged.failed.extend(not_back);
    merged
        .kept
        .extend(still_open.into_iter().filter_map(|kept| {
            Some(ops::Kept {
                path: kept.location.native_path()?.to_path_buf(),
                cause: kept.cause,
            })
        }));
    result
        .map_err(|error| {
            merged.failed.push((source.to_path_buf(), error.clone()));
            error
        })
        .or(Ok(()))
}

/// The path a name numbered to keep both stood for: `a (2).txt` for `a.txt`, `photos (1)` for
/// `photos`.
/// `path` with " (n)" before its extension, as the copy numbers a name kept beside another.
fn numbered(path: &Path, n: usize) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (&name[..], ""),
    };
    path.with_file_name(format!("{stem} ({n}){extension}"))
}

fn unnumbered(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 && name[..dot].ends_with(')') => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    let open = stem.rfind(" (")?;
    let number = stem.get(open + 2..stem.len() - 1)?;
    if !stem.ends_with(')') || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(path.with_file_name(format!("{}{extension}", &stem[..open])))
}

/// Where an original a merge replaces went before the merge.
enum Original {
    Trashed(Trashed),
    /// In the merge's aside folder, which puts it back or deletes it.
    Aside,
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
            find_clashes(
                source,
                &target,
                (Path::new(""), Path::new("")),
                (&mut found, &mut Default::default()),
            )?;
        }
        Ok(_) | Err(_) => {}
    }
    Ok(found)
}

/// The name `folder`'s drive stores `name` under: its own, or the one it is fixed to.
fn stored_name(folder: &Path, name: &std::ffi::OsStr, is_directory: bool) -> OsString {
    crate::adapters::local_operations::typed_names::stored_in(folder, name, is_directory)
        .unwrap_or_else(|| name.to_os_string())
}

fn find_clashes(
    source: &Path,
    target: &Path,
    (relative, stored_relative): (&Path, &Path),
    (found, folders): (
        &mut Vec<Clashing>,
        &mut std::collections::HashMap<PathBuf, PathBuf>,
    ),
) -> Result<(), Error> {
    let mut entries = fs::read_dir(source)
        .map_err(crate::error::os_io_error)?
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect::<Vec<_>>();
    entries.sort();
    for name in entries {
        let Ok(incoming) = fs::symlink_metadata(source.join(&name)) else {
            continue;
        };
        // By the name the drive would store it under, as the merge will meet it.
        let stored = stored_name(target, &name, incoming.is_dir());
        let Ok(existing) = fs::symlink_metadata(target.join(&stored)) else {
            continue;
        };
        if incoming.is_dir() && existing.is_dir() {
            folders.insert(stored_relative.join(&stored), relative.join(&name));
            find_clashes(
                &source.join(&name),
                &target.join(&stored),
                (&relative.join(&name), &stored_relative.join(&stored)),
                (found, folders),
            )?;
        } else {
            found.push(Clashing {
                path: relative.join(&name),
                stored: stored_relative.join(&stored),
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

/// Puts back at its name an original set aside for a replacement that never came; a failure says
/// where the original is.
fn bring_back(path: &Path, original: &Original, target: &Path) -> Result<(), Error> {
    let at = target.join(path);
    match original {
        Original::Trashed(trashed) => {
            let Some(entry) = crate::trash::list()
                .into_iter()
                .find(|entry| entry.info == trashed.info)
            else {
                return Ok(());
            };
            crate::trash::restore(&entry).map(drop).map_err(|error| {
                crate::error::io_error(rust_i18n::t!(
                    "The original of “%{name}” stayed in the Trash, as “%{file}”: %{error}",
                    name = path.display(),
                    file = entry.file.display(),
                    error = error.message()
                ))
            })
        }
        // The merge's aside folder puts these back itself.
        Original::Aside => Ok(()),
    }
}

/// What pasting one item into a folder would meet, for the conflict prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub source: PathBuf,
    /// The name the item would be stored under there: its own, or the one the drive stores it
    /// as (a FAT stick turns `a:b.txt` into `a_b.txt`). The prompt passes it on as the item's
    /// `name`, so Replace replaces what holds that name rather than numbering a copy beside it.
    pub name: OsString,
    /// The drive would store the item under another name than its own.
    pub renamed: bool,
    pub incoming: Side,
    /// What holds that name now, if anything.
    pub existing: Option<Side>,
    /// What holds the name is the item itself (a paste into the folder it is in).
    pub same: bool,
    /// An earlier item in the same paste lands on the same name.
    pub alias_of: Option<usize>,
}

/// What pasting `sources` into `into` would meet, item by item, by the names the destination
/// would store them under. Nothing is changed.
pub fn plan(sources: &[PathBuf], into: &Path) -> Vec<Result<Planned, Error>> {
    use std::os::unix::fs::MetadataExt;
    let mut landed: Vec<OsString> = Vec::new();
    sources
        .iter()
        .map(|source| {
            let meta = fs::symlink_metadata(source).map_err(crate::error::os_io_error)?;
            let own = source
                .file_name()
                .ok_or_else(|| translated_io_error("The item has no name"))?;
            let stored =
                crate::adapters::local_operations::typed_names::stored_in(into, own, meta.is_dir());
            let name = stored.clone().unwrap_or_else(|| own.to_os_string());
            let existing = fs::symlink_metadata(into.join(&name)).ok();
            let same = existing.as_ref().is_some_and(|existing| {
                existing.dev() == meta.dev() && existing.ino() == meta.ino()
            });
            let alias_of = landed.iter().position(|earlier| *earlier == name);
            landed.push(name.clone());
            Ok(Planned {
                source: source.clone(),
                renamed: stored.is_some(),
                name,
                incoming: Side::of(&meta),
                existing: existing.as_ref().map(Side::of),
                same,
                alias_of,
            })
        })
        .collect()
}
