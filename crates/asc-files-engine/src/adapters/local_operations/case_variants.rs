// SPDX-License-Identifier: MIT

//! Top-level items of one paste whose names differ only in case. On a
//! case-insensitive destination whose rules do not fold, such as a casefolded
//! folder, only looking a later name up shows that an earlier item took it.

use std::{
    collections::HashMap,
    ffi::OsString,
    os::fd::AsFd,
    path::{Path, PathBuf},
};

use super::{
    copy_engine::stage::collision_key,
    fs_ops::{Entry, fs},
    native_fs::{LocalFileIdentity, open_local_parent_directory},
    placement,
};

/// An item of the batch as saved: where, and which file it is.
#[derive(Clone)]
pub(crate) struct Saved(PathBuf, LocalFileIdentity);

pub(crate) struct CaseVariants {
    /// The other items of each one, by index, whose names differ only in case.
    others: HashMap<usize, Vec<usize>>,
    saved: HashMap<usize, Saved>,
}

impl CaseVariants {
    pub(crate) fn new(names: impl IntoIterator<Item = (usize, OsString)>) -> Self {
        let mut keyed = HashMap::<OsString, Vec<(usize, OsString)>>::new();
        for (index, name) in names {
            keyed
                .entry(collision_key(&name))
                .or_default()
                .push((index, name));
        }
        let mut others = HashMap::new();
        for items in keyed.values().filter(|items| items.len() > 1) {
            for (index, name) in items {
                let spelled_otherwise = items
                    .iter()
                    .filter(|(_, other)| other != name)
                    .map(|(other, _)| *other)
                    .collect::<Vec<_>>();
                if !spelled_otherwise.is_empty() {
                    others.insert(*index, spelled_otherwise);
                }
            }
        }
        Self {
            others,
            saved: HashMap::new(),
        }
    }

    pub(crate) fn follows(&self, index: usize, earlier: usize) -> bool {
        earlier < index
            && self
                .others
                .get(&index)
                .is_some_and(|others| others.contains(&earlier))
    }

    /// Records what `index` was saved as, where a later item may find it.
    pub(crate) fn saved(&mut self, index: usize, target: &Path) {
        let precedes = self
            .others
            .get(&index)
            .is_some_and(|others| others.iter().any(|&other| other > index));
        if precedes && let Some(identity) = identity(target) {
            self.saved
                .insert(index, Saved(target.to_path_buf(), identity));
        }
    }

    pub(crate) fn earlier(&self, index: usize) -> Vec<Saved> {
        self.others
            .get(&index)
            .into_iter()
            .flatten()
            .filter(|&&other| other < index)
            .filter_map(|other| self.saved.get(other).cloned())
            .collect()
    }
}

/// Whether the name `target` asks for is held by one of the items saved earlier under another
/// spelling.
pub(crate) fn taken_by(target: &Path, earlier: &[Saved]) -> bool {
    if earlier.is_empty() {
        return false;
    }
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return false;
    };
    let Ok(dir) = open_local_parent_directory(parent) else {
        return false;
    };
    let to = Entry::new(&dir, name);
    let Ok(found) = fs().stat(to, false) else {
        return false;
    };
    earlier.iter().any(|Saved(saved, identity)| {
        found.identity == *identity
            || saved
                .parent()
                .zip(saved.file_name())
                .and_then(|(parent, name)| {
                    let saved_dir = open_local_parent_directory(parent).ok()?;
                    placement::alias_of(fs(), Entry::new(&saved_dir, name), to)
                })
                .is_some_and(|alias| alias == *identity)
    })
}

fn identity(target: &Path) -> Option<LocalFileIdentity> {
    let (parent, name) = (target.parent()?, target.file_name()?);
    let dir = open_local_parent_directory(parent).ok()?;
    fs().stat(Entry::new(&dir, name), false)
        .ok()
        .map(|stat| stat.identity)
}

/// Whether the names `folder`'s drive stores fold case by its rules (SMB without POSIX
/// extensions, FAT, NTFS): there two spellings are known to meet without looking either up.
pub(crate) fn folds(folder: &Path) -> bool {
    open_local_parent_directory(folder)
        .is_ok_and(|dir| super::mount_profile::name_rules(fs(), dir.as_fd()).folds)
}
