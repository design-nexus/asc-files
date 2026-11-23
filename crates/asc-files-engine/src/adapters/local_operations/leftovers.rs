// SPDX-License-Identifier: MIT
//! Spike: engine items extracted from the legacy adapters/local_operations/leftovers.rs.

use std::{
    cell::OnceCell,
    collections::HashSet,
    ffi::{OsStr, OsString},
    os::fd::{AsFd, BorrowedFd, OwnedFd},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, LazyLock, Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{
    UNSUPPORTED_REASON,
    copy_engine::now,
    deferred_close::{self, Pause, Wait},
    fs_ops::{Entry, EntryStat, FsOps, fs},
    holders,
    mount_profile::{known_mount, name_limit},
    names::numbered_names,
    native_fs::{open_local_parent_directory, os_error_detail},
    placement::{self, PlaceError},
    placement_reason,
};
use crate::{
    adapters::volume::{Family, SillyNames},
    model::{
        Location,
        hidden_names::{Leftover, classify, foreign_leftover},
    },
    services::{
        Holder, KeptCause, KeptItem, LeftoverInside, LoadHandle, OperationEvent, RecoverRequest,
        RenameCause, RenamedItem, ReviewedLeftover, TransferReport,
    },
};
use rustix::{
    fs::{AtFlags, FileType, Mode},
    io::Errno,
};

const LINGERS: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub(crate) struct Deadline(Arc<OnceLock<SystemTime>>);

impl Deadline {
    pub(super) fn wait(&self, budget: Duration) -> Wait {
        Wait::since(*self.0.get_or_init(now), budget)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Refused {
    Os(Errno),
    StillOpen(Errno),
    Stopped(Errno),
    Changed(Errno),
}

impl Refused {
    pub(super) fn errno(self) -> Errno {
        match self {
            Self::Os(errno)
            | Self::StillOpen(errno)
            | Self::Stopped(errno)
            | Self::Changed(errno) => errno,
        }
    }
}

pub(super) fn remove_folder(
    fs: &dyn FsOps,
    at: Entry<'_>,
    deadline: &Deadline,
    stopped: &dyn Fn() -> bool,
    unchanged: &dyn Fn() -> bool,
) -> Result<(), Refused> {
    let mut wait = None;
    let mut at_once = false;
    loop {
        let errno = match fs.unlink(at, AtFlags::REMOVEDIR) {
            Ok(()) => return Ok(()),
            Err(errno) => errno,
        };
        let Some(held) = held_only(fs, at, errno) else {
            return Err(Refused::Os(errno));
        };
        at_once = held.emptied && !at_once;
        if !at_once {
            let wait = wait.get_or_insert_with(|| deadline.wait(held.budget));
            match wait.pause(stopped) {
                Pause::Retry => {}
                Pause::Spent => return Err(Refused::StillOpen(errno)),
                Pause::Cancelled => return Err(Refused::Stopped(errno)),
            }
        }
        if !unchanged() {
            return Err(Refused::Changed(errno));
        }
    }
}

pub(super) fn remove_held_folder(
    fs: &dyn FsOps,
    at: Entry<'_>,
    held: BorrowedFd<'_>,
    deadline: &Deadline,
    stopped: &dyn Fn() -> bool,
) -> Result<(), Refused> {
    let unchanged = || match (fs.stat(at, false), fs.stat_fd(held)) {
        (Ok(named), Ok(held)) => named.identity == held.identity,
        _ => false,
    };
    if !unchanged() {
        return Err(Refused::Changed(Errno::NOENT));
    }
    remove_folder(fs, at, deadline, stopped, &unchanged)
}

pub(super) fn holder(fs: &dyn FsOps, at: Entry<'_>) -> Option<Holder> {
    let leftovers: Vec<_> = fs
        .names(at)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| foreign_leftover(name).is_some())
        .collect();
    holders::find(fs, at, &leftovers)
}

pub(super) fn still_open(fs: &dyn FsOps, at: Entry<'_>) -> Option<String> {
    holders::held_inside(holder(fs, at).as_ref())
        .or_else(|| holders::held_elsewhere(known_mount(fs, at.dir).1.silly_names))
}

pub(super) fn is_leftover(fs: &dyn FsOps, dir: BorrowedFd<'_>, name: &OsStr) -> bool {
    foreign_leftover(name).is_some_and(|family| {
        known_mount(fs, dir).1.silly_names == Some(SillyNames::Renamed(family))
    })
}

struct Held {
    emptied: bool,
    budget: Duration,
}

fn held_only(fs: &dyn FsOps, at: Entry<'_>, errno: Errno) -> Option<Held> {
    if !matches!(errno, Errno::NOTEMPTY | Errno::EXIST | Errno::ACCESS) {
        return None;
    }
    let names = fs.names(at).ok()?;
    if names.iter().any(|name| foreign_leftover(name).is_none()) {
        return None;
    }
    let (_, capabilities) = known_mount(fs, at.dir);
    let only_leftovers = match (capabilities.silly_names?, errno) {
        (SillyNames::Renamed(family), Errno::NOTEMPTY | Errno::EXIST) => names
            .iter()
            .all(|name| foreign_leftover(name) == Some(family)),
        (SillyNames::Unlisted, Errno::NOTEMPTY | Errno::EXIST | Errno::ACCESS) => names.is_empty(),
        _ => false,
    };
    only_leftovers.then(|| Held {
        emptied: names.is_empty() && errno != Errno::ACCESS,
        budget: capabilities
            .deferred_close
            .map_or(LINGERS, deferred_close::closing),
    })
}

fn folder(fs: &dyn FsOps, at: Entry<'_>) -> Result<OwnedFd, PlaceError> {
    match placement::open_beneath(fs, at.dir, Path::new(at.name)) {
        Err(PlaceError::Io(Errno::NOENT)) => {}
        opened => return opened,
    }
    match fs.mkdir(at, Mode::from_raw_mode(0o777)) {
        Ok(()) | Err(Errno::EXIST) => placement::open_beneath(fs, at.dir, Path::new(at.name)),
        Err(error) => Err(placement::creation_error(error)),
    }
}

fn listed(fs: &dyn FsOps, held: &OwnedFd) -> Result<Vec<OsString>, Errno> {
    let mut names = fs.names(Entry::new(held, OsStr::new(".")))?;
    names.sort();
    Ok(names)
}

fn only_leftovers(fs: &dyn FsOps, folder: Entry<'_>) -> bool {
    let Ok(held) = placement::open_beneath(fs, folder.dir, Path::new(folder.name)) else {
        return false;
    };
    let Ok(names) = listed(fs, &held) else {
        return false;
    };
    !names.is_empty()
        && names.iter().all(|name| {
            let entry = Entry::new(&held, name);
            is_leftover(fs, held.as_fd(), name)
                || fs.stat(entry, false).is_ok_and(|stat| stat.is_directory())
                    && only_leftovers(fs, entry)
        })
}
