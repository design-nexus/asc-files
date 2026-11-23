// SPDX-License-Identifier: MIT
//! The Files backend (`asc-files-backend`). JSON-RPC 2.0, one message per line, on stdin and stdout.
//! stdout carries only protocol lines; diagnostics go to stderr.

use std::{
    collections::HashMap,
    io::{BufRead, Write},
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread,
};

use asc_files_engine::{
    error::Error,
    jobs::{self, Action, Event, Item, JobId, Outcome, Record, Request},
    merge::{self, Clash, MergePlan},
    ops::{Cancel, Conflict, NameMode, Progress, RenameCause, SpecialKind},
    trash,
    undo::{self, Journal},
};
use serde_json::{Value, json};

mod listing;

/// Bumped on any change a client must know about; `initialize` reports it.
const PROTOCOL_VERSION: u32 = 1;

fn main() {
    let (out, lines) = mpsc::channel::<Value>();
    let writer = thread::spawn(move || {
        let stdout = std::io::stdout();
        for message in lines {
            let mut lock = stdout.lock();
            if writeln!(lock, "{message}")
                .and_then(|()| lock.flush())
                .is_err()
            {
                break;
            }
        }
    });
    let mut session = Session::new(out.clone());
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => session.handle(message),
            Err(error) => Some(error_reply(Value::Null, PARSE_ERROR, &error.to_string())),
        };
        if let Some(reply) = reply {
            let _ = out.send(reply);
        }
        if session.shutting_down {
            break;
        }
    }
    session.finish();
    drop(session);
    drop(out);
    let _ = writer.join();
}

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
/// Application errors: the operation was understood and refused.
const OPERATION_FAILED: i64 = 1;

struct Session {
    out: Sender<Value>,
    initialized: bool,
    shutting_down: bool,
    listings: listing::Listings,
    jobs: HashMap<u64, jobs::Handle>,
    forwarders: Vec<thread::JoinHandle<()>>,
    /// The session's undo history, shared with every other Files process; None without one.
    journal: Option<Journal>,
    /// Undo, redo and clash lookups running off the loop; waited for at shutdown.
    workers: Vec<thread::JoinHandle<()>>,
}

impl Session {
    fn new(out: Sender<Value>) -> Self {
        Self {
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
        }
    }

    /// Returns the response, or None for a notification (a message without an id).
    fn handle(&mut self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Some(error_reply(
                id.unwrap_or(Value::Null),
                INVALID_REQUEST,
                "missing method",
            ));
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = if !self.initialized && method != "initialize" {
            Err((INVALID_REQUEST, "initialize first".to_owned()))
        } else if let Some(direction) = method.strip_prefix("undo.").filter(|d| *d != "history") {
            match self.start_undo(direction, id.clone().unwrap_or(Value::Null)) {
                Ok(()) => return None,
                Err(error) => Err(error),
            }
        } else if method == "jobs.clashes" {
            match self.start_clashes(id.clone().unwrap_or(Value::Null), &params) {
                Ok(()) => return None,
                Err(error) => Err(error),
            }
        } else if method.starts_with("list.") {
            // A listing answers on its own thread; a request without an id still runs.
            match self
                .listings
                .handle(method, id.clone().unwrap_or(Value::Null), &params)
            {
                Ok(None) => return None,
                Ok(Some(result)) => Ok(result),
                Err(error) => Err(error),
            }
        } else {
            self.dispatch(method, &params)
        };
        let id = id?;
        Some(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => error_reply(id, code, &message),
        })
    }

    fn dispatch(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                self.initialized = true;
                Ok(json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "server": "asc-files-backend",
                    "capabilities": capabilities(self.journal.is_some()),
                }))
            }
            "undo.history" => {
                let journal = self.journal.as_ref().ok_or_else(no_history)?;
                let history = journal.history().map_err(operation_error)?;
                let entry = |entry: Option<(u64, String)>| {
                    entry.map(|(id, label)| json!({"id": id, "label": label}))
                };
                Ok(json!({"undo": entry(history.undo), "redo": entry(history.redo)}))
            }
            "jobs.start" => {
                let mut request = job_request(params)?;
                request.record = self.journal.clone().map(|journal| Record {
                    journal,
                    label: params
                        .get("label")
                        .and_then(Value::as_str)
                        .map_or_else(|| default_label(&request.items), str::to_owned),
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
                Ok(json!({"job": job}))
            }
            "jobs.cancel" => {
                let job = u64_param(params, "job")?;
                if let Some(handle) = self.jobs.get(&job) {
                    handle.cancel();
                }
                Ok(Value::Null)
            }
            "trash.list" => Ok(json!({
                "entries": trash::list().iter().map(entry_json).collect::<Vec<_>>(),
            })),
            "trash.restore" => {
                let entry = listed_entry(params)?;
                let restored = trash::restore(&entry).map_err(operation_error)?;
                Ok(json!({"path": lossy(&restored)}))
            }
            "trash.delete" => {
                let entry = listed_entry(params)?;
                trash::delete(&entry).map_err(operation_error)?;
                Ok(Value::Null)
            }
            "trash.empty" => {
                let (deleted, failed) = trash::empty();
                Ok(json!({
                    "deleted": deleted,
                    "failed": failed
                        .iter()
                        .map(|(entry, error)| json!({"info": lossy(&entry.info), "error": error_json(error)}))
                        .collect::<Vec<_>>(),
                }))
            }
            "shutdown" => {
                self.shutting_down = true;
                Ok(Value::Null)
            }
            other => Err((METHOD_NOT_FOUND, format!("unknown method {other}"))),
        }
    }

    /// Stops what is still running and waits until every job has sent its last event.
    fn finish(&mut self) {
        self.listings.close_all();
        for handle in self.jobs.values() {
            handle.cancel();
        }
        for (_, handle) in self.jobs.drain() {
            handle.join();
        }
        for forwarder in self.forwarders.drain(..) {
            let _ = forwarder.join();
        }
        // An undo in progress finishes: stopping it halfway would leave its entry half done.
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }

    /// Lists the clashes a merge of each source into `into` would meet, on a thread of its own:
    /// it walks folders, which on a slow drive takes a while.
    fn start_clashes(&mut self, id: Value, params: &Value) -> Result<(), (i64, String)> {
        let into = path_param(params, "into")?;
        let sources = params
            .get("sources")
            .and_then(Value::as_array)
            .ok_or((INVALID_PARAMS, "sources must be an array".to_owned()))?
            .iter()
            .map(|source| {
                source
                    .as_str()
                    .map(PathBuf::from)
                    .ok_or((INVALID_PARAMS, "sources must be string paths".to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let out = self.out.clone();
        self.workers.push(thread::spawn(move || {
            let items: Vec<Value> = sources
                .iter()
                .map(|source| clashes_json(source, &into))
                .collect();
            let _ = out.send(json!({"jsonrpc": "2.0", "id": id, "result": {"items": items}}));
        }));
        Ok(())
    }

    /// Runs `undo.undo` or `undo.redo` on a thread of its own; it answers `id` when done.
    fn start_undo(&mut self, direction: &str, id: Value) -> Result<(), (i64, String)> {
        let redo = match direction {
            "undo" => false,
            "redo" => true,
            other => return Err((METHOD_NOT_FOUND, format!("unknown method undo.{other}"))),
        };
        let journal = self.journal.clone().ok_or_else(no_history)?;
        let out = self.out.clone();
        self.workers.push(thread::spawn(move || {
            let (cancel, progress) = (Cancel::new(), Progress::new());
            let outcome = if redo {
                journal.redo(&cancel, &progress)
            } else {
                journal.undo(&cancel, &progress)
            };
            let reply = match outcome {
                Ok(outcome) => json!({"jsonrpc": "2.0", "id": id, "result": undo_json(&outcome)}),
                Err(error) => {
                    let (code, message) = operation_error(error);
                    error_reply(id, code, &message)
                }
            };
            let _ = out.send(reply);
        }));
        Ok(())
    }
}

fn job_request(params: &Value) -> Result<Request, (i64, String)> {
    let into = path_param(params, "into")?;
    let items = params
        .get("items")
        .and_then(Value::as_array)
        .ok_or((INVALID_PARAMS, "items must be an array".to_owned()))?
        .iter()
        .map(|item| {
            let source = path_param(item, "source")?;
            let action = match item.get("action").and_then(Value::as_str) {
                Some("copy") => Action::Copy(match item.get("conflict").and_then(Value::as_str) {
                    None | Some("fail") => Conflict::Fail,
                    Some("keepBoth") => Conflict::KeepBoth,
                    Some("merge") => Conflict::Merge,
                    Some(other) => {
                        return Err((INVALID_PARAMS, format!("unknown conflict {other}")));
                    }
                }),
                Some("merge") => Action::Merge(merge_plan(item)?),
                Some("move") => Action::Move,
                Some("replace") => Action::Replace,
                Some("trash") => Action::Trash,
                other => return Err((INVALID_PARAMS, format!("unknown action {other:?}"))),
            };
            Ok(Item { source, action })
        })
        .collect::<Result<_, _>>()?;
    let names = match params.get("names").and_then(Value::as_str) {
        None | Some("fix") => NameMode::Fix,
        Some("keep") => NameMode::Keep,
        Some(other) => return Err((INVALID_PARAMS, format!("unknown names {other}"))),
    };
    let durable = match params.get("durable") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_bool()
                .ok_or((INVALID_PARAMS, "durable must be a boolean".to_owned()))?,
        ),
    };
    Ok(Request {
        into,
        items,
        names,
        record: None,
        durable,
    })
}

/// `choices: {"<path in the folder>": "keepBoth"|"replace"|"skip"}` and `otherwise`.
fn merge_plan(item: &Value) -> Result<MergePlan, (i64, String)> {
    let clash = |value: &Value| match value.as_str() {
        Some("keepBoth") => Ok(Clash::KeepBoth),
        Some("replace") => Ok(Clash::Replace),
        Some("skip") => Ok(Clash::Skip),
        _ => Err((INVALID_PARAMS, format!("unknown clash choice {value}"))),
    };
    let choices = match item.get("choices") {
        None => Default::default(),
        Some(Value::Object(choices)) => choices
            .iter()
            .map(|(path, choice)| Ok((PathBuf::from(path), clash(choice)?)))
            .collect::<Result<_, (i64, String)>>()?,
        Some(_) => return Err((INVALID_PARAMS, "choices must be an object".to_owned())),
    };
    let otherwise = match item.get("otherwise") {
        None => Clash::KeepBoth,
        Some(value) => clash(value)?,
    };
    Ok(MergePlan { choices, otherwise })
}

fn clashes_json(source: &std::path::Path, into: &std::path::Path) -> Value {
    let side = |side: &merge::Side| json!({"isFolder": side.is_folder, "size": side.size, "modified": side.modified});
    match merge::clashes(source, into) {
        Ok(clashes) => json!({
            "source": lossy(source),
            "clashes": clashes
                .iter()
                .map(|clash| json!({
                    "path": lossy(&clash.path),
                    "incoming": side(&clash.incoming),
                    "existing": side(&clash.existing),
                }))
                .collect::<Vec<_>>(),
        }),
        Err(error) => json!({"source": lossy(source), "error": error_json(&error)}),
    }
}

fn capabilities(undo: bool) -> Vec<&'static str> {
    let mut capabilities = vec![
        "list",
        "list.watch",
        "jobs.copy",
        "jobs.move",
        "jobs.replace",
        "jobs.trash",
        "jobs.merge",
        "jobs.durable",
        "trash",
    ];
    if undo {
        capabilities.push("undo");
    }
    capabilities
}

fn no_history() -> (i64, String) {
    operation_error(Error::new(
        asc_files_engine::error::ErrorKind::NotSupported,
        "There is no undo history in this session",
    ))
}

/// What a batch is called in the history when the client names it nothing.
fn default_label(items: &[Item]) -> String {
    let verb = |action: &Action| match action {
        Action::Copy(_) | Action::Merge(_) => "Copy",
        Action::Move => "Move",
        Action::Replace => "Replace",
        Action::Trash => "Move to Trash",
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
    json!({
        "id": outcome.id,
        "label": outcome.label,
        "done": outcome.done,
        "left": outcome
            .left
            .iter()
            .map(|(path, error)| json!({"path": lossy(path), "error": error_json(error)}))
            .collect::<Vec<_>>(),
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
        Event::Started { job, total } => ("job.started", json!({"job": id(job), "total": total})),
        Event::Progress {
            job,
            item,
            progress,
        } => (
            "job.progress",
            json!({"job": id(job), "item": item, "bytes": progress.bytes, "files": progress.files}),
        ),
        Event::ItemDone {
            job,
            index,
            outcome,
        } => {
            let outcome = match outcome {
                Outcome::Done { name, changes } => json!({
                    "status": "done",
                    "path": lossy(&name),
                    "renamed": changes.renamed.iter().map(|renamed| json!({
                        "requested": lossy(&renamed.requested),
                        "savedAs": lossy(&renamed.saved_as),
                        "cause": rename_cause(renamed.cause),
                    })).collect::<Vec<_>>(),
                    "skipped": changes.skipped.iter().map(|skipped| json!({
                        "path": lossy(&skipped.path),
                        "kind": special_kind(skipped.kind),
                    })).collect::<Vec<_>>(),
                }),
                Outcome::Failed(error) => json!({"status": "failed", "error": error_json(&error)}),
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
                    params["undoError"] = error_json(&error);
                }
            }
            params["durable"] = json!(settled.is_some());
            if let Some(settled) = settled {
                params["settled"] = json!(settled.is_ok());
                if let Err(error) = settled {
                    params["settleError"] = error_json(&error);
                }
            }
            ("job.finished", params)
        }
    };
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

/// An entry is named by its `.trashinfo`; it is found afresh, so the engine's identity check
/// refuses one that changed since the client's listing.
fn listed_entry(params: &Value) -> Result<trash::Entry, (i64, String)> {
    let info = path_param(params, "info")?;
    trash::list()
        .into_iter()
        .find(|entry| entry.info == info)
        .ok_or((
            OPERATION_FAILED,
            "That item is no longer in the Trash".to_owned(),
        ))
}

fn entry_json(entry: &trash::Entry) -> Value {
    json!({
        "info": lossy(&entry.info),
        "file": lossy(&entry.file),
        "original": lossy(&entry.original),
        "deleted": entry.deleted,
    })
}

fn id(job: JobId) -> u64 {
    job.0
}

/// The kind is for code to branch on; the message is a localized sentence to show.
fn error_json(error: &Error) -> Value {
    json!({"kind": format!("{:?}", error.kind()), "message": error.message()})
}

fn operation_error(error: Error) -> (i64, String) {
    (
        OPERATION_FAILED,
        serde_json::to_string(&error_json(&error)).unwrap_or_default(),
    )
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Paths travel as UTF-8 strings in this spike; non-UTF-8 names need a byte form in v2.
fn path_param(params: &Value, name: &str) -> Result<PathBuf, (i64, String)> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or((INVALID_PARAMS, format!("{name} must be a string path")))
}

fn u64_param(params: &Value, name: &str) -> Result<u64, (i64, String)> {
    params.get(name).and_then(Value::as_u64).ok_or((
        INVALID_PARAMS,
        format!("{name} must be a non-negative integer"),
    ))
}

fn lossy(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}
