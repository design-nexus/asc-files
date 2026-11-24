// SPDX-License-Identifier: MIT
//! Batches of operations on a worker thread, reported as events.
//!
//! Every item in a batch gets exactly one [`Event::ItemDone`]: a failure ends that item only.
//! Cancelling stops the item in flight; items not yet started are reported as not attempted.
//!
//! Batches writing to the same drive run one after another, in the order they were started, so
//! two copies onto one disk do not fight over it; batches for different drives run side by side.
//! A batch written to a removable drive is synced before it finishes, so its end means the drive
//! can be unplugged.

use std::{
    collections::{HashMap, VecDeque},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Sender,
    },
    thread,
    time::Duration,
};

use crate::{
    adapters::local_operations::case_variants::{self, CaseVariants, Saved},
    drives,
    error::Error,
    merge::{self, MergePlan},
    ops::{self, Cancel, Changes, Conflict, NameMode, Progress, ProgressSnapshot},
    undo::{Journal, Step},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JobId(pub u64);

/// What to do with one item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Copy(Conflict),
    /// Merge a folder into the one of its name, with a choice for each clash; see [`merge`].
    Merge(MergePlan),
    Move,
    /// Move, taking the next free "name (n)" where the name is taken.
    MoveKeepingBoth,
    /// Move, replacing what holds the name; the original goes to the Trash.
    MoveReplacing,
    /// Merge a folder into the one of its name by moving its contents; see [`merge::merge_moving`].
    MergeMoving(MergePlan),
    /// Copy, replacing what holds the name; the original goes to the Trash.
    Replace,
    Trash,
    /// Delete for good, without the Trash; see [`ops::delete`]. Not filed for undo.
    Delete,
}

impl Action {
    /// Whether the item only goes away, writing nothing into the batch's folder.
    fn removes(&self) -> bool {
        matches!(self, Self::Trash | Self::Delete)
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub source: PathBuf,
    pub action: Action,
    /// The name to give the item in the destination instead of its own, as a conflict prompt
    /// offered it. Used by copies and moves.
    pub name: Option<OsString>,
}

impl Item {
    pub fn new(source: impl Into<PathBuf>, action: Action) -> Self {
        Self {
            source: source.into(),
            action,
            name: None,
        }
    }
}

/// A batch: every item lands in `into` (ignored by [`Action::Trash`] and [`Action::Delete`]).
#[derive(Clone, Debug)]
pub struct Request {
    pub into: PathBuf,
    pub items: Vec<Item>,
    /// What to do with a name the destination drive cannot store; see [`ops::copy`].
    pub names: NameMode,
    /// Where to file the batch for undo, and what to call it there.
    pub record: Option<Record>,
    /// Sync the destination's drive before finishing: None decides by whether it is removable.
    pub durable: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct Record {
    pub journal: Journal,
    /// What the user saw the operation called, such as "Copy 3 items".
    pub label: String,
}

/// How one item ended.
#[derive(Debug)]
pub enum Outcome {
    /// Done; `name` is what the item is called in the destination, or in the Trash. `changes`
    /// lists what was written under another name or left out.
    Done { name: PathBuf, changes: Changes },
    /// It stopped with `error`; `changes` is what it did before that, such as source entries a
    /// move had to leave in place.
    Failed { error: Error, changes: Changes },
    /// The batch was cancelled before this item started.
    NotAttempted,
}

#[derive(Debug)]
pub enum Event {
    /// The batch waits for `ahead` batches writing to the same drive.
    Queued {
        job: JobId,
        ahead: usize,
    },
    Started {
        job: JobId,
        total: usize,
    },
    Progress {
        job: JobId,
        item: usize,
        progress: ProgressSnapshot,
    },
    ItemDone {
        job: JobId,
        index: usize,
        outcome: Outcome,
    },
    Finished {
        job: JobId,
        done: usize,
        failed: usize,
        not_attempted: usize,
        cancelled: bool,
        /// The undo entry the batch was filed as: None when nothing it did can be undone (or
        /// nothing was asked to be recorded), `Err` when the history could not be written.
        undo: Result<Option<u64>, Error>,
        /// For a durable batch, whether its drive confirmed everything was written; None when
        /// the batch was not durable.
        settled: Option<Result<(), Error>>,
    },
}

/// A running batch.
pub struct Handle {
    pub id: JobId,
    cancel: Cancel,
    thread: Option<thread::JoinHandle<()>>,
}

impl Handle {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Waits until the batch has sent [`Event::Finished`].
    pub fn join(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// How often a running item's progress is sampled; a sample equal to the last is not sent.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Starts `request` on its own worker thread, sending its events to `events`.
pub fn start(request: Request, events: Sender<Event>) -> Handle {
    start_with(request, events, Cancel::new())
}

/// [`start`], stopped by `cancel` as well as by the handle, as a test's fault hook stops it.
#[cfg(test)]
pub(crate) fn start_sharing(request: Request, events: Sender<Event>, cancel: Cancel) -> Handle {
    start_with(request, events, cancel)
}

fn start_with(request: Request, events: Sender<Event>, cancel: Cancel) -> Handle {
    let id = JobId(NEXT_JOB.fetch_add(1, Ordering::Relaxed));
    let worker_cancel = cancel.clone();
    let thread = thread::Builder::new()
        .name(format!("asc-files-job-{}", id.0))
        .spawn(move || run(id, request, &worker_cancel, &events))
        .expect("a job thread");
    Handle {
        id,
        cancel,
        thread: Some(thread),
    }
}

fn run(job: JobId, request: Request, cancel: &Cancel, events: &Sender<Event>) {
    let send = |event| {
        // A caller that stopped listening still gets the work done; it just hears nothing.
        let _ = events.send(event);
    };
    let _turn = lanes().wait_turn(lane_of(&request), job, cancel, |ahead| {
        send(Event::Queued { job, ahead });
    });
    send(Event::Started {
        job,
        total: request.items.len(),
    });
    let writes = request.items.iter().any(|item| !item.action.removes());
    let durable = writes
        && request
            .durable
            .unwrap_or_else(|| drives::is_removable(&request.into));
    // Opened before anything is written, so reaching it covers the whole batch.
    let settle = (durable && !cancel.is_cancelled()).then(|| ops::Settle::open(&request.into));
    let (mut done, mut failed, mut not_attempted) = (0, 0, 0);
    let mut steps = Vec::new();
    // Items whose names differ only in case: a later one a case-insensitive destination takes for
    // an earlier one is numbered, as no one chose a conflict for it.
    let mut variants = CaseVariants::new(request.items.iter().enumerate().filter_map(
        |(index, item)| {
            let named = matches!(item.action, Action::Copy(Conflict::Fail) | Action::Move);
            let name = item
                .name
                .clone()
                .or_else(|| Some(item.source.file_name()?.to_os_string()));
            name.filter(|_| named).map(|name| (index, name))
        },
    ));
    let folds = case_variants::folds(&request.into);
    // One wait for files still open elsewhere covers the whole batch, as legacy's did.
    let deadline = crate::adapters::local_operations::leftovers::Deadline::default();
    for (index, item) in request.items.iter().enumerate() {
        let (outcome, step) = if cancel.is_cancelled() {
            (Outcome::NotAttempted, Vec::new())
        } else {
            run_item(
                job,
                index,
                item,
                (
                    &request.into,
                    request.names,
                    (&variants.earlier(index), folds),
                ),
                (cancel, &deadline),
                &send,
            )
        };
        if let Outcome::Done { name, .. } = &outcome {
            variants.saved(index, name);
        }
        steps.extend(step);
        match outcome {
            Outcome::Done { .. } => done += 1,
            Outcome::Failed { .. } => failed += 1,
            Outcome::NotAttempted => not_attempted += 1,
        }
        send(Event::ItemDone {
            job,
            index,
            outcome,
        });
    }
    let settled = settle.map(|settle| settle.and_then(|settle| settle.reach()));
    // Filed even when cancelled or partly failed: what was done can be undone.
    let undo = match &request.record {
        Some(record) => record.journal.record(&record.label, steps),
        None => Ok(None),
    };
    send(Event::Finished {
        job,
        done,
        failed,
        not_attempted,
        cancelled: cancel.is_cancelled(),
        undo,
        settled,
    });
}

/// The drive a batch writes to, or for a batch that only trashes or deletes, the drive of what it
/// removes.
fn lane_of(request: &Request) -> u64 {
    let only_removes = request.items.iter().all(|item| item.action.removes());
    let path = match request.items.first() {
        Some(item) if only_removes => item.source.as_path(),
        _ => request.into.as_path(),
    };
    drives::device(path).unwrap_or(0)
}

/// The batches waiting for each drive, the one at the front running.
#[derive(Default)]
struct Lanes {
    waiting: Mutex<HashMap<u64, VecDeque<JobId>>>,
    turn: Condvar,
}

fn lanes() -> &'static Lanes {
    static LANES: OnceLock<Lanes> = OnceLock::new();
    LANES.get_or_init(Lanes::default)
}

/// A batch's place at the front of its lane, given up when dropped.
struct Turn {
    lane: u64,
    job: JobId,
}

impl Lanes {
    /// Waits until `job` is at the front of `lane`, telling `queued` how many are ahead if it
    /// has to wait. A batch cancelled while waiting goes at once, to report its items as not
    /// attempted.
    fn wait_turn(&self, lane: u64, job: JobId, cancel: &Cancel, queued: impl Fn(usize)) -> Turn {
        let mut waiting = self
            .waiting
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let queue = waiting.entry(lane).or_default();
        queue.push_back(job);
        let ahead = queue.len() - 1;
        if ahead > 0 {
            queued(ahead);
        }
        while waiting.get(&lane).and_then(VecDeque::front) != Some(&job) && !cancel.is_cancelled() {
            waiting = self
                .turn
                .wait_timeout(waiting, Duration::from_millis(200))
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        Turn { lane, job }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let lanes = lanes();
        let mut waiting = lanes
            .waiting
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(queue) = waiting.get_mut(&self.lane) {
            queue.retain(|job| *job != self.job);
            if queue.is_empty() {
                waiting.remove(&self.lane);
            }
        }
        lanes.turn.notify_all();
    }
}

fn run_item(
    job: JobId,
    index: usize,
    item: &Item,
    (into, names, earlier): (&std::path::Path, NameMode, (&[Saved], bool)),
    (cancel, deadline): (
        &Cancel,
        &crate::adapters::local_operations::leftovers::Deadline,
    ),
    send: &(impl Fn(Event) + Sync),
) -> (Outcome, Vec<Step>) {
    let progress = Progress::new();
    let running = Arc::new(AtomicBool::new(true));
    let result = thread::scope(|scope| {
        let (progress_ref, running_ref) = (&progress, &running);
        scope.spawn(move || {
            let mut last = ProgressSnapshot::default();
            while running_ref.load(Ordering::Acquire) {
                thread::sleep(PROGRESS_EVERY);
                let now = progress_ref.snapshot();
                if now != last {
                    send(Event::Progress {
                        job,
                        item: index,
                        progress: now,
                    });
                    last = now;
                }
            }
        });
        let source = item.source.as_path();
        let plain = |error| (error, Changes::default(), Vec::new());
        let result = match &item.action {
            // A plain merge keeps both where names clash; it goes through the merge planner so
            // what it makes inside the folder can be undone.
            Action::Copy(Conflict::Merge) => merged(
                (source, item.name.as_deref()),
                into,
                &MergePlan::default(),
                (names, false, cancel, &progress),
            ),
            Action::Merge(plan) => merged(
                (source, item.name.as_deref()),
                into,
                plan,
                (names, false, cancel, &progress),
            ),
            Action::MergeMoving(plan) => merged(
                (source, item.name.as_deref()),
                into,
                plan,
                (names, true, cancel, &progress),
            ),
            Action::MoveReplacing => match &item.name {
                Some(name) => {
                    ops::move_replacing_as(source, (into, name), names, cancel, &progress)
                }
                None => ops::move_replacing(source, into, names, cancel, &progress),
            }
            .map(|replaced| {
                let step = Step::moved_over(source, (into, &replaced));
                (
                    into.join(replaced.name),
                    Changes {
                        original_deleted: replaced.original_deleted,
                        ..replaced.changes
                    },
                    Vec::from_iter(step),
                )
            })
            .map_err(plain),
            &Action::Copy(conflict) => {
                let copy = |conflict| match &item.name {
                    Some(name) => {
                        ops::copy_as(source, (into, name), conflict, names, cancel, &progress)
                    }
                    None => ops::copy(source, into, conflict, names, cancel, &progress),
                };
                let copied = if aliased_by_an_earlier(item, into, earlier) {
                    copy(Conflict::KeepBoth).map(|copied| aliased(source, copied))
                } else {
                    copy(conflict)
                };
                copied
                    .map(|copied| {
                        let step = Step::copied(source, (into, &copied.name), conflict, false);
                        (into.join(copied.name), copied.changes, Vec::from_iter(step))
                    })
                    .map_err(plain)
            }
            Action::Move | Action::MoveKeepingBoth => {
                let keeping_both = |item: &Item| {
                    ops::move_keeping_both(&item.source, into, names, cancel, &progress)
                };
                let moved = match (&item.action, &item.name) {
                    (Action::MoveKeepingBoth, Some(name)) => {
                        ops::move_keeping_both_as(source, (into, name), names, cancel, &progress)
                    }
                    (_, Some(name)) => ops::move_as(source, (into, name), names, cancel, &progress),
                    (Action::MoveKeepingBoth, None) => keeping_both(item),
                    _ if aliased_by_an_earlier(item, into, earlier) => {
                        ops::move_numbered(source, into, names, cancel, &progress).map(|moved| {
                            ops::Moved {
                                changes: aliased_changes(source, &moved.name, moved.changes),
                                ..moved
                            }
                        })
                    }
                    _ => ops::move_item(source, into, names, cancel, &progress),
                };
                match moved {
                    // The copy is in place but deleting the sources stopped: nothing is filed,
                    // as an undo would meet what is left of them.
                    Ok(ops::Moved {
                        stopped: Some(error),
                        changes,
                        ..
                    }) => Err((error, changes, Vec::new())),
                    moved => moved
                        .map(|moved| {
                            let step = Step::moved(source, (into, &moved));
                            (into.join(moved.name), moved.changes, Vec::from_iter(step))
                        })
                        .map_err(plain),
                }
            }
            Action::Replace => match &item.name {
                Some(name) => ops::replace_as(source, (into, name), names, cancel, &progress),
                None => ops::replace(source, into, names, cancel, &progress),
            }
            .map(|replaced| {
                let step = Step::replaced(source, (into, &replaced));
                (
                    into.join(replaced.name),
                    Changes {
                        original_deleted: replaced.original_deleted,
                        ..replaced.changes
                    },
                    Vec::from_iter(step),
                )
            })
            .map_err(plain),
            Action::Trash => crate::trash::put_within(source, deadline)
                .map(|trashed| {
                    let step = Step::trashed(source, &trashed);
                    (trashed.file, Changes::default(), Vec::from_iter(step))
                })
                .map_err(plain),
            Action::Delete => ops::delete_within(source, cancel, deadline)
                .map(|()| (source.to_path_buf(), Changes::default(), Vec::new()))
                .map_err(plain),
        };
        running.store(false, Ordering::Release);
        result
    });
    match result {
        Ok((name, changes, steps)) => (Outcome::Done { name, changes }, steps),
        // A merge that failed part way still files what it did.
        Err((error, changes, steps)) => (Outcome::Failed { error, changes }, steps),
    }
}

/// A merge as one item: done when every incoming item was, failed with the first failure
/// otherwise; either way its steps are what it made and replaced.
#[allow(clippy::type_complexity)]
fn merged(
    (source, named): (&Path, Option<&std::ffi::OsStr>),
    into: &Path,
    plan: &MergePlan,
    (names, moving, cancel, progress): (NameMode, bool, &Cancel, &Progress),
) -> Result<(PathBuf, Changes, Vec<Step>), (Error, Changes, Vec<Step>)> {
    let name = named.or_else(|| source.file_name()).unwrap_or_default();
    let merged = if moving {
        merge::merge_moving_as(source, (into, name), plan, names, cancel, progress)
    } else {
        merge::merge_as(source, (into, name), plan, names, cancel, progress)
    }
    .map_err(|error| (error, Changes::default(), Vec::new()))?;
    let steps = Step::merged(&merged);
    let changes = Changes {
        renamed: merged.renamed,
        skipped: merged.skipped,
        kept: merged.kept,
        original_deleted: false,
    };
    if let Some((_, first)) = merged.failed.first() {
        let error = match merged.failed.len() - 1 {
            0 => first.clone(),
            more => first.clone().with_more(more),
        };
        return Err((error, changes, steps));
    }
    Ok((into.join(&merged.name), changes, steps))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    /// Lanes no other test uses.
    const LANE: u64 = u64::MAX - 1;
    const OTHER_LANE: u64 = u64::MAX - 2;

    #[test]
    fn a_batch_for_a_busy_drive_waits_its_turn_and_another_drive_does_not() {
        let first = lanes().wait_turn(LANE, JobId(u64::MAX), &Cancel::new(), |_| {
            panic!("the first in its lane does not wait")
        });
        let (told, ahead) = mpsc::channel();
        let (started, running) = mpsc::channel();
        let second = thread::spawn(move || {
            let _turn = lanes().wait_turn(LANE, JobId(u64::MAX - 1), &Cancel::new(), |n| {
                told.send(n).unwrap();
            });
            started.send(()).unwrap();
        });
        assert_eq!(ahead.recv_timeout(Duration::from_secs(5)), Ok(1));
        assert!(
            running.recv_timeout(Duration::from_millis(400)).is_err(),
            "the second waits while the first runs"
        );
        let _elsewhere = lanes().wait_turn(OTHER_LANE, JobId(u64::MAX - 2), &Cancel::new(), |_| {
            panic!("another drive's lane is free")
        });

        drop(first);
        assert_eq!(running.recv_timeout(Duration::from_secs(5)), Ok(()));
        second.join().unwrap();
    }

    #[test]
    fn a_batch_cancelled_while_it_waits_stops_waiting() {
        const LANE: u64 = u64::MAX - 3;
        let _first = lanes().wait_turn(LANE, JobId(u64::MAX - 3), &Cancel::new(), |_| {});
        let cancel = Cancel::new();
        let waiting_cancel = cancel.clone();
        let (done, finished) = mpsc::channel();
        thread::spawn(move || {
            let _turn = lanes().wait_turn(LANE, JobId(u64::MAX - 4), &waiting_cancel, |_| {});
            done.send(()).unwrap();
        });
        thread::sleep(Duration::from_millis(100));
        cancel.cancel();
        assert_eq!(finished.recv_timeout(Duration::from_secs(5)), Ok(()));
    }
}

/// Whether the name an item asks for in `into` is held by an item of this batch saved earlier
/// under another spelling. Where the destination's rules fold case that is known; elsewhere, as
/// in a casefolded folder, only looking the name up shows it.
fn aliased_by_an_earlier(item: &Item, into: &Path, (earlier, folds): (&[Saved], bool)) -> bool {
    item.name.is_none()
        && !earlier.is_empty()
        && (folds
            || item
                .source
                .file_name()
                .is_some_and(|name| case_variants::taken_by(&into.join(name), earlier)))
}

fn aliased(source: &Path, copied: ops::Copied) -> ops::Copied {
    ops::Copied {
        changes: aliased_changes(source, &copied.name, copied.changes),
        ..copied
    }
}

/// Reports the numbered name an aliased item was saved under, before what the copy reported.
fn aliased_changes(source: &Path, saved: &std::ffi::OsStr, changes: Changes) -> Changes {
    let mut renamed = vec![ops::Renamed {
        requested: PathBuf::from(source.file_name().unwrap_or_default()),
        saved_as: PathBuf::from(saved),
        cause: ops::RenameCause::Alias,
    }];
    renamed.extend(changes.renamed);
    Changes { renamed, ..changes }
}
