// SPDX-License-Identifier: MIT
//! Fuzzes the protocol parser with arbitrary lines, arbitrary JSON and valid requests with fields
//! swapped for arbitrary values. Only [`call::parse`] runs: nothing is carried out, so no input
//! can touch a file. Every line must come back as a call or as one of the protocol's errors, and
//! every path in a call must be absolute and every name plain.

mod common;

use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

use asc_files_backend::{
    call::{self, Call, Id},
    wire,
};
use common::{METHODS, valid};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>().prop_map(|f| json!(f)),
        ".{0,12}".prop_map(Value::from),
        prop::sample::select(vec![
            "/", "/a", "..", ".", "a/b", "", "/a\u{0}b", "keepBoth", "copy", "folder"
        ])
        .prop_map(Value::from),
        "[A-Za-z0-9+/=]{0,12}".prop_map(|bytes| json!({"bytes": bytes})),
    ];
    leaf.prop_recursive(4, 32, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::vec((".{0,8}", inner), 0..6)
                .prop_map(|fields| Value::Object(fields.into_iter().collect::<Map<_, _>>())),
        ]
    })
}

/// Replaces the value at one place in `value` (picked by `at` among all places) with `with`.
fn replace_somewhere(value: &mut Value, at: usize, with: Value) {
    fn count(value: &Value) -> usize {
        1 + match value {
            Value::Array(items) => items.iter().map(count).sum(),
            Value::Object(fields) => fields.values().map(count).sum(),
            _ => 0,
        }
    }
    fn walk(value: &mut Value, mut at: usize, with: Value) {
        if at == 0 {
            *value = with;
            return;
        }
        at -= 1;
        let children: Vec<&mut Value> = match value {
            Value::Array(items) => items.iter_mut().collect(),
            Value::Object(fields) => fields.values_mut().collect(),
            _ => Vec::new(),
        };
        for child in children {
            let size = count(child);
            if at < size {
                return walk(child, at, with);
            }
            at -= size;
        }
    }
    let at = at % count(value);
    walk(value, at, with);
}

fn check(line: &str) {
    let incoming = call::parse(line);
    match incoming.call {
        Ok(call) => {
            assert!(matches!(incoming.id, Id::Request(_) | Id::Notification));
            check_call(&call);
        }
        Err(fault) => {
            assert!(
                [
                    wire::PARSE_ERROR,
                    wire::INVALID_REQUEST,
                    wire::METHOD_NOT_FOUND,
                    wire::INVALID_PARAMS
                ]
                .contains(&fault.code),
                "{fault:?}"
            );
            assert!(!fault.message.is_empty());
            // The reply it makes is one JSON line.
            let reply = fault.reply(Value::Null).to_string();
            assert!(!reply.contains('\n'));
        }
    }
}

fn absolute(path: &Path) {
    assert!(path.is_absolute(), "{path:?}");
    assert!(!path.as_os_str().as_bytes().contains(&0), "{path:?}");
}

fn plain(name: &OsStr) {
    let bytes = name.as_bytes();
    assert!(
        !bytes.is_empty() && !bytes.contains(&b'/') && !bytes.contains(&0),
        "{name:?}"
    );
    assert!(name != "." && name != "..", "{name:?}");
}

fn check_call(call: &Call) {
    match call {
        Call::ListOpen { path, .. }
        | Call::Rename { path, .. }
        | Call::LeftoversRecover { path } => absolute(path),
        Call::Create { into, .. } => absolute(into),
        Call::TrashRestore { info } | Call::TrashDelete { info } => absolute(info),
        Call::JobsClashes { into, sources } => {
            absolute(into);
            sources.iter().for_each(|source| absolute(source));
        }
        Call::JobsStart { request, .. } => {
            absolute(&request.into);
            for item in &request.items {
                absolute(&item.source);
                if let Some(name) = &item.name {
                    plain(name);
                }
                if let asc_files_engine::jobs::Action::Restore(Some(to)) = &item.action {
                    absolute(to);
                }
            }
        }
        Call::LeftoversReview { folder, names } | Call::LeftoversSweep { folder, names } => {
            absolute(folder);
            names.iter().for_each(|name| plain(name));
        }
        _ => {}
    }
}

#[test]
fn every_valid_request_parses() {
    for request in valid() {
        let incoming = call::parse(&request.to_string());
        assert!(
            incoming.call.is_ok(),
            "{request}: {:?}",
            incoming.call.err()
        );
        assert_eq!(incoming.id, Id::Request(request["id"].clone()));
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2_000, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_lines_are_answered(line in ".{0,200}") {
        check(&line);
    }

    #[test]
    fn arbitrary_json_is_answered(message in any_json()) {
        check(&message.to_string());
    }

    #[test]
    fn known_methods_with_arbitrary_params_are_answered(
        method in prop::sample::select(METHODS),
        params in any_json(),
        id in any_json(),
    ) {
        check(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string());
    }

    #[test]
    fn valid_requests_with_one_value_swapped_are_answered(
        which in 0..METHODS.len(),
        at in any::<usize>(),
        with in any_json(),
    ) {
        let mut request = valid().swap_remove(which);
        replace_somewhere(&mut request, at, with);
        check(&request.to_string());
    }

    #[test]
    fn truncated_requests_are_answered(which in 0..METHODS.len(), cut in any::<usize>()) {
        let line = valid()[which].to_string();
        let cut = (0..=cut % (line.len() + 1)).rev().find(|cut| line.is_char_boundary(*cut)).unwrap();
        check(&line[..cut]);
    }
}
