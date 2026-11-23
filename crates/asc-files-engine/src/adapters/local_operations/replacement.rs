// SPDX-License-Identifier: MIT
//! Spike: engine items extracted from the legacy adapters/local_operations/replacement.rs.

use super::fs_ops::{Entry, EntryStat, OffMain, fs, list_folder};
use crate::{
    model::{Location, hidden_names::HiddenName},
    services::{
        ItemIdentity, KeptCause, KeptItem, PlacedSnapshot, ReplacedOriginal, TransferReport,
    },
};

use super::placement::{Displacement, Exchanged, PlaceError, RenameFailure};
use super::{
    blocking_join_message, cancelled_local_operation, copy_engine, copy_failure_after_cleanup,
    deletion::{
        PinnedEntry, local_delete_worker_count, parallel_delete_local_blocking_with_workers,
    },
    io_error,
    leftovers::Deadline,
    mount_profile::{self, NamePolicy, known_mount},
    native_fs::{
        LocalFileIdentity, Pinned, Recorded, open_local_parent_directory, os_error_detail,
    },
    placement, placement_error_kind, placement_reason, translated_io_error,
};
use std::{
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    future::Future,
    ops::ControlFlow,
    os::fd::{AsFd, OwnedFd},
    path::{Path, PathBuf},
    pin::Pin,
    rc::Rc,
    sync::{Arc, atomic::AtomicUsize},
};

pub(super) enum StageContent {
    Directory,
    Symlink(OsString),
    Fifo,
}

/// Created and discarded off the main thread; there is no `Drop` cleanup.
pub(super) struct StagedSibling {
    pub(super) ownership: Option<copy_engine::Ownership>,
    pub(super) parent: Arc<OwnedFd>,
    pub(super) name: OsString,
    pub(super) identity: LocalFileIdentity,
    pub(super) restrictive: Vec<copy_engine::Restrictive>,
}

impl StagedSibling {
    pub(super) fn create(parent: Arc<OwnedFd>, content: StageContent) -> Result<Self, PlaceError> {
        let (name, identity) = placement::reserve_hidden_name(HiddenName::Copy, |name| {
            let entry = Entry::new(&*parent, name);
            let made = match &content {
                StageContent::Directory => {
                    fs().mkdir(entry, rustix::fs::Mode::from_raw_mode(0o700))
                }
                StageContent::Symlink(link) => fs().symlink(link, entry),
                StageContent::Fifo => fs().create_node(
                    entry,
                    rustix::fs::FileType::Fifo,
                    rustix::fs::Mode::from_raw_mode(0o600),
                ),
            };
            match made {
                Ok(()) => fs()
                    .stat(entry, false)
                    .map(|stat| stat.identity)
                    .map_err(PlaceError::Io),
                Err(rustix::io::Errno::EXIST) => {
                    placement::left_by_lost_reply(fs(), entry, |stat| match &content {
                        StageContent::Directory => {
                            stat.is_directory()
                                && fs().names(entry).is_ok_and(|names| names.is_empty())
                        }
                        StageContent::Symlink(link) => {
                            stat.kind == rustix::fs::FileType::Symlink
                                && fs().readlink(entry).is_ok_and(|current| current == *link)
                        }
                        StageContent::Fifo => stat.kind == rustix::fs::FileType::Fifo,
                    })
                    .ok_or(PlaceError::Exists)
                }
                Err(error) => Err(PlaceError::Io(error)),
            }
        })?;
        Ok(Self {
            ownership: None,
            parent,
            name,
            identity,
            restrictive: Vec::new(),
        })
    }

    pub(super) fn recorded(&self) -> Recorded {
        Recorded {
            identity: self.identity,
            attributes: self
                .ownership
                .as_ref()
                .and_then(copy_engine::Ownership::attributes),
        }
    }
}
