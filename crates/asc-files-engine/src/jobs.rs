// SPDX-License-Identifier: MIT
//! Batches of operations on a worker thread, reported as events.
//!
//! Every item in a batch gets exactly one [`Event::ItemDone`]: a failure ends that item only.
//! Cancelling stops the item in flight; items not yet started are reported as not attempted.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Sender,
    },
    thread,
    time::Duration,
};

use crate::{
    error::Error,
    ops::{self, Cancel, Changes, Conflict, NameMode, Progress, ProgressSnapshot},
    undo::{Journal, Step},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JobId(pub u64);

/// What to do with one item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Copy(Conflict),
    Move,
    /// Copy, replacing what holds the name; the original goes to the Trash.
    Replace,
    Trash,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub source: PathBuf,
    pub action: Action,
}

/// A batch: every item lands in `into` (ignored by [`Action::Trash`]).
#[derive(Clone, Debug)]
pub struct Request {
    pub into: PathBuf,
    pub items: Vec<Item>,
    /// What to do with a name the destination drive cannot store; see [`ops::copy`].
    pub names: NameMode,
    /// Where to file the batch for undo, and what to call it there.
    pub record: Option<Record>,
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
    Done {
        name: PathBuf,
        changes: Changes,
    },
    Failed(Error),
    /// The batch was cancelled before this item started.
    NotAttempted,
}

#[derive(Debug)]
pub enum Event {
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
    let id = JobId(NEXT_JOB.fetch_add(1, Ordering::Relaxed));
    let cancel = Cancel::new();
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
    send(Event::Started {
        job,
        total: request.items.len(),
    });
    let (mut done, mut failed, mut not_attempted) = (0, 0, 0);
    let mut steps = Vec::new();
    for (index, item) in request.items.iter().enumerate() {
        let (outcome, step) = if cancel.is_cancelled() {
            (Outcome::NotAttempted, None)
        } else {
            run_item(
                job,
                index,
                item,
                (&request.into, request.names),
                cancel,
                &send,
            )
        };
        steps.extend(step);
        match outcome {
            Outcome::Done { .. } => done += 1,
            Outcome::Failed(_) => failed += 1,
            Outcome::NotAttempted => not_attempted += 1,
        }
        send(Event::ItemDone {
            job,
            index,
            outcome,
        });
    }
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
    });
}

fn run_item(
    job: JobId,
    index: usize,
    item: &Item,
    (into, names): (&std::path::Path, NameMode),
    cancel: &Cancel,
    send: &(impl Fn(Event) + Sync),
) -> (Outcome, Option<Step>) {
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
        let result = match item.action {
            Action::Copy(conflict) => {
                let merged_into_existing = conflict == Conflict::Merge
                    && source
                        .file_name()
                        .is_some_and(|name| std::fs::symlink_metadata(into.join(name)).is_ok());
                ops::copy(source, into, conflict, names, cancel, &progress).map(|copied| {
                    let step =
                        Step::copied(source, (into, &copied.name), conflict, merged_into_existing);
                    (into.join(copied.name), copied.changes, step)
                })
            }
            Action::Move => ops::move_item(source, into, names, cancel, &progress).map(|moved| {
                let step = Step::moved(source, (into, &moved));
                (into.join(moved.name), moved.changes, step)
            }),
            Action::Replace => {
                ops::replace(source, into, names, cancel, &progress).map(|replaced| {
                    let step = Step::replaced(source, (into, &replaced));
                    (into.join(replaced.name), replaced.changes, step)
                })
            }
            Action::Trash => ops::trash(source).map(|trashed| {
                let step = Step::trashed(source, &trashed);
                (trashed.file, Changes::default(), step)
            }),
        };
        running.store(false, Ordering::Release);
        result
    });
    match result {
        Ok((name, changes, step)) => (Outcome::Done { name, changes }, step),
        Err(error) => (Outcome::Failed(error), None),
    }
}
