// SPDX-License-Identifier: MIT
//! The engine's behavioural tests, ported from the legacy app's `adapters/local_operations/tests`
//! (A8; see docs/PORTING-LEDGER.md), and their test seams: the fault-injecting filesystem and the
//! copy-activity counter.
//!
//! Legacy drove the engine through GLib futures; these call its synchronous core directly. The
//! stand-ins below keep the legacy helpers' names and meaning so each test reads as it did.
#![allow(dead_code)]
#![allow(unused_imports)]

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    error::Error,
    ffi::{OsStr, OsString},
    fs, io,
    os::{
        fd::{AsFd, OwnedFd},
        unix::{ffi::OsStringExt, fs::PermissionsExt},
    },
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use fs_quirks::{FsQuirks, Preset, Quirks};

use super::{
    copy_engine::{self, Destination, Options, SharedProgress},
    mount_profile::NamePolicy,
    names::{Taken, numbered_name, numbered_names, parse_copy_suffix},
    native_fs::{LocalFileIdentity, open_local_parent_directory},
};
use crate::{
    error::{Error as EngineError, ErrorKind},
    model::{Location, hidden_names::HiddenName},
    services::{
        ItemIdentity, NameMode, OperationEvent, OperationRequestId, PasteItem, PasteRequest,
        RenamedItem, SkippedItem, TransferConflict, TransferReport,
    },
};

mod aliases;
mod conflicts;
mod create_entry;
mod deletion;
mod file_copy;
mod folder_copy;
pub(crate) mod fs_quirks;
mod leftovers;
mod merge;
mod moves;
mod naming;
mod paste_results;
mod paths;
mod placement;
mod replacement;

/// Tests that change process-wide state (the clock, the kernel release, mount capabilities)
/// run one at a time; legacy held the GLib main context for this.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    Ok(SERIAL.lock().unwrap_or_else(|error| error.into_inner()))
}

/// The legacy `gio::Cancellable`, as the engine's cancel flag.
#[derive(Clone, Default)]
struct Cancellable(Arc<AtomicBool>);

impl Cancellable {
    fn new() -> Self {
        Self::default()
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn flag(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }
}

/// File copies running at once, and the most seen, across copy workers.
static COPY_ACTIVITY: std::sync::Mutex<(usize, usize)> = std::sync::Mutex::new((0, 0));

fn copy_activity() -> std::sync::MutexGuard<'static, (usize, usize)> {
    COPY_ACTIVITY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub(super) struct CopyActivity;

impl CopyActivity {
    pub(super) fn start() -> Self {
        let mut activity = copy_activity();
        activity.0 += 1;
        activity.1 = activity.1.max(activity.0);
        Self
    }
}

impl Drop for CopyActivity {
    fn drop(&mut self) {
        copy_activity().0 -= 1;
    }
}

fn fixing() -> NamePolicy {
    NamePolicy::new(NameMode::Fix)
}

struct ClockGuard;

impl ClockGuard {
    fn set(now: SystemTime) -> Self {
        super::copy_engine::set_clock_for_test(Some(now));
        Self
    }
}

impl Drop for ClockGuard {
    fn drop(&mut self) {
        super::copy_engine::set_clock_for_test(None);
    }
}

struct KernelGuard;

impl KernelGuard {
    fn set(release: Option<(u32, u32, u32)>) -> Self {
        super::copy_engine::set_kernel_for_test(release);
        Self
    }
}

impl Drop for KernelGuard {
    fn drop(&mut self) {
        super::copy_engine::set_kernel_for_test(None);
    }
}

struct PlacedEntriesGuard;

impl PlacedEntriesGuard {
    fn set(limit: usize) -> Self {
        super::copy_engine::set_placed_entries_for_test(Some(limit));
        Self
    }
}

impl Drop for PlacedEntriesGuard {
    fn drop(&mut self) {
        super::copy_engine::set_placed_entries_for_test(None);
    }
}

fn separate_volumes(source: &Path, destination: &Path) -> (FsQuirks, FsQuirks) {
    (
        FsQuirks::install(source, Quirks::default()),
        FsQuirks::install(destination, Quirks::default()),
    )
}

pub(super) fn names(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    Ok(names)
}

/// Where inode numbers are not trusted, a file born within one birth-time tick
/// of `than` passes for it.
fn wait_for_a_later_birth_time(probe: &Path, than: &fs::Metadata) {
    let Ok(born) = than.created() else {
        return;
    };
    loop {
        fs::write(probe, b"probe").expect("write the probe");
        let stamped = fs::metadata(probe).expect("stat the probe");
        fs::remove_file(probe).expect("remove the probe");
        if stamped.created().is_ok_and(|later| later > born) {
            break;
        }
    }
}

fn set_times(path: &Path, seconds: u64, nanoseconds: u32) -> io::Result<()> {
    let times = rustix::fs::Timespec {
        tv_sec: i64::try_from(seconds).map_err(io::Error::other)?,
        tv_nsec: nanoseconds.into(),
    };
    rustix::fs::utimensat(
        rustix::fs::CWD,
        path,
        &rustix::fs::Timestamps {
            last_access: times,
            last_modification: times,
        },
        rustix::fs::AtFlags::empty(),
    )?;
    Ok(())
}

fn base_takes_noreplace(folder: &Path) -> io::Result<bool> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    let (one, other) = (folder.join("rename-probe"), folder.join("rename-probe-2"));
    fs::write(&one, b"one")?;
    let renamed = renameat_with(CWD, &one, CWD, &other, RenameFlags::NOREPLACE).is_ok();
    for probe in [&one, &other] {
        let _ = fs::remove_file(probe);
    }
    Ok(renamed)
}

fn base_takes_rename_flags(folder: &Path) -> io::Result<bool> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    if !base_takes_noreplace(folder)? {
        return Ok(false);
    }
    let (one, other) = (folder.join("rename-probe"), folder.join("rename-probe-2"));
    fs::write(&one, b"one")?;
    fs::write(&other, b"other")?;
    let exchanged = renameat_with(CWD, &one, CWD, &other, RenameFlags::EXCHANGE).is_ok();
    // exfat-fuse accepts RENAME_EXCHANGE and overwrites instead of swapping.
    let overwrote = exchanged && fs::symlink_metadata(&one).is_err();
    for probe in [&one, &other] {
        let _ = fs::remove_file(probe);
    }
    Ok(!overwrote)
}

fn hard_linked(original: &Path, link: &Path) -> io::Result<bool> {
    match fs::hard_link(original, link) {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == Some(rustix::io::Errno::PERM.raw_os_error()) => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

fn base_links_keep_their_number(folder: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let (one, other) = (folder.join("link-probe"), folder.join("link-probe-2"));
    fs::write(&one, b"one")?;
    let kept = hard_linked(&one, &other)?
        && fs::symlink_metadata(&one)?.ino() == fs::symlink_metadata(&other)?.ino();
    for probe in [&one, &other] {
        let _ = fs::remove_file(probe);
    }
    Ok(kept)
}

pub(super) fn base_places_files_by_linking(folder: &Path) -> io::Result<bool> {
    Ok(!base_takes_noreplace(folder)? && base_links_keep_their_number(folder)?)
}

fn forced_mode(path: &Path) -> io::Result<u32> {
    use rustix::fs::{AtFlags, CWD, StatxFlags};
    let stat = rustix::fs::statx(
        CWD,
        path,
        AtFlags::SYMLINK_NOFOLLOW | AtFlags::STATX_FORCE_SYNC,
        StatxFlags::TYPE | StatxFlags::MODE,
    )?;
    Ok(u32::from(stat.stx_mode))
}

fn a_later_change_time(path: &Path, than: &Path, seconds: u64) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let changed = |metadata: &fs::Metadata| (metadata.ctime(), metadata.ctime_nsec());
    let earlier = changed(&fs::symlink_metadata(than)?);
    loop {
        set_times(path, seconds, 0)?;
        let stamped = fs::symlink_metadata(path)?;
        if changed(&stamped) == (stamped.mtime(), stamped.mtime_nsec()) {
            return Ok(false);
        }
        if changed(&stamped) > earlier {
            return Ok(true);
        }
    }
}

/// Sets the times of each file beneath `root` again from outside the seam,
/// as Samba does once the SMB client's deferred close of a file it wrote
/// reaches it: only the change time moves.
fn land_deferred_closes(root: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let before = fs::symlink_metadata(&path)?;
        if before.is_dir() {
            land_deferred_closes(&path)?;
            continue;
        }
        if !before.is_file() {
            continue;
        }
        let time = |tv_sec, tv_nsec| rustix::fs::Timespec { tv_sec, tv_nsec };
        let times = rustix::fs::Timestamps {
            last_access: time(before.atime(), before.atime_nsec()),
            last_modification: time(before.mtime(), before.mtime_nsec()),
        };
        // A kernel without fine-grained times stamps within its tick.
        let deadline = Instant::now() + CHANGE_TIME_WAIT;
        loop {
            rustix::fs::utimensat(rustix::fs::CWD, &path, &times, rustix::fs::AtFlags::empty())?;
            let after = fs::symlink_metadata(&path)?;
            if (after.ctime(), after.ctime_nsec()) != (before.ctime(), before.ctime_nsec()) {
                break;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "the change time of {} did not move within {CHANGE_TIME_WAIT:?}; this mount \
                     may report the modification time as the change time",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

const CHANGE_TIME_WAIT: Duration = Duration::from_secs(2);

fn has_colon(name: &OsStr) -> Option<rustix::io::Errno> {
    name.as_encoded_bytes()
        .contains(&b':')
        .then_some(rustix::io::Errno::INVAL)
}

/// Copies `source` to the new item `target` as legacy's `copy_new_recursively_on_filesystem`
/// did for native paths, giving the target it was published under.
fn copy_new_recursively_on_filesystem(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    taken: Taken,
    cancellable: Cancellable,
    names: NamePolicy,
) -> Result<std::path::PathBuf, EngineError> {
    let target = target.as_ref().to_path_buf();
    let options = Options {
        names,
        ..Options::default()
    };
    let (copied, _) = copy_engine::copy_local(
        source.as_ref().to_path_buf(),
        Destination::New(target.clone(), taken),
        options,
        (cancellable.flag(), Arc::new(SharedProgress::default())),
    );
    copied.map(|copied| target.with_file_name(copied.name))
}

fn copy_new_recursively(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    cancellable: Cancellable,
) -> Result<(), EngineError> {
    copy_new_recursively_on_filesystem(
        source,
        target,
        Taken::Fail,
        cancellable,
        NamePolicy::default(),
    )
    .map(drop)
}

/// The copy with the counters a progress follower would read.
fn copy_new_recursively_with_progress(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    cancellable: Cancellable,
    progress: Arc<SharedProgress>,
) -> Result<(), EngineError> {
    let target = target.as_ref().to_path_buf();
    let (copied, _) = copy_engine::copy_local(
        source.as_ref().to_path_buf(),
        Destination::New(target, Taken::Fail),
        Options::default(),
        (cancellable.flag(), progress),
    );
    copied.map(drop)
}

fn reporting(
    source: &Path,
    destination: Destination,
    options: Options,
) -> (Result<(), EngineError>, crate::services::TransferReport) {
    let (copied, report) = copy_engine::copy_local(
        source.to_path_buf(),
        destination,
        options,
        (Arc::default(), Arc::new(SharedProgress::default())),
    );
    (copied.map(drop), report)
}

fn copy_reporting(
    source: &Path,
    target: &Path,
    workers: Option<usize>,
) -> Result<crate::services::TransferReport, Box<dyn Error>> {
    let destination = Destination::New(target.to_path_buf(), Taken::Fail);
    let (copied, reported) = reporting(source, destination, workers_only(workers));
    copied?;
    Ok(reported)
}

fn merge_reporting(
    source: &Path,
    target: &Path,
    workers: Option<usize>,
) -> (Result<(), EngineError>, crate::services::TransferReport) {
    reporting(
        source,
        Destination::Merge(target.to_path_buf(), None),
        workers_only(workers),
    )
}

fn workers_only(workers: Option<usize>) -> Options {
    Options {
        workers,
        ..Options::default()
    }
}

/// Legacy's `copy_local` with its progress follower, which these tests always left out.
fn copy_local_with(
    source: PathBuf,
    destination: Destination,
    options: Options,
    progress: Option<()>,
    cancellable: &Cancellable,
) -> Result<copy_engine::Copied, EngineError> {
    let _ = progress;
    copy_engine::copy_local(
        source,
        destination,
        options,
        (cancellable.flag(), Arc::new(SharedProgress::default())),
    )
    .0
}

/// A new copy, or a merge into the folder at `target` keeping both where names clash.
fn copy_recursively(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    merge: bool,
    cancellable: Cancellable,
) -> Result<(), EngineError> {
    let target = target.as_ref().to_path_buf();
    let destination = if merge {
        Destination::Merge(target, None)
    } else {
        Destination::New(target, Taken::Fail)
    };
    copy_engine::copy_local(
        source.as_ref().to_path_buf(),
        destination,
        Options::default(),
        (cancellable.flag(), Arc::new(SharedProgress::default())),
    )
    .0
    .map(drop)
}

fn copy_recursively_fixing(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    merge: bool,
    cancellable: Cancellable,
) -> Result<(), EngineError> {
    let target = target.as_ref().to_path_buf();
    let destination = if merge {
        Destination::Merge(target, None)
    } else {
        Destination::New(target, Taken::Fail)
    };
    let options = Options {
        names: fixing(),
        ..Options::default()
    };
    copy_engine::copy_local(
        source.as_ref().to_path_buf(),
        destination,
        options,
        (cancellable.flag(), Arc::new(SharedProgress::default())),
    )
    .0
    .map(drop)
}

/// A paste of one item, as legacy's provider ran it, through the engine's public copy. Returns
/// the bytes its progress counted, which legacy read from the last progress event.
fn paste_copy(source: &Path, into: &Path) -> Option<u64> {
    let progress = crate::ops::Progress::new();
    crate::ops::copy(
        source,
        into,
        crate::ops::Conflict::Fail,
        NameMode::Fix,
        &crate::ops::Cancel::new(),
        &progress,
    )
    .ok()
    .map(|copied| copied.progress.bytes)
}

/// A paste of several items, through the engine's public copy one after another as jobs run
/// them. Legacy's provider copied many small items in parallel (its bulk path); jobs do not yet.
fn paste_copies(sources: impl IntoIterator<Item = PathBuf>, into: &Path) {
    for source in sources {
        let _ = crate::ops::copy(
            &source,
            into,
            crate::ops::Conflict::Fail,
            NameMode::Fix,
            &crate::ops::Cancel::new(),
            &crate::ops::Progress::new(),
        );
    }
}

/// What each item of a paste created, from its progress events.
fn run_paste_collecting_created(
    request: PasteRequest,
) -> Result<Vec<Option<Location>>, Box<dyn Error>> {
    Ok(run_paste(request)
        .iter()
        .filter_map(|event| match event {
            OperationEvent::TransferProgress {
                created_location, ..
            } => Some(created_location.clone()),
            _ => None,
        })
        .collect())
}

fn renamed_beneath(
    target: &Path,
    report: &crate::services::TransferReport,
) -> Vec<(String, String, crate::services::RenameCause)> {
    let relative = |location: &crate::model::Location| {
        location
            .native_path()
            .and_then(|path| path.strip_prefix(target).ok())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut renamed = report
        .renamed
        .iter()
        .map(|item| {
            (
                relative(&item.requested),
                relative(&item.saved_as),
                item.cause,
            )
        })
        .collect::<Vec<_>>();
    renamed.sort_by(|left, right| left.0.cmp(&right.0));
    renamed
}

/// A paste that must succeed, as legacy's `run_paste` asserted.
fn run_paste(request: PasteRequest) -> Vec<OperationEvent> {
    let events = run_paste_events(request);
    assert!(
        matches!(events.last(), Some(OperationEvent::Pasted { .. })),
        "{events:?}"
    );
    events
}

/// Legacy's paste through its provider, run instead as a batch of the engine's jobs and told as
/// the provider's events.
fn run_paste_events(request: PasteRequest) -> Vec<OperationEvent> {
    use crate::{
        jobs::{self, Action, Event, Item, Outcome, Request},
        merge::{Clash, MergePlan},
        ops::Conflict,
    };
    let id = request.id;
    let into = request
        .destination
        .native_path()
        .expect("a native destination")
        .to_path_buf();
    let items: Vec<Item> = request
        .items
        .iter()
        .map(|item| {
            let action = match (request.move_sources, item.conflict) {
                (false, TransferConflict::FailIfExists) => Action::Copy(Conflict::Fail),
                (false, TransferConflict::KeepBoth) => Action::Copy(Conflict::KeepBoth),
                (false, TransferConflict::ReplaceExisting) => Action::Replace,
                // Legacy's paste merge let incoming items take the place of same-named ones.
                (false, TransferConflict::Merge) => Action::Merge(MergePlan {
                    otherwise: Clash::Replace,
                    ..MergePlan::default()
                }),
                (true, TransferConflict::FailIfExists) => Action::Move,
                (true, TransferConflict::KeepBoth) => Action::MoveKeepingBoth,
                (true, TransferConflict::ReplaceExisting) => Action::MoveReplacing,
                (true, TransferConflict::Merge) => Action::MergeMoving(MergePlan {
                    otherwise: Clash::Replace,
                    ..MergePlan::default()
                }),
            };
            Item {
                source: item
                    .source
                    .native_path()
                    .expect("a native source")
                    .to_path_buf(),
                action,
                name: item.target_name.as_ref().map(|shown| shown.name.clone()),
            }
        })
        .collect();
    let (send, received) = std::sync::mpsc::channel();
    jobs::start(
        Request {
            into: into.clone(),
            items: items.clone(),
            names: NameMode::Fix,
            record: None,
            durable: Some(false),
        },
        send,
    )
    .join();
    let mut events = Vec::new();
    let mut completed = Vec::new();
    let mut failure = None;
    let mut bytes = 0;
    for event in received.try_iter() {
        match event {
            Event::Progress { progress, .. } => bytes = progress.bytes,
            Event::ItemDone { index, outcome, .. } => match outcome {
                Outcome::Done { name, changes } => {
                    // Legacy left out of the paste's undo an item whose original was deleted.
                    let undoable = !changes.original_deleted;
                    let report = changes_report(&into, changes);
                    if !report.is_empty() {
                        events.push(OperationEvent::ItemsReported {
                            request_id: id,
                            report,
                        });
                    }
                    // A move is reported by where it landed, not as a created location.
                    events.push(OperationEvent::TransferProgress {
                        request_id: id,
                        completed_items: completed.len() + 1,
                        completed_files: 0,
                        total_files: None,
                        current_file: None,
                        transferred_bytes: bytes,
                        total_bytes: None,
                        created_location: (!request.move_sources && undoable)
                            .then(|| Location::local(&name)),
                    });
                    // Legacy did not report an item moved to where it already was.
                    if request.move_sources && items[index].source != name {
                        events.push(OperationEvent::ItemMoved {
                            request_id: id,
                            from: Location::local(&items[index].source),
                            to: Location::local(&name),
                            merged: false,
                        });
                    }
                    if undoable {
                        completed.push(Location::local(&name));
                    }
                }
                Outcome::Failed { error, changes } => {
                    let report = changes_report(&into, changes);
                    if !report.is_empty() {
                        events.push(OperationEvent::ItemsReported {
                            request_id: id,
                            report,
                        });
                    }
                    failure.get_or_insert(error.message().to_owned());
                }
                Outcome::NotAttempted => {}
            },
            _ => {}
        }
    }
    events.push(OperationEvent::TransferProgress {
        request_id: id,
        completed_items: completed.len(),
        completed_files: 0,
        total_files: None,
        current_file: None,
        transferred_bytes: bytes,
        total_bytes: None,
        created_location: None,
    });
    events.push(match failure {
        None => OperationEvent::Pasted {
            request_id: id,
            locations: completed,
        },
        Some(message) => OperationEvent::TransferFailed {
            request_id: id,
            completed_locations: completed,
            message,
        },
    });
    events
}

fn trashed_event(
    events: &mut Vec<OperationEvent>,
    id: OperationRequestId,
    at: &Path,
    trashed: &crate::trash::Trashed,
) {
    if let Some(identity) = super::native_fs::identity_at_path(&trashed.file) {
        events.push(OperationEvent::ItemTrashed {
            request_id: id,
            location: Location::local(at),
            identity,
        });
    }
}

/// What the public API reports, as legacy's provider reported it.
fn changes_report(into: &Path, changes: crate::ops::Changes) -> TransferReport {
    TransferReport {
        renamed: changes
            .renamed
            .into_iter()
            .map(|renamed| RenamedItem {
                requested: Location::local(into.join(renamed.requested)),
                saved_as: Location::local(into.join(renamed.saved_as)),
                cause: renamed.cause,
            })
            .collect(),
        skipped: changes
            .skipped
            .into_iter()
            .map(|skipped| SkippedItem {
                location: Location::local(into.join(skipped.path)),
                kind: skipped.kind,
            })
            .collect(),
        kept: changes
            .kept
            .into_iter()
            .map(|kept| crate::services::KeptItem {
                location: Location::local(kept.path),
                cause: kept.cause,
            })
            .collect(),
        ..TransferReport::default()
    }
}

fn followed_report(events: &[OperationEvent]) -> TransferReport {
    let mut reported = TransferReport::default();
    for event in events {
        if let OperationEvent::ItemsReported { report, .. } = event {
            reported.extend(report.clone());
        }
    }
    reported.follow_renames();
    reported
}

fn terminal_transfer(events: &[OperationEvent]) -> Option<&OperationEvent> {
    events.iter().rev().find(|event| {
        matches!(
            event,
            OperationEvent::Pasted { .. }
                | OperationEvent::TransferFailed { .. }
                | OperationEvent::Cancelled { .. }
        )
    })
}

fn item_moves(events: &[OperationEvent]) -> Vec<(Location, Location)> {
    events
        .iter()
        .filter_map(|event| match event {
            OperationEvent::ItemMoved { from, to, .. } => Some((from.clone(), to.clone())),
            _ => None,
        })
        .collect()
}

fn trashed_items(events: &[OperationEvent]) -> Vec<(Location, (u64, u64))> {
    events
        .iter()
        .filter_map(|event| match event {
            OperationEvent::ItemTrashed {
                location, identity, ..
            } => Some((location.clone(), numbers(*identity))),
            _ => None,
        })
        .collect()
}

fn numbers(identity: ItemIdentity) -> (u64, u64) {
    (identity.evidence.device, identity.evidence.inode)
}

fn numbers_at(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path).expect("identity");
    (metadata.dev(), metadata.ino())
}

/// Legacy pumped the main loop until the operation ended; a batch here has ended on return.
/// Delivers the events of every deletion started with [`LocalOperationProvider`] until each has
/// finished, as legacy's tests turned the GLib loop.
fn wait_for_operation<T: ?Sized>(_events: &T, _ended: impl Fn(&OperationEvent) -> bool) {
    pump();
}

/// Legacy's operation provider, as far as deleting and restoring from the Trash go: a deletion is
/// a batch of jobs (`Action::Delete` or `Action::Trash`), reported as legacy's `run_deletion`
/// did. Its events reach `emit` only as [`pump`] delivers them, on the test's thread, so a test can
/// cancel by dropping the handle from inside `emit`, as it could under GLib.
struct LocalOperationProvider;

struct Deleting {
    id: OperationRequestId,
    permanent: bool,
    entries: Vec<crate::model::FileEntry>,
    events: std::sync::mpsc::Receiver<crate::jobs::Event>,
    emit: Rc<dyn Fn(OperationEvent)>,
    _job: Rc<crate::jobs::Handle>,
}

thread_local! {
    static DELETING: RefCell<Vec<Deleting>> = const { RefCell::new(Vec::new()) };
    /// The flag the next deletion's job shares, set by [`run_deletion`].
    static SHARED_CANCEL: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

impl LocalOperationProvider {
    fn delete(
        &self,
        request: crate::services::DeleteRequest,
        emit: Rc<dyn Fn(OperationEvent)>,
    ) -> crate::services::LoadHandle {
        use crate::jobs::{self, Action, Item, Request};
        let path = |entry: &crate::model::FileEntry| {
            entry
                .location
                .native_path()
                .expect("a native entry")
                .to_path_buf()
        };
        let items = request
            .entries
            .iter()
            .map(|entry| Item {
                source: path(entry),
                action: if request.permanent {
                    Action::Delete
                } else {
                    Action::Trash
                },
                name: None,
            })
            .collect::<Vec<_>>();
        let into = request
            .entries
            .first()
            .and_then(|entry| path(entry).parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let (send, events) = std::sync::mpsc::channel();
        let request_items = Request {
            into,
            items,
            names: crate::ops::NameMode::Fix,
            record: None,
            durable: Some(false),
        };
        let job = Rc::new(
            match SHARED_CANCEL.with(|shared| shared.borrow_mut().take()) {
                Some(flag) => {
                    jobs::start_sharing(request_items, send, crate::ops::Cancel::sharing(flag))
                }
                None => jobs::start(request_items, send),
            },
        );
        let cancelling = job.clone();
        DELETING.with(|deleting| {
            deleting.borrow_mut().push(Deleting {
                id: request.id,
                permanent: request.permanent,
                entries: request.entries,
                events,
                emit,
                _job: job,
            });
        });
        crate::services::LoadHandle::new(move || cancelling.cancel())
    }

    fn restore(
        &self,
        request: crate::services::RestoreRequest,
        emit: Rc<dyn Fn(OperationEvent)>,
    ) -> crate::services::LoadHandle {
        let crate::services::RestoreSource::OriginalLocations { locations, .. } = request.source
        else {
            panic!("the stand-in restores by original location only");
        };
        let (mut restored, mut failed) = (Vec::new(), Vec::new());
        for location in locations {
            let original = location.native_path().map(Path::to_path_buf);
            let entry = off_ui_thread(crate::trash::list)
                .into_iter()
                .filter(|entry| Some(&entry.original) == original.as_ref())
                .max_by_key(|entry| entry.deleted.clone());
            match entry.map(|entry| off_ui_thread(|| crate::trash::restore(&entry))) {
                Some(Ok(path)) => {
                    if let Some(identity) =
                        off_ui_thread(|| super::native_fs::identity_at_path(&path))
                    {
                        emit(OperationEvent::ItemRestored {
                            request_id: request.id,
                            location: Location::local(&path),
                            identity,
                        });
                    }
                    restored.push(Location::local(&path));
                }
                _ => failed.push(location),
            }
        }
        emit(if failed.is_empty() {
            OperationEvent::Restored {
                request_id: request.id,
                locations: restored.clone(),
                restored,
            }
        } else {
            OperationEvent::RestoreCompletedWithErrors {
                request_id: request.id,
                restored_locations: restored.clone(),
                restored,
                message: "restore failed".to_owned(),
            }
        });
        crate::services::LoadHandle::new(|| {})
    }
}

/// Runs file calls off a test marked as the UI thread, as legacy's provider did on its workers.
fn off_ui_thread<T: Send>(call: impl FnOnce() -> T + Send) -> T {
    thread::scope(|scope| scope.spawn(call).join().expect("the call returns"))
}

/// Legacy's `run_deletion`: deletes or trashes `entries`, stopping when `cancellable` is set.
fn run_deletion(
    id: OperationRequestId,
    entries: Vec<crate::model::FileEntry>,
    permanent: bool,
    emit: Rc<dyn Fn(OperationEvent)>,
    cancellable: Cancellable,
) {
    // The job reads the test's flag itself, as legacy's walk read its cancellable.
    SHARED_CANCEL.with(|shared| shared.replace(Some(cancellable.flag())));
    let _operation = LocalOperationProvider.delete(
        crate::services::DeleteRequest {
            id,
            entries,
            permanent,
        },
        emit,
    );
    pump();
}

/// Delivers every pending deletion's events, as legacy's `run_deletion` emitted them, until each
/// has finished.
fn pump() {
    use crate::jobs::{Event, Outcome};
    loop {
        let Some(mut deleting) = DELETING.with(|deleting| deleting.borrow_mut().pop()) else {
            return;
        };
        let (id, permanent, total) = (deleting.id, deleting.permanent, deleting.entries.len());
        let location = |index: usize| deleting.entries[index].location.clone();
        let mut affected = std::collections::HashSet::new();
        for entry in &deleting.entries {
            affected.extend(entry.location.parent());
            if entry.kind == crate::model::EntryKind::Directory {
                affected.insert(entry.location.clone());
            }
        }
        if !permanent {
            affected.insert(Location::uri("trash:///"));
        }
        let (mut deleted, mut failed, mut cut_off) = (Vec::new(), Vec::new(), Vec::new());
        let (mut errors, mut retryable, mut completed) = (Vec::new(), Vec::new(), 0);
        let mut cancelled = false;
        let emit = deleting.emit.clone();
        while let Ok(event) = deleting.events.recv_timeout(Duration::from_secs(60)) {
            match event {
                Event::ItemDone { index, outcome, .. } => {
                    let mut progressed = Vec::new();
                    match outcome {
                        Outcome::Done { name, .. } => {
                            completed += 1;
                            if !permanent
                                && let Some(identity) =
                                    off_ui_thread(|| super::native_fs::identity_at_path(&name))
                            {
                                emit(OperationEvent::ItemTrashed {
                                    request_id: id,
                                    location: location(index),
                                    identity,
                                });
                            }
                            deleted.push(location(index));
                            progressed.push(location(index));
                        }
                        // A trash cut off left its item whole; a delete cut off part way did not.
                        Outcome::Failed { error, .. } if error.kind() == ErrorKind::Cancelled => {
                            completed += 1;
                            cancelled = true;
                            if permanent {
                                failed.push(location(index));
                            } else {
                                cut_off.push(location(index));
                            }
                        }
                        Outcome::Failed { error, .. } => {
                            completed += 1;
                            if !permanent && error.kind() == ErrorKind::NotSupported {
                                retryable.push(location(index));
                            }
                            errors.push(format!(
                                "{}: {}",
                                deleting.entries[index].display_name,
                                error.message()
                            ));
                            failed.push(location(index));
                        }
                        Outcome::NotAttempted => {
                            cancelled = true;
                            cut_off.push(location(index));
                            continue;
                        }
                    }
                    emit(OperationEvent::DeleteProgress {
                        request_id: id,
                        completed,
                        total,
                        deleted_locations: progressed,
                    });
                }
                Event::Finished { .. } => break,
                _ => {}
            }
        }
        emit(if cancelled {
            OperationEvent::Cancelled {
                request_id: id,
                result: crate::services::CancelledOperation {
                    completed: deleted,
                    failed,
                    not_attempted: cut_off,
                    affected_locations: affected,
                },
            }
        } else if errors.is_empty() {
            OperationEvent::Deleted {
                request_id: id,
                locations: deleted,
            }
        } else {
            OperationEvent::CompletedWithErrors {
                request_id: id,
                deleted_locations: deleted,
                has_non_retryable_failures: errors.len() > retryable.len(),
                retryable_locations: retryable,
                message: errors.join("\n"),
            }
        });
    }
}

/// A listing's entry for `path`, as legacy's views handed to its provider.
fn file_entry(path: &std::path::Path) -> crate::model::FileEntry {
    use crate::model::{EntryKind, FileEntry, MetadataValue};
    FileEntry {
        location: Location::local(path),
        thumbnail_path: None,
        native_name: path.file_name().unwrap_or_default().to_owned(),
        display_name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        kind: EntryKind::File,
        size: MetadataValue::Unknown,
        modified_unix_seconds: MetadataValue::Unknown,
        recent_unix_seconds: MetadataValue::Unknown,
        is_hidden: false,
        mode: MetadataValue::Unknown,
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        recent_uri: None,
    }
}

fn directory_entry(path: &std::path::Path) -> crate::model::FileEntry {
    use crate::model::{EntryKind, FileEntry, MetadataValue};
    FileEntry {
        kind: EntryKind::Directory,
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        ..file_entry(path)
    }
}

/// Legacy's GLib-driven forms of the deletion walk, run synchronously.
use super::deletion::PinnedEntry;

fn parallel_delete_local_with(
    roots: Vec<PinnedEntry>,
    cancellable: Cancellable,
    delete: impl FnOnce(Vec<PinnedEntry>, Arc<AtomicBool>) -> Result<(), String>,
) -> Result<(), EngineError> {
    if roots.is_empty() {
        return Ok(());
    }
    if cancellable.is_cancelled() {
        return Err(crate::error::cancelled());
    }
    match delete(roots, cancellable.flag()) {
        Ok(()) => Ok(()),
        Err(error) if error == "Delete cancelled" => Err(crate::error::cancelled()),
        Err(error) => Err(EngineError::new(ErrorKind::Failed, &error)),
    }
}

fn parallel_delete_local(
    roots: Vec<PinnedEntry>,
    cancellable: Cancellable,
) -> Result<(), EngineError> {
    parallel_delete_local_with(roots, cancellable, |roots, cancelled| {
        let workers = super::deletion::local_delete_worker_count(&roots);
        super::deletion::parallel_delete_local_blocking_with_workers(
            roots,
            cancelled,
            workers,
            &super::leftovers::Deadline::default(),
        )
    })
}

fn permanently_delete_local(
    parent: OwnedFd,
    name: OsString,
    expected: Option<super::native_fs::Recorded>,
    cancellable: Cancellable,
) -> Result<(), EngineError> {
    parallel_delete_local(
        vec![PinnedEntry {
            parent: Arc::new(parent),
            name,
            identity: expected,
        }],
        cancellable,
    )
}

/// What deletion pins an item by, read now; None for a path with no parent.
fn local_file_identity(path: &Path) -> Result<Option<super::native_fs::Recorded>, EngineError> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(None);
    };
    let parent =
        super::native_fs::open_local_parent_directory(parent).map_err(crate::error::io_error)?;
    let stat = super::fs_ops::fs()
        .stat(super::fs_ops::Entry::new(&parent, name), false)
        .map_err(|errno| crate::error::os_io_error(io::Error::from(errno)))?;
    Ok(Some(super::native_fs::Recorded::of(&stat)))
}

fn permanently_delete_local_path_if_unchanged(
    path: PathBuf,
    expected: Option<super::native_fs::Recorded>,
    cancellable: Cancellable,
) -> Result<(), EngineError> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(crate::error::translated_io_error("Invalid delete target"));
    };
    let parent =
        super::native_fs::open_local_parent_directory(parent).map_err(crate::error::io_error)?;
    permanently_delete_local(parent, name.to_os_string(), expected, cancellable)
}

fn drive_until_transfer_settles<T: ?Sized>(_events: &T) {}

/// The kind of error the engine reports for `errno`, as legacy compared against GIO's mapping.
fn kind_of(errno: rustix::io::Errno) -> ErrorKind {
    crate::error::os_io_error(io::Error::from_raw_os_error(errno.raw_os_error())).kind()
}

fn pump_until_transfer<T: ?Sized>(_events: &T) {}

/// Legacy's move of one item to the new path `target`, through the engine's public move.
fn move_local(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    taken: Taken,
    cancellable: Cancellable,
    progress: Option<()>,
    names: NamePolicy,
) -> Result<PathBuf, EngineError> {
    let _ = progress;
    let target = target.as_ref();
    let (into, name) = (
        target.parent().expect("a target folder"),
        target.file_name().expect("a target name"),
    );
    let cancel = crate::ops::Cancel::sharing(cancellable.flag());
    let progress = crate::ops::Progress::new();
    let moved = match taken {
        Taken::Number => crate::ops::move_keeping_both_as(
            source.as_ref(),
            (into, name),
            names.mode,
            &cancel,
            &progress,
        ),
        _ => crate::ops::move_as(
            source.as_ref(),
            (into, name),
            names.mode,
            &cancel,
            &progress,
        ),
    }?;
    Ok(into.join(moved.name))
}

/// Legacy's copying options for a replace: its progress follower, which these tests leave out,
/// and its name rules.
#[derive(Clone, Default)]
struct Copying {
    progress: Option<()>,
    names: NamePolicy,
}

/// Legacy's replace of the item at `target` with `source`, copied or moved, through the engine's
/// public replace. `affected` was the folders a view had to read again; listings follow their
/// folders now, so it is left as given.
fn replace_local(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    move_source: bool,
    cancellable: Cancellable,
    affected: Option<&mut HashSet<Location>>,
) -> Result<(), EngineError> {
    replace_local_with(
        source,
        target,
        move_source,
        cancellable,
        affected,
        Copying::default(),
        &|_, _| {},
    )
}

fn replace_local_with(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    move_source: bool,
    cancellable: Cancellable,
    _affected: Option<&mut HashSet<Location>>,
    copying: Copying,
    on_replaced: &dyn Fn(
        crate::services::ReplacedOriginal,
        Option<crate::services::PlacedSnapshot>,
    ),
) -> Result<(), EngineError> {
    use crate::services::ReplacedOriginal;
    let target = target.as_ref();
    let (into, name) = (
        target.parent().expect("a target folder"),
        target.file_name().expect("a target name"),
    );
    let cancel = crate::ops::Cancel::sharing(cancellable.flag());
    let progress = crate::ops::Progress::new();
    let mode = copying.names.mode;
    let replaced = if move_source {
        crate::ops::move_replacing_as(source.as_ref(), (into, name), mode, &cancel, &progress)
    } else {
        crate::ops::replace_as(source.as_ref(), (into, name), mode, &cancel, &progress)
    }?;
    if replaced.original_deleted {
        on_replaced(ReplacedOriginal::Deleted, None);
    } else if let Some(trashed) = &replaced.original {
        on_replaced(
            ReplacedOriginal::Trashed(super::native_fs::identity_at_path(&trashed.file)),
            None,
        );
    }
    Ok(())
}

/// What legacy's merge reported writing: the topmost paths it created, and those whose originals
/// it replaced.
#[derive(Clone, Debug, Default)]
struct MergeWritten {
    created: Vec<Location>,
    overwritten: Vec<Location>,
}

/// Legacy's merge of the folder `source` into the existing folder `target`, incoming items taking
/// the place of same-named ones, through the engine's merge.
fn merge_local(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    move_source: bool,
    cancellable: Cancellable,
    on_merged: &dyn Fn(MergeWritten),
) -> Result<(), EngineError> {
    use crate::merge::{self, Clash, Did, MergePlan};
    let target = target.as_ref();
    let (into, name) = (
        target.parent().expect("a target folder"),
        target.file_name().expect("a target name"),
    );
    let plan = MergePlan {
        otherwise: Clash::Replace,
        ..MergePlan::default()
    };
    let cancel = crate::ops::Cancel::sharing(cancellable.flag());
    let progress = crate::ops::Progress::new();
    let merged = if move_source {
        merge::merge_moving_as(
            source.as_ref(),
            (into, name),
            &plan,
            NameMode::Fix,
            &cancel,
            &progress,
        )
    } else {
        merge::merge_as(
            source.as_ref(),
            (into, name),
            &plan,
            NameMode::Fix,
            &cancel,
            &progress,
        )
    }?;
    let mut written = MergeWritten::default();
    for did in &merged.done {
        match did {
            Did::Copied { copy: at, .. } | Did::Moved { to: at, .. } => {
                written.created.push(Location::local(at));
            }
            Did::Replaced { target: at, .. } | Did::MovedOver { to: at, .. } => {
                written.overwritten.push(Location::local(at));
            }
            Did::RemovedFolder { .. } => {}
        }
    }
    on_merged(written);
    match merged.failed.into_iter().next() {
        Some((_, error)) => Err(error),
        None => Ok(()),
    }
}
