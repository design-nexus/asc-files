// SPDX-License-Identifier: MIT
//! `list.*`: each open listing lives on its own thread with its folder's [`Watch`], so a read
//! or stat that stalls on a dead network share holds that listing alone, never the loop. A
//! listing answers its own requests, in the order they were sent, and re-reads itself when the
//! folder changes, telling the client the new generation with `list.changed`.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use asc_files_engine::{
    error::{Error, ErrorKind},
    listing::{Change, Kind, Listing, Options, Order, Readers, Row, SortKey, Watch},
};
use serde_json::{Value, json};

use super::{INVALID_PARAMS, error_reply, lossy, operation_error, path_param, u64_param};

enum Command {
    Window {
        id: Value,
        start: usize,
        count: usize,
    },
    /// Parsed on the listing's thread, so what a request leaves out keeps its current value.
    Reorder {
        id: Value,
        params: Value,
    },
    Refresh {
        id: Value,
    },
    Changed(Change),
    /// The watch holds a sender too, so the channel alone never closes.
    Close,
}

pub(crate) struct Listings {
    out: Sender<Value>,
    readers: Arc<Readers>,
    next: u64,
    open: HashMap<u64, Sender<Command>>,
}

impl Listings {
    pub(crate) fn new(out: Sender<Value>) -> Self {
        Self {
            out,
            readers: Arc::default(),
            next: 0,
            open: HashMap::new(),
        }
    }

    /// Handles one `list.*` request. Ok(None) means the listing's thread will answer `id`.
    pub(crate) fn handle(
        &mut self,
        method: &str,
        id: Value,
        params: &Value,
    ) -> Result<Option<Value>, (i64, String)> {
        if method == "list.open" {
            let folder = path_param(params, "path")?;
            let options = options(params, Options::default())?;
            self.next += 1;
            let listing = self.next;
            let (send, commands) = mpsc::channel();
            let (out, readers, changes) = (self.out.clone(), self.readers.clone(), send.clone());
            self.open.insert(listing, send);
            thread::Builder::new()
                .name(format!("asc-files-listing-{listing}"))
                .spawn(move || {
                    run(
                        listing,
                        (folder, options),
                        (id, &out),
                        &readers,
                        (changes, commands),
                    );
                })
                .expect("a listing thread");
            return Ok(None);
        }
        let listing = u64_param(params, "listing")?;
        if method == "list.close" {
            // The thread ends once it has answered what it already holds.
            if let Some(listing) = self.open.remove(&listing) {
                let _ = listing.send(Command::Close);
            }
            return Ok(Some(Value::Null));
        }
        let command = match method {
            "list.window" => Command::Window {
                id,
                start: usize::try_from(u64_param(params, "start")?).unwrap_or(usize::MAX),
                count: usize::try_from(u64_param(params, "count")?).unwrap_or(usize::MAX),
            },
            "list.reorder" => Command::Reorder {
                id,
                params: params.clone(),
            },
            "list.refresh" => Command::Refresh { id },
            other => return Err((super::METHOD_NOT_FOUND, format!("unknown method {other}"))),
        };
        let sent = self
            .open
            .get(&listing)
            .is_some_and(|listing| listing.send(command).is_ok());
        if sent {
            Ok(None)
        } else {
            self.open.remove(&listing);
            Err((INVALID_PARAMS, "unknown listing".to_owned()))
        }
    }

    /// Lets every listing go; a thread stalled on a dead share ends when its read returns.
    pub(crate) fn close_all(&mut self) {
        for (_, listing) in self.open.drain() {
            let _ = listing.send(Command::Close);
        }
    }
}

fn run(
    listing: u64,
    (folder, opened_with): (PathBuf, Options),
    (id, out): (Value, &Sender<Value>),
    readers: &Arc<Readers>,
    (changes, commands): (Sender<Command>, Receiver<Command>),
) {
    let reply = |id: Value, result: Result<Value, (i64, String)>| {
        let _ = out.send(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => error_reply(id, code, &message),
        });
    };
    let Some(reading) = readers.claim(&folder) else {
        reply(id, Err(operation_error(not_responding())));
        return;
    };
    // Armed before the read, so nothing that changes during it is missed.
    let watch = Watch::start(&folder, move |change| {
        let _ = changes.send(Command::Changed(change));
    });
    let read = Listing::read(&folder, opened_with);
    drop(reading);
    let mut current = match read {
        Ok(read) => read,
        Err(error) => {
            reply(id, Err(operation_error(error)));
            return;
        }
    };
    reply(
        id,
        Ok(json!({
            "listing": listing,
            "generation": current.generation(),
            "count": current.len(),
            "followsChanges": watch.follows_changes(),
        })),
    );
    let notify = |method: &str, params: Value| {
        let _ = out.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    };
    for command in commands {
        match command {
            Command::Window { id, start, count } => {
                let rows: Vec<Value> = current.window(start, count).iter().map(row).collect();
                reply(
                    id,
                    Ok(json!({
                        "listing": listing,
                        "generation": current.generation(),
                        "start": start.min(current.len()),
                        "rows": rows,
                    })),
                );
            }
            Command::Reorder { id, params } => {
                let result = options(&params, current.options())
                    .and_then(|options| current.set_options(options).map_err(operation_error));
                reply(id, result.map(|()| counted(&current)));
            }
            Command::Refresh { id } => {
                let result = current.refresh().map_err(operation_error);
                reply(id, result.map(|()| counted(&current)));
            }
            Command::Changed(Change::Changed) => {
                // A folder that went fails here; its Gone follows.
                if current.refresh().is_ok() {
                    let mut params = counted(&current);
                    params["listing"] = json!(listing);
                    notify("list.changed", params);
                }
            }
            Command::Changed(Change::Gone { nearest }) => notify(
                "list.gone",
                json!({"listing": listing, "path": lossy(current.folder()), "nearest": lossy(&nearest)}),
            ),
            Command::Changed(Change::Unmounted { nearest }) => notify(
                "list.unmounted",
                json!({"listing": listing, "path": lossy(current.folder()), "nearest": lossy(&nearest)}),
            ),
            Command::Close => break,
        }
    }
}

fn counted(listing: &Listing) -> Value {
    json!({"generation": listing.generation(), "count": listing.len()})
}

fn not_responding() -> Error {
    Error::new(
        ErrorKind::TimedOut,
        "The drive is not responding; earlier reads of it have not returned",
    )
}

/// `showHidden` and `sort: {key, descending, foldersFirst}`, each defaulting to `base`'s.
fn options(params: &Value, base: Options) -> Result<Options, (i64, String)> {
    let show_hidden = match params.get("showHidden") {
        None => base.show_hidden,
        Some(value) => value
            .as_bool()
            .ok_or((INVALID_PARAMS, "showHidden must be a boolean".to_owned()))?,
    };
    let Some(sort) = params.get("sort") else {
        return Ok(Options {
            show_hidden,
            ..base
        });
    };
    let key = match sort.get("key").and_then(Value::as_str) {
        None => base.order.key,
        Some("name") => SortKey::Name,
        Some("size") => SortKey::Size,
        Some("modified") => SortKey::Modified,
        Some(other) => return Err((INVALID_PARAMS, format!("unknown sort key {other}"))),
    };
    let flag = |name: &str, default: bool| match sort.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or((INVALID_PARAMS, format!("sort.{name} must be a boolean"))),
    };
    Ok(Options {
        show_hidden,
        order: Order {
            key,
            descending: flag("descending", base.order.descending)?,
            folders_first: flag("foldersFirst", base.order.folders_first)?,
        },
    })
}

fn row(row: &Row) -> Value {
    let kind = match row.kind {
        Kind::Folder => "folder",
        Kind::File => "file",
        Kind::Link => "link",
        Kind::Special => "special",
    };
    let mut value = json!({
        "name": row.name.to_string_lossy(),
        "kind": kind,
        "linkToFolder": row.link_to_folder,
        "hidden": row.hidden,
    });
    match row.stat {
        Some(stat) => {
            value["size"] = json!(stat.size);
            value["modified"] = json!(stat.modified.0);
            value["modifiedNs"] = json!(stat.modified.1);
            value["mode"] = json!(stat.mode);
        }
        None => value["gone"] = json!(true),
    }
    value
}
