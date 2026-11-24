// SPDX-License-Identifier: MIT
//! The protocol's JSON Schema (`docs/protocol.schema.json`) against the backend: it names every
//! method the parser knows and every error kind, and the example of each request matches it. The
//! end-to-end tests in `protocol.rs` check every message they exchange against it too.

use std::collections::BTreeSet;

use asc_files_backend::{
    call::{self, Id},
    wire,
};
use serde_json::{Value, json};

mod common;

use common::{Checker, METHODS, schema, valid};

#[test]
fn the_schema_lists_every_method_the_parser_knows() {
    let listed: BTreeSet<&str> = schema()["$defs"]["methods"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(listed, METHODS.iter().copied().collect());
    for method in METHODS {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": method}).to_string();
        if let Err(fault) = call::parse(&line).call {
            assert_ne!(fault.code, wire::METHOD_NOT_FOUND, "{method}");
        }
    }
    let unknown = call::parse(r#"{"jsonrpc": "2.0", "id": 1, "method": "no.such"}"#);
    assert_eq!(unknown.call.err().unwrap().code, wire::METHOD_NOT_FOUND);
}

#[test]
fn the_schema_lists_every_error_kind() {
    assert_eq!(
        schema()["$defs"]["errorKind"]["enum"],
        json!(wire::ERROR_KINDS)
    );
}

#[test]
fn every_example_request_matches_the_schema() {
    let mut checker = Checker::default();
    for request in valid() {
        let method = request["method"].as_str().unwrap();
        checker.check("request", &request, &method);
        checker.check(
            &format!("methods/{method}/params"),
            &request["params"],
            &method,
        );
        assert_eq!(
            call::parse(&request.to_string()).id,
            Id::Request(request["id"].clone())
        );
    }
}

#[test]
fn refusals_match_the_schema() {
    let mut checker = Checker::default();
    let error = asc_files_engine::error::Error::new(
        asc_files_engine::error::ErrorKind::Exists,
        "“a” already exists",
    );
    let reply = wire::Fault::operation(&error).reply(json!(7));
    checker.check("reply", &reply, &"refusal");
    checker.check("operationError", &reply["error"], &"refusal");
    for line in [
        "{",
        "[]",
        r#"{"jsonrpc": "2.0", "id": 1, "method": "list.open"}"#,
    ] {
        let fault = call::parse(line).call.err().unwrap();
        checker.check("reply", &fault.reply(Value::Null), &line);
    }
}

#[test]
fn names_that_are_not_utf8_match_the_schema() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let mut checker = Checker::default();
    let name = wire::name(&OsString::from_vec(b"caf\xe9".to_vec()));
    checker.check("name", &name, &"bytes");
    checker.check("path", &json!({"bytes": "L2E="}), &"bytes");
}
