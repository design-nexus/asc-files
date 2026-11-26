// SPDX-License-Identifier: MIT
//! Permanent deletion: a pinned, parallel walk that never follows a renamed folder or a link out of
//! the tree; see [`crate::ops::delete`].

use super::fs_ops::{Entry, EntryStat, fs};
use crate::{
    model::{FileEntry, Location, hidden_names},
    services::{OperationEvent, OperationRequestId},
};

use super::{
    MAX_LOCAL_IO_WORKERS, fs_ops, io_error,
    leftovers::{self, Deadline, Refused},
    mount_profile::identity_at,
    native_fs::{
        self, Recorded, local_directory_children, open_local_parent_directory, os_error_detail,
        retry_local_open,
    },
};
use super::{
    translated_io_error,
    work_queue::{self, Group, Job, WorkQueue},
};
use std::{
    cell::Cell,
    collections::HashSet,
    ffi::{OsStr, OsString},
    future::Future,
    io,
    os::fd::{AsFd, OwnedFd},
    path::{Path, PathBuf},
    pin::Pin,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
    thread,
};

pub(super) enum LocalDeleteStep {
    Removed,
    Leftover,
    Directory {
        handle: OwnedFd,
        children: Vec<OsString>,
    },
}

pub(super) fn ensure_expected_local_identity<Fd: AsFd>(
    parent: &Fd,
    name: &OsStr,
    current: &EntryStat,
    expected: Option<Recorded>,
) -> Result<(), String> {
    let unchanged = |expected: Recorded| {
        native_fs::removable(
            expected,
            current,
            || identity_at(fs(), parent.as_fd()),
            hidden_names::classify(name).is_some(),
        )
    };
    if expected.is_some_and(|expected| !unchanged(expected)) {
        return Err(rust_i18n::t!(
            "%{name} changed while the operation was in progress",
            name = name.to_string_lossy()
        )
        .into_owned());
    }
    Ok(())
}

/// Check the name too: an open descriptor can outlive a rename.
pub(super) fn ensure_local_delete_target_unchanged<ParentFd: AsFd, TargetFd: AsFd>(
    parent: &ParentFd,
    name: &OsStr,
    target: &TargetFd,
) -> Result<(), String> {
    let named = fs()
        .stat(Entry::new(parent, name), false)
        .map_err(|error| {
            rust_i18n::t!(
                "%{name} changed while it was being deleted: %{error}",
                name = name.to_string_lossy(),
                error = os_error_detail(error)
            )
            .into_owned()
        })?;
    let opened = fs().stat_fd(target.as_fd()).map_err(|error| {
        rust_i18n::t!(
            "Could not recheck %{name}: %{error}",
            name = name.to_string_lossy(),
            error = os_error_detail(error)
        )
        .into_owned()
    })?;
    if named.identity != opened.identity {
        return Err(rust_i18n::t!(
            "%{name} changed while it was being deleted",
            name = name.to_string_lossy()
        )
        .into_owned());
    }
    Ok(())
}

pub(super) fn open_local_delete_target<Fd: AsFd>(
    parent: &Fd,
    name: &OsStr,
    expected: Option<Recorded>,
    leftovers_stay: bool,
) -> Result<LocalDeleteStep, String> {
    let is_leftover = || leftovers_stay && leftovers::is_leftover(fs(), parent.as_fd(), name);
    let stat = match fs().stat(Entry::new(parent, name), false) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) if is_leftover() => return Ok(LocalDeleteStep::Leftover),
        Err(error) => {
            return Err(rust_i18n::t!(
                "Could not inspect %{name}: %{error}",
                name = name.to_string_lossy(),
                error = os_error_detail(error)
            )
            .into_owned());
        }
    };
    ensure_expected_local_identity(parent, name, &stat, expected)?;
    if !stat.is_directory() {
        if is_leftover() {
            return Ok(LocalDeleteStep::Leftover);
        }
        fs().unlink(Entry::new(parent, name), rustix::fs::AtFlags::empty())
            .map_err(|error| {
                rust_i18n::t!(
                    "Could not delete “%{name}”: %{error}",
                    name = name.to_string_lossy(),
                    error = os_error_detail(error)
                )
                .into_owned()
            })?;
        return Ok(LocalDeleteStep::Removed);
    }
    // Reject symlinks swapped in after the type check, including intermediate components.
    let handle = retry_local_open(|| {
        fs().open_at(
            Entry::new(parent, name),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
            Some(
                rustix::fs::ResolveFlags::BENEATH
                    | rustix::fs::ResolveFlags::NO_SYMLINKS
                    | rustix::fs::ResolveFlags::NO_MAGICLINKS,
            ),
        )
    })
    .map_err(|error| {
        rust_i18n::t!(
            "%{name} changed while it was being deleted: %{error}",
            name = name.to_string_lossy(),
            error = os_error_detail(error)
        )
        .into_owned()
    })?;
    let opened = fs().stat_fd(handle.as_fd()).map_err(|error| {
        rust_i18n::t!(
            "Could not recheck %{name}: %{error}",
            name = name.to_string_lossy(),
            error = os_error_detail(error)
        )
        .into_owned()
    })?;
    ensure_expected_local_identity(parent, name, &opened, expected)?;
    let children = local_directory_children(&handle)?;
    Ok(LocalDeleteStep::Directory { handle, children })
}

#[derive(Clone)]
pub(crate) struct PinnedEntry {
    pub(crate) parent: Arc<OwnedFd>,
    pub(crate) name: OsString,
    pub(crate) identity: Option<Recorded>,
}

pub(super) struct LocalDeleteGuard {
    pub(super) parent: Arc<OwnedFd>,
    pub(super) name: OsString,
    pub(super) handle: Arc<OwnedFd>,
}

pub(super) type LocalDeleteQueue = WorkQueue<LocalDeleteJob>;

pub(super) enum LocalDeleteJob {
    Entry {
        parent: Arc<OwnedFd>,
        name: OsString,
        expected: Option<Recorded>,
        guard: Option<Arc<LocalDeleteGuard>>,
        completion: Option<Arc<Group<LocalDeleteJob>>>,
        deadline: Deadline,
    },
    RemoveDirectory {
        parent: Arc<OwnedFd>,
        name: OsString,
        handle: Arc<OwnedFd>,
        completion: Option<Arc<Group<LocalDeleteJob>>>,
        deadline: Deadline,
    },
}

impl Job for LocalDeleteJob {
    type Error = String;

    fn run(self, queue: &LocalDeleteQueue) {
        process_local_delete_job(queue, self);
    }

    fn cancelled() -> String {
        "Delete cancelled".to_owned()
    }

    fn panicked() -> String {
        "Delete worker panicked".to_owned()
    }

    fn not_started(error: io::Error) -> String {
        format!("Could not start delete worker: {error}")
    }
}

pub(super) fn process_local_delete_job(queue: &LocalDeleteQueue, job: LocalDeleteJob) {
    match job {
        LocalDeleteJob::Entry {
            parent,
            name,
            expected,
            guard,
            completion,
            deadline,
        } => {
            if queue.is_stopped() {
                return;
            }
            let in_deleted_folder = guard.is_some();
            if let Some(guard) = guard
                && let Err(error) = ensure_local_delete_target_unchanged(
                    guard.parent.as_ref(),
                    &guard.name,
                    guard.handle.as_ref(),
                )
            {
                queue.fail(error);
                return;
            }
            let step =
                match open_local_delete_target(parent.as_ref(), &name, expected, in_deleted_folder)
                {
                    Ok(step) => step,
                    Err(error) => {
                        queue.fail(error);
                        return;
                    }
                };
            match step {
                LocalDeleteStep::Removed | LocalDeleteStep::Leftover => queue.complete(completion),
                LocalDeleteStep::Directory { handle, children } => {
                    let handle = Arc::new(handle);
                    if children.is_empty() {
                        remove_local_delete_directory(
                            queue, parent, name, handle, completion, &deadline,
                        );
                        return;
                    }
                    let guard = Arc::new(LocalDeleteGuard {
                        parent: parent.clone(),
                        name: name.clone(),
                        handle: handle.clone(),
                    });
                    let group = Group::new(
                        children.len(),
                        LocalDeleteJob::RemoveDirectory {
                            parent: parent.clone(),
                            name,
                            handle: handle.clone(),
                            completion,
                            deadline: deadline.clone(),
                        },
                    );
                    for child in children {
                        queue.enqueue(LocalDeleteJob::Entry {
                            parent: handle.clone(),
                            name: child,
                            expected: None,
                            guard: Some(guard.clone()),
                            completion: Some(group.clone()),
                            deadline: deadline.clone(),
                        });
                    }
                }
            }
        }
        LocalDeleteJob::RemoveDirectory {
            parent,
            name,
            handle,
            completion,
            deadline,
        } => remove_local_delete_directory(queue, parent, name, handle, completion, &deadline),
    }
}

pub(super) fn remove_local_delete_directory(
    queue: &LocalDeleteQueue,
    parent: Arc<OwnedFd>,
    name: OsString,
    handle: Arc<OwnedFd>,
    completion: Option<Arc<Group<LocalDeleteJob>>>,
    deadline: &Deadline,
) {
    if queue.is_stopped() {
        return;
    }
    if let Err(error) =
        ensure_local_delete_target_unchanged(parent.as_ref(), &name, handle.as_ref())
    {
        queue.fail(error);
        return;
    }
    let changed = Cell::new(None);
    let unchanged = || {
        ensure_local_delete_target_unchanged(parent.as_ref(), &name, handle.as_ref())
            .map_err(|error| changed.set(Some(error)))
            .is_ok()
    };
    let folder = Entry::new(parent.as_ref(), &name);
    match leftovers::remove_folder(fs(), folder, deadline, &|| queue.is_stopped(), &unchanged) {
        Ok(()) => queue.complete(completion),
        Err(Refused::Stopped(_)) => {}
        Err(Refused::Changed(_)) => queue.fail(changed.take().unwrap_or_default()),
        Err(refused) => {
            let still_open = matches!(refused, Refused::StillOpen(_))
                .then(|| leftovers::still_open(fs(), folder))
                .flatten();
            queue.fail(
                rust_i18n::t!(
                    "Could not delete “%{name}”: %{error}",
                    name = name.to_string_lossy(),
                    error = still_open.map_or_else(
                        || os_error_detail(refused.errno()),
                        crate::services::error_detail
                    )
                )
                .into_owned(),
            );
        }
    }
}

pub(crate) fn local_device_is_rotational(fd: impl AsFd) -> Option<bool> {
    fs_ops::assert_off_main_thread();
    let stat = rustix::fs::fstat(fd).ok()?;
    let device = PathBuf::from("/sys/dev/block").join(format!(
        "{}:{}",
        rustix::fs::major(stat.st_dev),
        rustix::fs::minor(stat.st_dev)
    ));
    let device = std::fs::canonicalize(device).ok()?;
    rotational_sysfs_node(&device, &mut HashSet::new())
}

pub(super) fn rotational_sysfs_node(path: &Path, visited: &mut HashSet<PathBuf>) -> Option<bool> {
    let path = std::fs::canonicalize(path).ok()?;
    if !visited.insert(path.clone()) {
        return None;
    }

    let mut has_slave = false;
    let mut unknown_slave = false;
    if let Ok(slaves) = std::fs::read_dir(path.join("slaves")) {
        for slave in slaves.flatten() {
            has_slave = true;
            match rotational_sysfs_node(&slave.path(), visited) {
                Some(true) => return Some(true),
                Some(false) => {}
                None => unknown_slave = true,
            }
        }
    }
    if has_slave {
        return (!unknown_slave).then_some(false);
    }

    let mut current = Some(path.as_path());
    while let Some(node) = current {
        if let Ok(value) = std::fs::read_to_string(node.join("queue/rotational")) {
            return match value.trim() {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            };
        }
        current = node.parent();
    }
    None
}

pub(super) fn bounded_local_worker_count(available: usize, rotational: bool) -> usize {
    let available = available.max(1);
    if rotational {
        1
    } else {
        available.min(MAX_LOCAL_IO_WORKERS)
    }
}

pub(crate) fn local_delete_worker_count(roots: &[PinnedEntry]) -> usize {
    let available = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let rotational = roots
        .iter()
        .any(|root| fs().rotational(root.parent.as_fd()) == Some(true));
    bounded_local_worker_count(available, rotational)
}

pub(crate) fn parallel_delete_local_blocking_with_workers(
    roots: Vec<PinnedEntry>,
    cancelled: Arc<AtomicBool>,
    worker_count: usize,
    deadline: &Deadline,
) -> Result<(), String> {
    let worker_count = worker_count.max(1);
    let queue = Arc::new(LocalDeleteQueue::new(cancelled.clone()));
    for root in roots {
        queue.enqueue(LocalDeleteJob::Entry {
            parent: root.parent,
            name: root.name,
            expected: root.identity,
            guard: None,
            completion: None,
            deadline: deadline.clone(),
        });
    }

    run_local_delete_workers(&queue, worker_count, |work| {
        thread::Builder::new().spawn(work)
    });

    queue.result()
}

pub(super) fn run_local_delete_workers(
    queue: &Arc<LocalDeleteQueue>,
    worker_count: usize,
    spawn: impl FnMut(Box<dyn FnOnce() + Send>) -> std::io::Result<thread::JoinHandle<()>>,
) {
    work_queue::run_workers(queue, worker_count, Some(10), spawn);
}
