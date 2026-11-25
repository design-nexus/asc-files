// SPDX-License-Identifier: MIT
//! Reads one protocol line into a typed [`Call`], checking every parameter, before anything is
//! done: nothing here touches a file, so the parser can be fuzzed on its own. A path must be
//! absolute and a name plain (see [`crate::wire`]).

use std::{ffi::OsString, path::PathBuf};

use asc_files_engine::{
    jobs::{Action, Item, Request},
    listing::{Options, Order, SortKey},
    merge::{Clash, MergePlan},
    ops::{Conflict, NameMode},
};
use serde_json::{Map, Value};

use crate::wire::{self, Fault, PARSE_ERROR};

/// Who a reply goes to.
#[derive(Clone, Debug, PartialEq)]
pub enum Id {
    /// A request: its reply carries this id (a string, a number or null).
    Request(Value),
    /// A notification, which gets no reply, not even an error.
    Notification,
    /// The line was not a request at all; its error goes with a null id.
    Unknown,
}

pub struct Incoming {
    pub id: Id,
    pub call: Result<Call, Fault>,
}

/// What a request leaves out of a listing's options keeps its value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OptionsPatch {
    pub show_hidden: Option<bool>,
    pub key: Option<SortKey>,
    pub descending: Option<bool>,
    pub folders_first: Option<bool>,
}

impl OptionsPatch {
    pub fn apply(&self, base: Options) -> Options {
        Options {
            show_hidden: self.show_hidden.unwrap_or(base.show_hidden),
            order: Order {
                key: self.key.unwrap_or(base.order.key),
                descending: self.descending.unwrap_or(base.order.descending),
                folders_first: self.folders_first.unwrap_or(base.order.folders_first),
            },
        }
    }
}

pub enum Call {
    Initialize {
        protocol_version: Option<u64>,
    },
    Shutdown,
    ListOpen {
        path: PathBuf,
        options: OptionsPatch,
        filter: Option<String>,
    },
    ListWindow {
        listing: u64,
        start: usize,
        count: usize,
    },
    ListReorder {
        listing: u64,
        options: OptionsPatch,
        filter: Option<String>,
    },
    ListRefresh {
        listing: u64,
    },
    ListClose {
        listing: u64,
    },
    JobsStart {
        request: Request,
        label: Option<String>,
    },
    JobsCancel {
        job: u64,
    },
    JobsClashes {
        into: PathBuf,
        sources: Vec<PathBuf>,
    },
    JobsPlan {
        into: PathBuf,
        sources: Vec<PathBuf>,
    },
    SearchStart {
        roots: Vec<PathBuf>,
        query: String,
        hidden: bool,
        limit: Option<u64>,
        /// Entries looked at and seconds spent before it stops; 0 for no limit.
        entries: Option<u64>,
        seconds: Option<u64>,
        /// Folders left out, by name or path (`search::Exclusions`).
        exclude: Vec<String>,
    },
    /// A rule for `exclude` as it would be kept, or why it can't be one.
    SearchExclusion {
        rule: String,
    },
    SearchCancel {
        search: u64,
    },
    /// What a folder holds, measured as `measure.progress` and `measure.finished` say.
    MeasureStart {
        path: PathBuf,
    },
    MeasureCancel {
        measure: u64,
    },
    /// Custom actions (`actions.*`): the folder, read and edited, and runs.
    ActionsList {
        inputs: Option<Vec<crate::actions::Input>>,
    },
    ActionsRead {
        id: String,
    },
    ActionsWrite {
        definition: Value,
        script: Option<Value>,
        create: bool,
    },
    ActionsSetEnabled {
        id: String,
        enabled: bool,
    },
    ActionsDelete {
        id: String,
    },
    ActionsDuplicate {
        id: String,
    },
    ActionsImport {
        path: PathBuf,
    },
    ActionsExport {
        id: String,
        into: PathBuf,
    },
    ActionsExamples,
    ActionsRun {
        id: String,
        inputs: Vec<crate::actions::Input>,
        parent: PathBuf,
        background: bool,
    },
    ActionsCancel {
        job: u64,
    },
    ActionsDismiss {
        job: u64,
    },
    /// The read, write and run bits of `path` set to `bits`.
    Chmod {
        path: PathBuf,
        bits: u64,
    },
    Rename {
        path: PathBuf,
        name: String,
        label: Option<String>,
    },
    Create {
        into: PathBuf,
        name: String,
        folder: bool,
        unique: bool,
        label: Option<String>,
    },
    UndoHistory,
    UndoPreview,
    Undo {
        overwrite: bool,
    },
    Redo,
    TrashList,
    TrashRestore {
        info: PathBuf,
    },
    TrashDelete {
        info: PathBuf,
    },
    TrashEmpty,
    LeftoversRecover {
        path: PathBuf,
    },
    LeftoversReview {
        folder: PathBuf,
        names: Vec<OsString>,
    },
    LeftoversSweep {
        folder: PathBuf,
        names: Vec<OsString>,
    },
}

/// Reads one line. Batches (JSON arrays) are not supported.
pub fn parse(line: &str) -> Incoming {
    let message = match serde_json::from_str::<Value>(line) {
        Ok(message) => message,
        Err(error) => {
            return Incoming {
                id: Id::Unknown,
                call: Err(Fault::new(PARSE_ERROR, error.to_string())),
            };
        }
    };
    let Value::Object(message) = message else {
        return unknown(Fault::invalid_request("a message must be an object"));
    };
    let id = match message.get("id") {
        None => Id::Notification,
        Some(id @ (Value::String(_) | Value::Number(_) | Value::Null)) => Id::Request(id.clone()),
        Some(_) => {
            return unknown(Fault::invalid_request(
                "id must be a string, number or null",
            ));
        }
    };
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Incoming {
            id,
            call: Err(Fault::invalid_request("jsonrpc must be \"2.0\"")),
        };
    }
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Incoming {
            id,
            call: Err(Fault::invalid_request("method must be a string")),
        };
    };
    let empty = Map::new();
    let call = match message.get("params") {
        None | Some(Value::Null) => call(method, &Params::new(&empty)),
        Some(Value::Object(params)) => call(method, &Params::new(params)),
        Some(_) => Err(Fault::invalid_params("params must be an object")),
    };
    Incoming { id, call }
}

fn unknown(fault: Fault) -> Incoming {
    Incoming {
        id: Id::Unknown,
        call: Err(fault),
    }
}

fn call(method: &str, params: &Params) -> Result<Call, Fault> {
    Ok(match method {
        "initialize" => Call::Initialize {
            protocol_version: params.optional("protocolVersion", Params::u64_of)?,
        },
        "shutdown" => Call::Shutdown,
        "list.open" => Call::ListOpen {
            path: params.path("path")?,
            options: options(params)?,
            filter: params.optional("filter", Params::string_of)?,
        },
        "list.window" => Call::ListWindow {
            listing: params.u64("listing")?,
            start: params.index("start")?,
            count: params.index("count")?,
        },
        "list.reorder" => Call::ListReorder {
            listing: params.u64("listing")?,
            options: options(params)?,
            filter: params.optional("filter", Params::string_of)?,
        },
        "list.refresh" => Call::ListRefresh {
            listing: params.u64("listing")?,
        },
        "list.close" => Call::ListClose {
            listing: params.u64("listing")?,
        },
        "jobs.start" => Call::JobsStart {
            request: job_request(params)?,
            label: params.optional("label", Params::string_of)?,
        },
        "jobs.cancel" => Call::JobsCancel {
            job: params.u64("job")?,
        },
        "jobs.clashes" => Call::JobsClashes {
            into: params.path("into")?,
            sources: params.array("sources", |at, value| {
                wire::absolute_path(value).ok_or_else(|| not_a_path(at))
            })?,
        },
        "jobs.plan" => Call::JobsPlan {
            into: params.path("into")?,
            sources: params.array("sources", |at, value| {
                wire::absolute_path(value).ok_or_else(|| not_a_path(at))
            })?,
        },
        "search.start" => Call::SearchStart {
            roots: params.array("roots", |at, value| {
                wire::absolute_path(value).ok_or_else(|| not_a_path(at))
            })?,
            query: params.string("query")?,
            hidden: params.optional("hidden", Params::bool_of)?.unwrap_or(false),
            limit: params.optional("limit", Params::u64_of)?,
            entries: params.optional("entries", Params::u64_of)?,
            seconds: params.optional("seconds", Params::u64_of)?,
            exclude: params
                .optional("exclude", Params::strings_of)?
                .unwrap_or_default(),
        },
        "search.exclusion" => Call::SearchExclusion {
            rule: params.string("rule")?,
        },
        "search.cancel" => Call::SearchCancel {
            search: params.u64("search")?,
        },
        "measure.start" => Call::MeasureStart {
            path: params.path("path")?,
        },
        "measure.cancel" => Call::MeasureCancel {
            measure: params.u64("measure")?,
        },
        "actions.list" => Call::ActionsList {
            inputs: params.optional("inputs", |at, value| inputs(&params, at, value))?,
        },
        "actions.read" => Call::ActionsRead {
            id: params.string("id")?,
        },
        "actions.write" => Call::ActionsWrite {
            definition: params.required("definition", |at, value| {
                value
                    .is_object()
                    .then(|| value.clone())
                    .ok_or_else(|| Fault::invalid_params(format!("{at} must be an object")))
            })?,
            script: params.optional("script", |at, value| {
                value
                    .is_object()
                    .then(|| value.clone())
                    .ok_or_else(|| Fault::invalid_params(format!("{at} must be an object")))
            })?,
            create: params.optional("create", Params::bool_of)?.unwrap_or(false),
        },
        "actions.setEnabled" => Call::ActionsSetEnabled {
            id: params.string("id")?,
            enabled: params.required("enabled", Params::bool_of)?,
        },
        "actions.delete" => Call::ActionsDelete {
            id: params.string("id")?,
        },
        "actions.duplicate" => Call::ActionsDuplicate {
            id: params.string("id")?,
        },
        "actions.import" => Call::ActionsImport {
            path: params.path("path")?,
        },
        "actions.export" => Call::ActionsExport {
            id: params.string("id")?,
            into: params.path("into")?,
        },
        "actions.examples" => Call::ActionsExamples,
        "actions.run" => Call::ActionsRun {
            id: params.string("id")?,
            inputs: params.required("inputs", |at, value| inputs(&params, at, value))?,
            parent: params.path("parent")?,
            background: params
                .optional("background", Params::bool_of)?
                .unwrap_or(false),
        },
        "actions.cancel" => Call::ActionsCancel {
            job: params.u64("job")?,
        },
        "actions.dismiss" => Call::ActionsDismiss {
            job: params.u64("job")?,
        },
        "ops.chmod" => Call::Chmod {
            path: params.path("path")?,
            bits: params.u64("bits")?,
        },
        "ops.rename" => Call::Rename {
            path: params.path("path")?,
            name: params.string("name")?,
            label: params.optional("label", Params::string_of)?,
        },
        "ops.create" => Call::Create {
            into: params.path("into")?,
            name: params.string("name")?,
            folder: match params.string("kind")?.as_str() {
                "folder" => true,
                "file" => false,
                other => return Err(unknown_value("kind", other)),
            },
            unique: params.optional("unique", Params::bool_of)?.unwrap_or(false),
            label: params.optional("label", Params::string_of)?,
        },
        "undo.history" => Call::UndoHistory,
        "undo.preview" => Call::UndoPreview,
        "undo.undo" => Call::Undo {
            overwrite: params
                .optional("overwrite", Params::bool_of)?
                .unwrap_or(false),
        },
        "undo.redo" => Call::Redo,
        "trash.list" => Call::TrashList,
        "trash.restore" => Call::TrashRestore {
            info: params.path("info")?,
        },
        "trash.delete" => Call::TrashDelete {
            info: params.path("info")?,
        },
        "trash.empty" => Call::TrashEmpty,
        "leftovers.recover" => Call::LeftoversRecover {
            path: params.path("path")?,
        },
        "leftovers.review" => Call::LeftoversReview {
            folder: params.path("folder")?,
            names: names(params)?,
        },
        "leftovers.sweep" => Call::LeftoversSweep {
            folder: params.path("folder")?,
            names: names(params)?,
        },
        other => return Err(Fault::unknown_method(other)),
    })
}

/// `showHidden` and `sort: {key, descending, foldersFirst}`, each optional.
fn options(params: &Params) -> Result<OptionsPatch, Fault> {
    let show_hidden = params.optional("showHidden", Params::bool_of)?;
    let Some(sort) = params.optional("sort", Params::object_of)? else {
        return Ok(OptionsPatch {
            show_hidden,
            ..OptionsPatch::default()
        });
    };
    let sort = params.nested(&params.at("sort"), sort);
    Ok(OptionsPatch {
        show_hidden,
        key: sort
            .optional("key", Params::string_of)?
            .map(|key| match key.as_str() {
                "name" => Ok(SortKey::Name),
                "size" => Ok(SortKey::Size),
                "modified" => Ok(SortKey::Modified),
                other => Err(unknown_value(&sort.at("key"), other)),
            })
            .transpose()?,
        descending: sort.optional("descending", Params::bool_of)?,
        folders_first: sort.optional("foldersFirst", Params::bool_of)?,
    })
}

fn names(params: &Params) -> Result<Vec<OsString>, Fault> {
    params.array("names", |at, value| {
        wire::plain_name(value).ok_or_else(|| not_a_name(at))
    })
}

fn job_request(params: &Params) -> Result<Request, Fault> {
    let items = params.array("items", |at, value| {
        let Value::Object(item) = value else {
            return Err(Fault::invalid_params(format!("{at} must be an object")));
        };
        job_item(&params.nested(at, item))
    })?;
    let names = match params.optional("names", Params::string_of)?.as_deref() {
        None | Some("fix") => NameMode::Fix,
        Some("keep") => NameMode::Keep,
        Some(other) => return Err(unknown_value("names", other)),
    };
    Ok(Request {
        into: params.path("into")?,
        items,
        names,
        record: None,
        durable: params.optional("durable", Params::bool_of)?,
    })
}

fn job_item(item: &Params) -> Result<Item, Fault> {
    let conflict = item.optional("conflict", Params::string_of)?;
    let conflict = conflict.as_deref();
    let action = match item.string("action")?.as_str() {
        "copy" => Action::Copy(match conflict {
            None | Some("fail") => Conflict::Fail,
            Some("keepBoth") => Conflict::KeepBoth,
            Some("merge") => Conflict::Merge,
            Some(other) => return Err(unknown_value(&item.at("conflict"), other)),
        }),
        "merge" if item.optional("move", Params::bool_of)? == Some(true) => {
            Action::MergeMoving(merge_plan(item)?)
        }
        "merge" => Action::Merge(merge_plan(item)?),
        "move" => match conflict {
            None | Some("fail") => Action::Move,
            Some("keepBoth") => Action::MoveKeepingBoth,
            Some("replace") => Action::MoveReplacing,
            Some(other) => return Err(unknown_value(&item.at("conflict"), other)),
        },
        "replace" => Action::Replace,
        "trash" => Action::Trash,
        "delete" => Action::Delete,
        "restore" => Action::Restore(item.optional("to", |at, value| {
            wire::absolute_path(value).ok_or_else(|| not_a_path(at))
        })?),
        other => return Err(unknown_value(&item.at("action"), other)),
    };
    Ok(Item {
        source: item.path("source")?,
        action,
        name: item.optional("name", |at, value| {
            wire::plain_name(value).ok_or_else(|| not_a_name(at))
        })?,
    })
}

/// `choices: {"<path in the folder>": "keepBoth"|"replace"|"skip"}` and `otherwise`.
fn merge_plan(item: &Params) -> Result<MergePlan, Fault> {
    let clash = |at: &str, value: &Value| match value.as_str() {
        Some("keepBoth") => Ok(Clash::KeepBoth),
        Some("replace") => Ok(Clash::Replace),
        Some("skip") => Ok(Clash::Skip),
        _ => Err(Fault::invalid_params(format!(
            "{at} must be \"keepBoth\", \"replace\" or \"skip\""
        ))),
    };
    let choices = match item.optional("choices", Params::object_of)? {
        None => Default::default(),
        Some(choices) => choices
            .iter()
            .map(|(path, choice)| {
                Ok((
                    PathBuf::from(path),
                    clash(&format!("{}.{path}", item.at("choices")), choice)?,
                ))
            })
            .collect::<Result<_, Fault>>()?,
    };
    Ok(MergePlan {
        choices,
        otherwise: item
            .optional("otherwise", clash)?
            .unwrap_or(Clash::KeepBoth),
    })
}

fn not_a_path(at: &str) -> Fault {
    Fault::invalid_params(format!("{at} must be an absolute path"))
}

fn not_a_name(at: &str) -> Fault {
    Fault::invalid_params(format!("{at} must be a plain name"))
}

fn unknown_value(at: &str, value: &str) -> Fault {
    Fault::invalid_params(format!("unknown {at} {value:?}"))
}

/// Selected items for an action: `[{path, folder?, type?}]`.
fn inputs(
    params: &Params<'_>,
    at: &str,
    value: &Value,
) -> Result<Vec<crate::actions::Input>, Fault> {
    let Value::Array(items) = value else {
        return Err(Fault::invalid_params(format!("{at} must be an array")));
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let at = format!("{at}[{index}]");
            let Value::Object(fields) = item else {
                return Err(Fault::invalid_params(format!("{at} must be an object")));
            };
            let input = params.nested(&at, fields);
            Ok(crate::actions::Input {
                path: input.path("path")?,
                folder: input.optional("folder", Params::bool_of)?.unwrap_or(false),
                content_type: input.optional("type", Params::string_of)?,
            })
        })
        .collect()
}

/// An object's fields, named in errors by their place in the request (`items[2].source`).
struct Params<'a> {
    fields: &'a Map<String, Value>,
    prefix: String,
}

impl<'a> Params<'a> {
    fn new(fields: &'a Map<String, Value>) -> Self {
        Self {
            fields,
            prefix: String::new(),
        }
    }

    /// The object at `at`, a field's full place in the request.
    fn nested<'b>(&self, at: &str, fields: &'b Map<String, Value>) -> Params<'b> {
        Params {
            fields,
            prefix: format!("{at}."),
        }
    }

    fn at(&self, name: &str) -> String {
        format!("{}{name}", self.prefix)
    }

    fn optional<T>(
        &self,
        name: &str,
        read: impl Fn(&str, &'a Value) -> Result<T, Fault>,
    ) -> Result<Option<T>, Fault> {
        match self.fields.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => read(&self.at(name), value).map(Some),
        }
    }

    fn required<T>(
        &self,
        name: &str,
        read: impl Fn(&str, &'a Value) -> Result<T, Fault>,
    ) -> Result<T, Fault> {
        self.optional(name, read)?
            .ok_or_else(|| Fault::invalid_params(format!("{} is required", self.at(name))))
    }

    fn path(&self, name: &str) -> Result<PathBuf, Fault> {
        self.required(name, |at, value| {
            wire::absolute_path(value).ok_or_else(|| not_a_path(at))
        })
    }

    fn string(&self, name: &str) -> Result<String, Fault> {
        self.required(name, Self::string_of)
    }

    fn u64(&self, name: &str) -> Result<u64, Fault> {
        self.required(name, Self::u64_of)
    }

    fn index(&self, name: &str) -> Result<usize, Fault> {
        self.u64(name)
            .map(|value| usize::try_from(value).unwrap_or(usize::MAX))
    }

    fn array<T>(
        &self,
        name: &str,
        read: impl Fn(&str, &'a Value) -> Result<T, Fault>,
    ) -> Result<Vec<T>, Fault> {
        self.required(name, |at, value| {
            value
                .as_array()
                .ok_or_else(|| Fault::invalid_params(format!("{at} must be an array")))?
                .iter()
                .enumerate()
                .map(|(index, element)| read(&format!("{at}[{index}]"), element))
                .collect()
        })
    }

    fn string_of(at: &str, value: &'a Value) -> Result<String, Fault> {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Fault::invalid_params(format!("{at} must be a string")))
    }

    fn strings_of(at: &str, value: &'a Value) -> Result<Vec<String>, Fault> {
        value
            .as_array()
            .ok_or_else(|| Fault::invalid_params(format!("{at} must be an array")))?
            .iter()
            .enumerate()
            .map(|(index, element)| Self::string_of(&format!("{at}[{index}]"), element))
            .collect()
    }

    fn bool_of(at: &str, value: &'a Value) -> Result<bool, Fault> {
        value
            .as_bool()
            .ok_or_else(|| Fault::invalid_params(format!("{at} must be a boolean")))
    }

    fn u64_of(at: &str, value: &'a Value) -> Result<u64, Fault> {
        value
            .as_u64()
            .ok_or_else(|| Fault::invalid_params(format!("{at} must be a non-negative integer")))
    }

    fn object_of(at: &str, value: &'a Value) -> Result<&'a Map<String, Value>, Fault> {
        value
            .as_object()
            .ok_or_else(|| Fault::invalid_params(format!("{at} must be an object")))
    }
}
