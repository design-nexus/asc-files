// SPDX-License-Identifier: MIT
//! One client's session: carries out each [`Call`] and sends what it answers and reports.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Sender},
    thread,
    time::Duration,
};

use asc_files_engine::{
    error::{Error, ErrorKind},
    jobs::{self, Action, Event, Item, JobId, Outcome, Record},
    merge,
    ops::{self, Cancel, Progress, RenameCause, SpecialKind},
    recovery,
    services::{Holder, KeptCause, LeftoverInside, ReviewedLeftover},
    trash,
    undo::{self, Journal, Step},
};
use serde_json::{Value, json};

use crate::{
    PROTOCOL_VERSION,
    call::{Call, Id, Incoming},
    listing,
    wire::{self, Fault},
};

pub struct Session {
    out: Sender<Value>,
    initialized: bool,
    shutting_down: bool,
    listings: listing::Listings,
    jobs: HashMap<u64, jobs::Handle>,
    forwarders: Vec<thread::JoinHandle<()>>,
    /// The session's undo history, shared with every other Files process; None without one.
    journal: Option<Journal>,
    /// Calls answered off the loop (undo, clashes, renames, …); waited for at shutdown.
    workers: Vec<thread::JoinHandle<()>>,
    /// Searches by id, each with its cancel; a search ended stays until shutdown.
    searches: HashMap<u64, Cancel>,
    next_search: u64,
    /// Folder measurements by id, as searches.
    measures: HashMap<u64, Cancel>,
    next_measure: u64,
    /// Custom actions: the folder, and the queue that runs them.
    action_folder: crate::actions::Folder,
    action_queue: crate::actions::Queue,
}

impl Session {
    pub fn new(out: Sender<Value>) -> Self {
        let action_folder = crate::actions::Folder::new();
        let action_queue = crate::actions::Queue::start(out.clone(), action_folder.store());
        Self {
            action_queue,
            listings: listing::Listings::new(out.clone()),
            out,
            initialized: false,
            shutting_down: false,
            jobs: HashMap::new(),
            forwarders: Vec::new(),
            journal: Journal::session()
                .inspect_err(|error| {
                    eprintln!("asc-files-backend: no undo history: {}", error.message());
                })
                .ok(),
            workers: Vec::new(),
            searches: HashMap::new(),
            next_search: 0,
            measures: HashMap::new(),
            next_measure: 0,
            action_folder,
        }
    }

    pub fn shutting_down(&self) -> bool {
        self.shutting_down
    }

    /// Carries out one parsed line and sends its reply, unless it was a notification or will be
    /// answered from another thread.
    pub fn handle(&mut self, incoming: Incoming) {
        let (id, notification) = match incoming.id {
            Id::Request(id) => (id, false),
            Id::Notification => (Value::Null, true),
            Id::Unknown => (Value::Null, false),
        };
        let result = incoming.call.and_then(|call| self.call(id.clone(), call));
        match result {
            Ok(Answer::Later) => {}
            _ if notification => {}
            Ok(Answer::Now(result)) => self.send(wire::result(id, result)),
            Err(fault) => self.send(fault.reply(id)),
        }
    }

    fn send(&self, message: Value) {
        let _ = self.out.send(message);
    }

    /// Answers `id` now, before work started after it sends notifications naming what the
    /// answer gives (a search's or a measurement's id): a client knows the id first.
    fn answer_first(&self, id: &Value, result: Value) -> Result<Answer, Fault> {
        if id.is_null() {
            return Ok(Answer::Now(result));
        }
        self.send(wire::result(id.clone(), result));
        Ok(Answer::Later)
    }

    /// Runs `work` on a thread of its own, which answers `id`.
    fn answer_later(
        &mut self,
        id: Value,
        work: impl FnOnce() -> Result<Value, Fault> + Send + 'static,
    ) {
        let out = self.out.clone();
        self.workers.push(thread::spawn(move || {
            let _ = out.send(wire::reply(id, work()));
        }));
    }

    fn call(&mut self, id: Value, call: Call) -> Result<Answer, Fault> {
        if !self.initialized && !matches!(call, Call::Initialize { .. }) {
            return Err(Fault::invalid_request("initialize first"));
        }
        let now = |value: Value| Ok(Answer::Now(value));
        match call {
            Call::Initialize { protocol_version } => {
                if let Some(asked) = protocol_version.filter(|asked| *asked != PROTOCOL_VERSION) {
                    let mut fault = Fault::invalid_request(format!(
                        "protocol version {asked} is not supported"
                    ));
                    fault.data = Some(json!({"supported": [PROTOCOL_VERSION]}));
                    return Err(fault);
                }
                self.initialized = true;
                now(json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "server": "asc-files-backend",
                    "capabilities": capabilities(self.journal.is_some()),
                }))
            }
            Call::Shutdown => {
                self.shutting_down = true;
                now(Value::Null)
            }
            Call::ListOpen {
                path,
                options,
                filter,
            } => {
                self.listings.open(id, path, &options, filter);
                Ok(Answer::Later)
            }
            Call::ListWindow {
                listing,
                start,
                count,
            } => later(self.listings.window(id, listing, start, count)),
            Call::ListReorder {
                listing,
                options,
                filter,
            } => later(self.listings.reorder(id, listing, options, filter)),
            Call::ListRefresh { listing } => later(self.listings.refresh(id, listing)),
            Call::ListClose { listing } => {
                self.listings.close(listing);
                now(Value::Null)
            }
            Call::JobsStart { mut request, label } => {
                request.record = self.journal.clone().map(|journal| Record {
                    journal,
                    label: label.unwrap_or_else(|| default_label(&request.items)),
                });
                let (tx, rx) = mpsc::channel();
                let handle = jobs::start(request, tx);
                let job = handle.id.0;
                let out = self.out.clone();
                self.forwarders.push(thread::spawn(move || {
                    for event in rx {
                        if out.send(notification(event)).is_err() {
                            break;
                        }
                    }
                }));
                self.jobs.insert(job, handle);
                now(json!({"job": job}))
            }
            Call::JobsCancel { job } => {
                if let Some(handle) = self.jobs.get(&job) {
                    handle.cancel();
                }
                now(Value::Null)
            }
            // Walks folders, which on a slow drive takes a while.
            Call::JobsClashes { into, sources } => {
                self.answer_later(id, move || {
                    let items: Vec<Value> = sources
                        .iter()
                        .map(|source| clashes_json(source, &into))
                        .collect();
                    Ok(json!({"items": items}))
                });
                Ok(Answer::Later)
            }
            // Looks at the destination, which on a slow drive takes a while.
            Call::JobsPlan { into, sources } => {
                self.answer_later(id, move || {
                    let side = |side: &merge::Side| json!({"isFolder": side.is_folder, "size": side.size, "modified": side.modified});
                    let items: Vec<Value> = merge::plan(&sources, &into)
                        .iter()
                        .zip(&sources)
                        .map(|(planned, source)| match planned {
                            Ok(planned) => json!({
                                "source": wire::path(&planned.source),
                                "name": wire::name(&planned.name),
                                "renamed": planned.renamed,
                                "incoming": side(&planned.incoming),
                                "existing": planned.existing.as_ref().map(side),
                                "same": planned.same,
                                "aliasOf": planned.alias_of,
                            }),
                            Err(error) => json!({"source": wire::path(source), "error": wire::error(error)}),
                        })
                        .collect();
                    Ok(json!({"items": items}))
                });
                Ok(Answer::Later)
            }
            // A search walks on a thread of its own, sending what it finds as it goes.
            Call::SearchStart {
                roots,
                query,
                hidden,
                limit,
                entries,
                seconds,
                exclude,
            } => {
                self.next_search += 1;
                let search = self.next_search;
                let cancel = Cancel::new();
                self.searches.insert(search, cancel.clone());
                let out = self.out.clone();
                let limits = asc_files_engine::search::Limits {
                    results: limit.map_or(100, |limit| {
                        usize::try_from(limit)
                            .unwrap_or(usize::MAX)
                            .clamp(1, 10_000)
                    }),
                    // 0: no limit.
                    entries: entries.map_or(200_000, |entries| match entries {
                        0 => usize::MAX,
                        entries => usize::try_from(entries).unwrap_or(usize::MAX),
                    }),
                    time: seconds.map_or(Duration::from_secs(10), |seconds| match seconds {
                        0 => Duration::MAX,
                        seconds => Duration::from_secs(seconds),
                    }),
                    exclusions: asc_files_engine::search::Exclusions::new(&exclude, &home()),
                    ..Default::default()
                };
                let answer = self.answer_first(&id, json!({"search": search}));
                self.workers.push(thread::spawn(move || {
                    let summary = asc_files_engine::search::search(&roots, &query, hidden, limits, &cancel, |batch| {
                        let _ = out.send(wire::notification(
                            "search.found",
                            json!({"search": search, "items": batch.iter().map(found_json).collect::<Vec<_>>()}),
                        ));
                    });
                    let _ = out.send(wire::notification(
                        "search.finished",
                        json!({
                            "search": search,
                            "items": summary.best.iter().map(found_json).collect::<Vec<_>>(),
                            "matched": summary.matched,
                            "entries": summary.entries,
                            "truncated": summary.truncated,
                            "cancelled": summary.cancelled,
                        }),
                    ));
                }));
                answer
            }
            Call::SearchExclusion { rule } => {
                match asc_files_engine::search::Exclusions::normalize(&rule, &home()) {
                    Ok(rule) => now(json!({"rule": rule})),
                    Err(why) => Err(Fault::invalid_params(why)),
                }
            }
            Call::SearchCancel { search } => {
                if let Some(cancel) = self.searches.get(&search) {
                    cancel.cancel();
                }
                now(Value::Null)
            }
            Call::MeasureStart { path } => {
                self.next_measure += 1;
                let measure = self.next_measure;
                let cancel = Cancel::new();
                self.measures.insert(measure, cancel.clone());
                let out = self.out.clone();
                let answer = self.answer_first(&id, json!({"measure": measure}));
                self.workers.push(thread::spawn(move || {
                    // The totals so far, every 150 ms at most.
                    let mut sent = std::time::Instant::now();
                    let result = asc_files_engine::info::measure(
                        &path,
                        Default::default(),
                        &cancel,
                        |so_far| {
                            if sent.elapsed() >= Duration::from_millis(150) {
                                sent = std::time::Instant::now();
                                let mut params = summary_json(so_far);
                                params["measure"] = json!(measure);
                                let _ = out.send(wire::notification("measure.progress", params));
                            }
                        },
                    );
                    let params = match result {
                        Ok(summary) => {
                            let mut params = summary_json(&summary);
                            params["unreadable"] = json!(summary.issues.unreadable);
                            params["timedOut"] = json!(summary.issues.timed_out);
                            params["depthLimited"] = json!(summary.issues.depth_limited);
                            params["cancelled"] = json!(summary.cancelled);
                            params["measure"] = json!(measure);
                            params
                        }
                        Err(error) => json!({"measure": measure, "error": wire::error(&error)}),
                    };
                    let _ = out.send(wire::notification("measure.finished", params));
                }));
                answer
            }
            Call::MeasureCancel { measure } => {
                if let Some(cancel) = self.measures.get(&measure) {
                    cancel.cancel();
                }
                now(Value::Null)
            }
            Call::ActionsList { inputs } => now(self.action_folder.list(inputs.as_deref())),
            Call::ActionsRead { id } => self
                .action_folder
                .read(&id)
                .map(Answer::Now)
                .map_err(|error| store_fault(&error)),
            Call::ActionsWrite {
                definition,
                script,
                create,
            } => self
                .action_folder
                .write(&definition, script.as_ref(), create)
                .map(|id| Answer::Now(json!({"id": id})))
                .map_err(|error| store_fault(&error)),
            Call::ActionsSetEnabled { id, enabled } => self
                .action_folder
                .set_enabled(&id, enabled)
                .map(|()| Answer::Now(Value::Null))
                .map_err(|error| store_fault(&error)),
            Call::ActionsDelete { id } => self
                .action_folder
                .delete(&id)
                .map(|()| Answer::Now(Value::Null))
                .map_err(|error| store_fault(&error)),
            Call::ActionsDuplicate { id } => self
                .action_folder
                .duplicate(&id)
                .map(|id| Answer::Now(json!({"id": id})))
                .map_err(|error| store_fault(&error)),
            Call::ActionsImport { path } => self
                .action_folder
                .import(&path)
                .map(|id| Answer::Now(json!({"id": id})))
                .map_err(|error| store_fault(&error)),
            Call::ActionsExport { id, into } => self
                .action_folder
                .export(&id, &into)
                .map(|path| Answer::Now(json!({"path": wire::path(&path)})))
                .map_err(|error| store_fault(&error)),
            Call::ActionsExamples => now(crate::actions::examples()),
            Call::ActionsRun {
                id,
                inputs,
                parent,
                background,
            } => match self.action_queue.run(id, inputs, parent, background) {
                Ok(job) => now(json!({"job": job})),
                Err(why) => Err(Fault::operation(&Error::new(ErrorKind::Failed, &why))),
            },
            Call::ActionsCancel { job } => {
                self.action_queue.cancel(job);
                now(Value::Null)
            }
            Call::ActionsDismiss { job } => {
                self.action_queue.dismiss(job);
                now(Value::Null)
            }
            Call::Chmod { path, bits } => {
                let bits = u32::try_from(bits).unwrap_or(u32::MAX);
                if bits > 0o777 {
                    return Err(Fault::invalid_params("bits must be at most 0o777 (511)"));
                }
                match asc_files_engine::info::set_mode(&path, bits) {
                    Ok(mode) => now(json!({"mode": mode})),
                    Err(error) => Err(Fault::operation(&error)),
                }
            }
            // A rename on a share may wait for a file below it to close.
            Call::Rename { path, name, label } => {
                let journal = self.journal.clone();
                self.answer_later(id, move || {
                    let renamed = ops::rename_typed(&path, &name, &Cancel::new())?;
                    let at = path.with_file_name(&renamed.name);
                    let step = Step::renamed(&path, &renamed);
                    Ok(filed(
                        json!({"path": wire::path(&at)}),
                        journal,
                        label.as_deref().unwrap_or("Rename"),
                        step,
                    ))
                });
                Ok(Answer::Later)
            }
            Call::Create {
                into,
                name,
                folder,
                unique,
                label,
            } => {
                let journal = self.journal.clone();
                self.answer_later(id, move || {
                    let made = ops::create(&into, &name, folder, unique)?;
                    let label =
                        label
                            .as_deref()
                            .unwrap_or(if folder { "New Folder" } else { "New File" });
                    Ok(filed(
                        json!({"path": wire::path(&made)}),
                        journal,
                        label,
                        Step::created(&made),
                    ))
                });
                Ok(Answer::Later)
            }
            Call::UndoHistory => {
                let journal = self.journal.as_ref().ok_or_else(no_history)?;
                let history = journal.history()?;
                let entry = |entry: Option<(u64, String)>| {
                    entry.map(|(id, label)| json!({"id": id, "label": label}))
                };
                now(json!({"undo": entry(history.undo), "redo": entry(history.redo)}))
            }
            Call::UndoPreview => {
                let journal = self.journal.as_ref().ok_or_else(no_history)?;
                let blocked = journal.blocked()?;
                now(
                    json!({"blocked": blocked.iter().map(|path| wire::path(path)).collect::<Vec<_>>()}),
                )
            }
            Call::Undo { .. } | Call::Redo => {
                let redo = matches!(call, Call::Redo);
                let overwrite = matches!(call, Call::Undo { overwrite: true });
                let journal = self.journal.clone().ok_or_else(no_history)?;
                // An undo in progress finishes at shutdown: stopping it halfway would leave its
                // entry half done.
                self.answer_later(id, move || {
                    let (cancel, progress) = (Cancel::new(), Progress::new());
                    let outcome = if redo {
                        journal.redo(&cancel, &progress)
                    } else {
                        journal.undo_with(overwrite, &cancel, &progress)
                    };
                    Ok(undo_json(&outcome?))
                });
                Ok(Answer::Later)
            }
            Call::TrashList => now(json!({
                "entries": trash::list().iter().map(entry_json).collect::<Vec<_>>(),
            })),
            // A restore that must copy can take long.
            Call::TrashRestore { info } => {
                let entry = listed_entry(&info)?;
                self.answer_later(id, move || {
                    let restored = trash::restore(&entry)?;
                    Ok(json!({"path": wire::path(&restored)}))
                });
                Ok(Answer::Later)
            }
            Call::TrashDelete { info } => {
                trash::delete(&listed_entry(&info)?)?;
                now(Value::Null)
            }
            Call::TrashEmpty => {
                let (deleted, failed) = trash::empty();
                now(json!({
                    "deleted": deleted,
                    "failed": failed
                        .iter()
                        .map(|(entry, error)| json!({"info": wire::path(&entry.info), "error": wire::error(error)}))
                        .collect::<Vec<_>>(),
                }))
            }
            // Each may wait on a share.
            Call::LeftoversRecover { path } => {
                self.answer_later(id, move || {
                    let recovered = recovery::recover(&path, &Cancel::new())?;
                    Ok(recovered_json(&recovered))
                });
                Ok(Answer::Later)
            }
            Call::LeftoversReview { folder, names } => {
                self.answer_later(id, move || {
                    Ok(json!({
                        "leftovers": recovery::review(&folder, &names)
                            .iter()
                            .map(reviewed_json)
                            .collect::<Vec<_>>(),
                    }))
                });
                Ok(Answer::Later)
            }
            Call::LeftoversSweep { folder, names } => {
                self.answer_later(id, move || {
                    recovery::sweep_probes(&folder, &names);
                    Ok(Value::Null)
                });
                Ok(Answer::Later)
            }
        }
    }

    /// Stops what is still running and waits until every job has sent its last event.
    pub fn finish(&mut self) {
        self.listings.close_all();
        for cancel in self.searches.values().chain(self.measures.values()) {
            cancel.cancel();
        }
        self.action_queue.finish();
        for handle in self.jobs.values() {
            handle.cancel();
        }
        for (_, handle) in self.jobs.drain() {
            handle.join();
        }
        for forwarder in self.forwarders.drain(..) {
            let _ = forwarder.join();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

enum Answer {
    Now(Value),
    /// Another thread sends the reply.
    Later,
}

fn later(sent: Result<(), Fault>) -> Result<Answer, Fault> {
    sent.map(|()| Answer::Later)
}

/// `result` with the history entry `step` was filed as: `undo` is its id, or null when it was
/// not filed (with `undoError` when the history could not be written).
fn filed(mut result: Value, journal: Option<Journal>, label: &str, step: Option<Step>) -> Value {
    let recorded = match (journal, step) {
        (Some(journal), Some(step)) => journal.record(label, vec![step]),
        _ => Ok(None),
    };
    match recorded {
        Ok(entry) => result["undo"] = json!(entry),
        Err(error) => {
            result["undo"] = Value::Null;
            result["undoError"] = wire::error(&error);
        }
    }
    result
}

fn clashes_json(source: &Path, into: &Path) -> Value {
    let side = |side: &merge::Side| json!({"isFolder": side.is_folder, "size": side.size, "modified": side.modified});
    match merge::clashes(source, into) {
        Ok(clashes) => json!({
            "source": wire::path(source),
            "clashes": clashes
                .iter()
                .map(|clash| json!({
                    "path": wire::path(&clash.path),
                    "incoming": side(&clash.incoming),
                    "existing": side(&clash.existing),
                }))
                .collect::<Vec<_>>(),
        }),
        Err(error) => json!({"source": wire::path(source), "error": wire::error(&error)}),
    }
}

fn found_json(found: &asc_files_engine::search::Found) -> Value {
    json!({
        "path": wire::path(&found.path),
        "isFolder": found.is_folder,
        "size": found.size,
        "modified": found.modified,
        "score": found.score,
    })
}

/// An actions-folder error as a fault with the kind a client tells apart.
fn store_fault(error: &asc_files_engine::actions::catalog::ActionStoreError) -> Fault {
    use asc_files_engine::actions::catalog::ActionStoreError as Store;
    let kind = match error {
        Store::AlreadyExists(_) => ErrorKind::Exists,
        Store::NotFound(_) => ErrorKind::NotFound,
        Store::Invalid(_) | Store::IdMismatch { .. } => ErrorKind::InvalidArgument,
        _ => ErrorKind::Failed,
    };
    Fault::operation(&Error::new(kind, &error.user_message()))
}

fn summary_json(summary: &asc_files_engine::info::Summary) -> Value {
    json!({
        "items": summary.items,
        "size": summary.size,
        "files": summary.files,
        "folders": summary.folders,
    })
}

/// Home, for search rules written from `~/`.
fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn capabilities(undo: bool) -> Vec<&'static str> {
    let mut capabilities = vec![
        "list",
        "list.watch",
        "jobs.copy",
        "jobs.move",
        "jobs.replace",
        "jobs.trash",
        "jobs.delete",
        "jobs.restore",
        "jobs.merge",
        "jobs.durable",
        "ops.rename",
        "ops.create",
        "search",
        "search.exclusions",
        "measure",
        "ops.chmod",
        "actions",
        "list.filter",
        "trash",
        "leftovers",
    ];
    if undo {
        capabilities.push("undo");
    }
    capabilities
}

fn no_history() -> Fault {
    Fault::operation(&Error::new(
        ErrorKind::NotSupported,
        "There is no undo history in this session",
    ))
}

/// What a batch is called in the history when the client names it nothing.
fn default_label(items: &[Item]) -> String {
    let verb = |action: &Action| match action {
        Action::Copy(_) | Action::Merge(_) => "Copy",
        Action::Move | Action::MoveKeepingBoth | Action::MoveReplacing | Action::MergeMoving(_) => {
            "Move"
        }
        Action::Replace => "Replace",
        Action::Trash => "Move to Trash",
        Action::Delete => "Delete",
        Action::Restore(_) => "Restore",
        Action::Extract(_) => "Extract",
        Action::Compress(_) => "Compress",
    };
    let first = items.first().map_or("Operation", |item| verb(&item.action));
    let same = items.iter().all(|item| verb(&item.action) == first);
    match (same, items.len()) {
        (true, 1) => first.to_owned(),
        (true, n) => format!("{first} {n} items"),
        (false, n) => format!("{n} operations"),
    }
}

fn undo_json(outcome: &undo::Outcome) -> Value {
    let changes = changes_json(&outcome.changes);
    json!({
        "id": outcome.id,
        "label": outcome.label,
        "done": outcome.done,
        "left": outcome
            .left
            .iter()
            .map(|(path, error)| json!({"path": wire::path(path), "error": wire::error(error)}))
            .collect::<Vec<_>>(),
        "renamed": changes["renamed"],
        "kept": changes["kept"],
    })
}

fn rename_cause(cause: RenameCause) -> &'static str {
    match cause {
        RenameCause::Alias => "alias",
        RenameCause::Taken => "taken",
        RenameCause::Invalid => "invalid",
        RenameCause::TooLong => "tooLong",
    }
}

fn renamed_json(renamed: &ops::Renamed) -> Value {
    json!({
        "requested": wire::path(&renamed.requested),
        "savedAs": wire::path(&renamed.saved_as),
        "cause": rename_cause(renamed.cause),
    })
}

fn kept_json(kept: &ops::Kept) -> Value {
    json!({"path": wire::path(&kept.path), "cause": kept_cause(&kept.cause)})
}

fn recovered_json(recovered: &recovery::Recovered) -> Value {
    json!({
        "restored": recovered.restored.iter().map(|path| wire::path(path)).collect::<Vec<_>>(),
        "renamed": recovered.renamed.iter().map(renamed_json).collect::<Vec<_>>(),
        "kept": recovered.kept.iter().map(kept_json).collect::<Vec<_>>(),
        "failed": recovered.failed.iter().map(|(path, error)| json!({
            "path": wire::path(path),
            "error": error,
        })).collect::<Vec<_>>(),
        "notAttempted": recovered.not_attempted.iter().map(|path| wire::path(path)).collect::<Vec<_>>(),
        "removed": recovered.removed,
    })
}

fn reviewed_json(reviewed: &ReviewedLeftover) -> Value {
    let inside = match &reviewed.inside {
        LeftoverInside::Item => json!({"kind": "item"}),
        LeftoverInside::Items { names, more } => json!({
            "kind": "items",
            "names": names.iter().map(|name| wire::name(name.as_ref())).collect::<Vec<_>>(),
            "more": more,
        }),
        LeftoverInside::StillOpen(reason) => json!({"kind": "stillOpen", "reason": reason}),
        LeftoverInside::Gone => json!({"kind": "gone"}),
        LeftoverInside::Unreadable(reason) => json!({"kind": "unreadable", "reason": reason}),
    };
    json!({
        "name": wire::name(&reviewed.name),
        "modified": reviewed.modified,
        "inside": inside,
    })
}

/// What an item did otherwise than asked, or left in place.
fn changes_json(changes: &ops::Changes) -> Value {
    json!({
        "renamed": changes.renamed.iter().map(renamed_json).collect::<Vec<_>>(),
        "skipped": changes.skipped.iter().map(|skipped| json!({
            "path": wire::path(&skipped.path),
            "kind": special_kind(skipped.kind),
        })).collect::<Vec<_>>(),
        "kept": changes.kept.iter().map(kept_json).collect::<Vec<_>>(),
        "originalDeleted": changes.original_deleted,
        "leftOut": changes.left_out,
    })
}

/// Why a move left a source entry in place, as `{reason, ...}`.
fn kept_cause(cause: &KeptCause) -> Value {
    match cause {
        KeptCause::Changed => json!({"reason": "changed"}),
        KeptCause::ReplacementChanged => json!({"reason": "replacementChanged"}),
        KeptCause::Undeletable(errno) => json!({
            "reason": "undeletable",
            "error": std::io::Error::from_raw_os_error(*errno).to_string(),
        }),
        KeptCause::Skipped(kind) => json!({"reason": "skipped", "kind": special_kind(*kind)}),
        KeptCause::CopyGone => json!({"reason": "copyGone"}),
        KeptCause::StillOpen(holder) => json!({
            "reason": "stillOpen",
            "program": holder.as_ref().map(|holder| match holder {
                Holder::Program(program) => program.clone(),
                Holder::Strata => "Files".to_owned(),
            }),
        }),
        KeptCause::NameNotRestored { saved_as, error } => json!({
            "reason": "nameNotRestored",
            "savedAs": saved_as.native_path().map(wire::path),
            "error": error,
        }),
        KeptCause::OriginalNotRemoved(error) => {
            json!({"reason": "originalNotRemoved", "error": error})
        }
    }
}

fn special_kind(kind: SpecialKind) -> &'static str {
    match kind {
        SpecialKind::Pipe => "pipe",
        SpecialKind::Socket => "socket",
        SpecialKind::Device => "device",
        SpecialKind::Symlink => "symlink",
    }
}

fn notification(event: Event) -> Value {
    let (method, params) = match event {
        Event::Queued { job, ahead } => ("job.queued", json!({"job": id(job), "ahead": ahead})),
        Event::Started {
            job,
            total,
            bytes,
            files,
        } => (
            "job.started",
            json!({"job": id(job), "total": total, "totalBytes": bytes, "totalFiles": files}),
        ),
        Event::Progress {
            job,
            item,
            progress,
        } => (
            "job.progress",
            json!({
                "job": id(job),
                "item": item,
                "bytes": progress.bytes,
                "files": progress.files,
                "flushes": progress.flushes,
            }),
        ),
        Event::Flushing { job } => ("job.flushing", json!({"job": id(job)})),
        Event::ItemDone {
            job,
            index,
            outcome,
        } => {
            let outcome = match outcome {
                Outcome::Done { name, changes } => {
                    let mut done = changes_json(&changes);
                    done["status"] = json!("done");
                    done["path"] = wire::path(&name);
                    done
                }
                Outcome::Failed { error, changes } => {
                    let mut failed = changes_json(&changes);
                    failed["status"] = json!("failed");
                    failed["error"] = wire::error(&error);
                    failed
                }
                Outcome::NotAttempted => json!({"status": "notAttempted"}),
            };
            (
                "job.itemDone",
                json!({"job": id(job), "index": index, "outcome": outcome}),
            )
        }
        Event::Finished {
            job,
            done,
            failed,
            not_attempted,
            cancelled,
            undo,
            settled,
        } => {
            let mut params = json!({"job": id(job), "done": done, "failed": failed, "notAttempted": not_attempted, "cancelled": cancelled});
            match undo {
                Ok(entry) => params["undo"] = json!(entry),
                Err(error) => {
                    params["undo"] = Value::Null;
                    params["undoError"] = wire::error(&error);
                }
            }
            params["durable"] = json!(settled.is_some());
            if let Some(settled) = settled {
                params["settled"] = json!(settled.is_ok());
                if let Err(error) = settled {
                    params["settleError"] = wire::error(&error);
                }
            }
            ("job.finished", params)
        }
    };
    wire::notification(method, params)
}

/// An entry is named by its `.trashinfo`; it is found afresh, so the engine's identity check
/// refuses one that changed since the client's listing.
fn listed_entry(info: &PathBuf) -> Result<trash::Entry, Fault> {
    trash::list()
        .into_iter()
        .find(|entry| entry.info == *info)
        .ok_or_else(|| {
            Fault::operation(&Error::new(
                ErrorKind::NotFound,
                "That item is no longer in the Trash",
            ))
        })
}

fn entry_json(entry: &trash::Entry) -> Value {
    json!({
        "info": wire::path(&entry.info),
        "file": wire::path(&entry.file),
        "original": wire::path(&entry.original),
        "deleted": entry.deleted,
    })
}

fn id(job: JobId) -> u64 {
    job.0
}
