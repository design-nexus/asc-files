// SPDX-License-Identifier: MIT
//! What the protocol tests share: an example of every request, and the protocol's JSON Schema.
#![allow(dead_code)]

use std::{collections::HashMap, sync::OnceLock};

use serde_json::{Value, json};

pub const METHODS: &[&str] = &[
    "initialize",
    "shutdown",
    "list.open",
    "list.window",
    "list.reorder",
    "list.refresh",
    "list.close",
    "jobs.start",
    "jobs.cancel",
    "jobs.clashes",
    "jobs.plan",
    "ops.rename",
    "ops.create",
    "undo.history",
    "undo.preview",
    "undo.undo",
    "undo.redo",
    "trash.list",
    "trash.restore",
    "trash.delete",
    "trash.empty",
    "leftovers.recover",
    "leftovers.review",
    "leftovers.sweep",
];

/// One valid request per method, with every optional field given.
pub fn valid() -> Vec<Value> {
    let params = [
        json!({"protocolVersion": 2}),
        json!({}),
        json!({"path": "/a", "showHidden": true, "sort": {"key": "size", "descending": true, "foldersFirst": false}}),
        json!({"listing": 1, "start": 0, "count": 10}),
        json!({"listing": 1, "showHidden": false, "sort": {"key": "modified"}}),
        json!({"listing": 1}),
        json!({"listing": 1}),
        json!({"into": "/b", "names": "keep", "label": "Copy", "durable": true, "items": [
            {"source": "/a/x", "action": "copy", "conflict": "keepBoth", "name": "y"},
            {"source": "/a/m", "action": "merge", "move": true, "choices": {"c/d": "replace"}, "otherwise": "skip"},
            {"source": "/a/z", "action": "move", "conflict": "replace"},
            {"source": "/a/t", "action": "restore", "to": "/a/u"},
            {"source": {"bytes": "L2EvdyVl"}, "action": "trash"},
        ]}),
        json!({"job": 1}),
        json!({"into": "/b", "sources": ["/a/x", {"bytes": "L2Evxf8="}]}),
        json!({"into": "/b", "sources": ["/a/x"]}),
        json!({"path": "/a/x", "name": "y", "label": "Rename"}),
        json!({"into": "/a", "name": "New Folder", "kind": "folder", "unique": true, "label": "New Folder"}),
        json!({}),
        json!({}),
        json!({"overwrite": true}),
        json!({}),
        json!({}),
        json!({"info": "/t/info/x.trashinfo"}),
        json!({"info": "/t/info/x.trashinfo"}),
        json!({}),
        json!({"path": "/a/.asc-files-originals-1"}),
        json!({"folder": "/a", "names": [".asc-files-originals-1", {"bytes": "/w=="}]}),
        json!({"folder": "/a", "names": [".asc-files-probe-1"]}),
    ];
    METHODS
        .iter()
        .zip(params)
        .enumerate()
        .map(|(id, (method, params))| {
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        })
        .collect()
}

/// The protocol's schema, `docs/protocol.schema.json`.
pub fn schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../docs/protocol.schema.json"))
            .expect("the schema is JSON")
    })
}

/// Checks values against parts of the schema, each compiled once.
#[derive(Default)]
pub struct Checker {
    validators: HashMap<String, jsonschema::Validator>,
}

impl Checker {
    /// Panics unless `value` matches the schema at `pointer` (`methods/list.open/result`, say).
    pub fn check(&mut self, pointer: &str, value: &Value, context: &dyn std::fmt::Display) {
        let validator = self
            .validators
            .entry(pointer.to_owned())
            .or_insert_with(|| {
                assert!(
                    schema()["$defs"].pointer(&format!("/{pointer}")).is_some(),
                    "the schema has no $defs/{pointer} ({context})"
                );
                jsonschema::validator_for(&json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "$defs": schema()["$defs"],
                    "$ref": format!("#/$defs/{pointer}"),
                }))
                .expect("the schema compiles")
            });
        let errors: Vec<String> = validator
            .iter_errors(value)
            .map(|error| format!("{} at {}", error, error.instance_path))
            .collect();
        assert!(
            errors.is_empty(),
            "{pointer} does not match the schema ({context}):\n{}\n{value}",
            errors.join("\n")
        );
    }

    /// A reply to `method`, sent with `params`: the envelope, its result or a refusal, and the
    /// params of a request that succeeded.
    pub fn reply(&mut self, method: &str, params: &Value, reply: &Value) {
        self.check("reply", reply, &method);
        if let Some(result) = reply.get("result") {
            self.check(&format!("methods/{method}/params"), params, &method);
            self.check(&format!("methods/{method}/result"), result, &method);
        } else if reply["error"]["code"] == 1 {
            self.check("operationError", &reply["error"], &method);
        }
    }

    /// A notification: the envelope and its params.
    pub fn notification(&mut self, message: &Value) {
        self.check("notification", message, &"notification");
        let method = message["method"].as_str().unwrap_or_default();
        self.check(
            &format!("notifications/{method}"),
            &message["params"],
            &method,
        );
    }
}
