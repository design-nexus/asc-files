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
    listing::{Change, Kind, Listing, Options, Readers, Row, Watch},
};
use serde_json::{Value, json};

use crate::{
    call::OptionsPatch,
    wire::{self, Fault},
};

/// How long a listing waits for one of its drive's read places before giving up on the drive.
const READ_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

enum Command {
    Window {
        id: Value,
        start: usize,
        count: usize,
    },
    /// Applied on the listing's thread, so what a request leaves out keeps its current value.
    Reorder {
        id: Value,
        options: OptionsPatch,
        /// New filter text, or None to keep the one it has.
        filter: Option<String>,
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

    /// Opens a listing of `folder`; its thread answers `id`.
    pub(crate) fn open(
        &mut self,
        id: Value,
        folder: PathBuf,
        options: &OptionsPatch,
        filter: Option<String>,
    ) {
        let options = options.apply(Options::default());
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
                    (folder, options, filter.unwrap_or_default()),
                    (id, &out),
                    &readers,
                    (changes, commands),
                );
            })
            .expect("a listing thread");
    }

    pub(crate) fn window(
        &mut self,
        id: Value,
        listing: u64,
        start: usize,
        count: usize,
    ) -> Result<(), Fault> {
        self.send(listing, Command::Window { id, start, count })
    }

    pub(crate) fn reorder(
        &mut self,
        id: Value,
        listing: u64,
        options: OptionsPatch,
        filter: Option<String>,
    ) -> Result<(), Fault> {
        self.send(
            listing,
            Command::Reorder {
                id,
                options,
                filter,
            },
        )
    }

    pub(crate) fn refresh(&mut self, id: Value, listing: u64) -> Result<(), Fault> {
        self.send(listing, Command::Refresh { id })
    }

    /// The thread ends once it has answered what it already holds.
    pub(crate) fn close(&mut self, listing: u64) {
        if let Some(listing) = self.open.remove(&listing) {
            let _ = listing.send(Command::Close);
        }
    }

    /// Hands `command` to its listing's thread, which answers it.
    fn send(&mut self, listing: u64, command: Command) -> Result<(), Fault> {
        let sent = self
            .open
            .get(&listing)
            .is_some_and(|listing| listing.send(command).is_ok());
        if sent {
            Ok(())
        } else {
            self.open.remove(&listing);
            Err(Fault::invalid_params("unknown listing"))
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
    (folder, opened_with, filter): (PathBuf, Options, String),
    (id, out): (Value, &Sender<Value>),
    readers: &Arc<Readers>,
    (changes, commands): (Sender<Command>, Receiver<Command>),
) {
    let reply = |id: Value, result: Result<Value, Fault>| {
        let _ = out.send(wire::reply(id, result));
    };
    // Waits a little for a place: many folders opened at once only queue; a stalled share refuses.
    let Some(reading) = readers.claim_within(&folder, READ_WAIT) else {
        reply(id, Err(Fault::operation(&not_responding())));
        return;
    };
    // Armed before the read, so nothing that changes during it is missed.
    let watch = Watch::start(&folder, move |change| {
        let _ = changes.send(Command::Changed(change));
    });
    let read = Listing::read(&folder, opened_with).and_then(|mut listing| {
        if !filter.is_empty() {
            listing.set_filter(&filter)?;
        }
        Ok(listing)
    });
    drop(reading);
    let mut current = match read {
        Ok(read) => read,
        Err(error) => {
            reply(id, Err(error.into()));
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
            "leftovers": leftovers(&current),
        })),
    );
    let notify = |method: &str, params: Value| {
        let _ = out.send(wire::notification(method, params));
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
            Command::Reorder {
                id,
                options,
                filter,
            } => {
                let result = current
                    .set_options(options.apply(current.options()))
                    .and_then(|()| match &filter {
                        Some(text) if text != current.filter() => current.set_filter(text),
                        _ => Ok(()),
                    })
                    .map_err(Fault::from);
                reply(id, result.map(|()| counted(&current)));
            }
            Command::Refresh { id } => {
                let result = current.refresh().map_err(Fault::from);
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
                json!({"listing": listing, "path": wire::path(current.folder()), "nearest": wire::path(&nearest)}),
            ),
            Command::Changed(Change::Unmounted { nearest }) => notify(
                "list.unmounted",
                json!({"listing": listing, "path": wire::path(current.folder()), "nearest": wire::path(&nearest)}),
            ),
            Command::Close => break,
        }
    }
}

fn counted(listing: &Listing) -> Value {
    json!({"generation": listing.generation(), "count": listing.len(), "leftovers": leftovers(listing)})
}

/// What interrupted operations left in the folder, hidden or not.
fn leftovers(listing: &Listing) -> Value {
    json!(
        listing
            .leftovers()
            .iter()
            .map(|name| wire::name(name))
            .collect::<Vec<_>>()
    )
}

fn not_responding() -> Error {
    Error::new(
        ErrorKind::TimedOut,
        "The drive is not responding; earlier reads of it have not returned",
    )
}

fn row(row: &Row) -> Value {
    let kind = match row.kind {
        Kind::Folder => "folder",
        Kind::File => "file",
        Kind::Link => "link",
        Kind::Special => "special",
    };
    let mut value = json!({
        "name": wire::name(&row.name),
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
