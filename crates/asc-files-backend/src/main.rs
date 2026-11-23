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
    jobs::{self, Action, Event, Item, JobId, Outcome, Request},
    ops::{Conflict, NameMode, RenameCause, SpecialKind},
    trash,
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
}

impl Session {
    fn new(out: Sender<Value>) -> Self {
        Self {
            out,
            initialized: false,
            shutting_down: false,
            listings: listing::Listings::default(),
            jobs: HashMap::new(),
            forwarders: Vec::new(),
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
                    "capabilities": ["list", "jobs.copy", "jobs.move", "jobs.replace", "jobs.trash", "trash"],
                }))
            }
            "list.open" => {
                let path = path_param(params, "path")?;
                let (listing, count) = self.listings.open(&path).map_err(operation_error)?;
                Ok(json!({"listing": listing, "count": count}))
            }
            "list.window" => {
                let listing = u64_param(params, "listing")?;
                let start = u64_param(params, "start")? as usize;
                let count = u64_param(params, "count")? as usize;
                let rows = self
                    .listings
                    .window(listing, start, count)
                    .ok_or((INVALID_PARAMS, "unknown listing".to_owned()))?;
                Ok(json!({"listing": listing, "start": start, "rows": rows}))
            }
            "list.close" => {
                self.listings.close(u64_param(params, "listing")?);
                Ok(Value::Null)
            }
            "jobs.start" => {
                let request = job_request(params)?;
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
        for handle in self.jobs.values() {
            handle.cancel();
        }
        for (_, handle) in self.jobs.drain() {
            handle.join();
        }
        for forwarder in self.forwarders.drain(..) {
            let _ = forwarder.join();
        }
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
    Ok(Request { into, items, names })
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
        } => (
            "job.finished",
            json!({"job": id(job), "done": done, "failed": failed, "notAttempted": not_attempted, "cancelled": cancelled}),
        ),
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
