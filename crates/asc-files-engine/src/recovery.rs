// SPDX-License-Identifier: MIT
//! What an interrupted operation left beside its target: originals set aside while a replace or
//! merge was under way (`.asc-files-originals-*`, `.asc-files-merge-originals-*`) and the probes
//! that tested what a drive can do. A folder's listing names them; these put originals back under
//! their names, say what a leftover holds, and remove stale probes.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::{
    adapters::local_operations::{fs_ops::fs, leftovers},
    error::{Error, ErrorKind},
    model::hidden_names::{Leftover, classify},
    ops::{Cancel, Kept, Renamed},
    services::ReviewedLeftover,
};

/// What [`recover`] did with an originals folder.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Recovered {
    /// Where each original was put back.
    pub restored: Vec<PathBuf>,
    /// Originals put back under another name, their own being taken.
    pub renamed: Vec<Renamed>,
    /// Entries left in the folder, with why (still open in a program, say).
    pub kept: Vec<Kept>,
    /// Originals that could not be put back, with why.
    pub failed: Vec<(PathBuf, String)>,
    /// Originals a cancel left in the folder.
    pub not_attempted: Vec<PathBuf>,
    /// The folder is gone, everything in it having been put back.
    pub removed: bool,
}

/// Puts the originals in `container` back under their names beside it, never over an item that
/// holds a name now: such an original is put back numbered. A merge's originals go back into the
/// folders they came from. A container holding only another program's leftover is refused.
pub fn recover(container: &Path, cancel: &Cancel) -> Result<Recovered, Error> {
    let recovered = leftovers::recover(fs(), container, &|| cancel.is_cancelled())
        .map_err(|message| Error::new(ErrorKind::InvalidArgument, &message))?;
    let path = |location: &crate::model::Location| location.native_path().map(Path::to_path_buf);
    Ok(Recovered {
        restored: recovered.restored.iter().filter_map(path).collect(),
        renamed: recovered
            .report
            .renamed
            .iter()
            .filter_map(|renamed| {
                Some(Renamed {
                    requested: path(&renamed.requested)?,
                    saved_as: path(&renamed.saved_as)?,
                    cause: renamed.cause.clone(),
                })
            })
            .collect(),
        kept: recovered
            .report
            .kept
            .iter()
            .filter_map(|kept| {
                Some(Kept {
                    path: path(&kept.location)?,
                    cause: kept.cause.clone(),
                })
            })
            .collect(),
        failed: recovered
            .failed
            .iter()
            .filter_map(|(location, why)| Some((path(location)?, why.clone())))
            .collect(),
        not_attempted: recovered.not_attempted.iter().filter_map(path).collect(),
        removed: recovered.removed,
    })
}

/// What each leftover named in `folder` holds, up to a bound, for a prompt that asks what to do.
pub fn review(folder: &Path, names: &[OsString]) -> Vec<ReviewedLeftover> {
    leftovers::review(fs(), folder, names)
}

/// Removes the probes among `names` in `folder` that are empty and more than an hour old; each
/// is tried once per process.
pub fn sweep_probes(folder: &Path, names: &[OsString]) {
    leftovers::sweep_probes(fs(), folder, names);
}

/// Whether a listing's entry is something an interrupted operation left: an originals folder to
/// recover, or a probe to sweep.
pub fn is_leftover(name: &std::ffi::OsStr) -> bool {
    matches!(
        classify(name),
        Some(Leftover::Originals | Leftover::MergeOriginals | Leftover::Probe)
    )
}
