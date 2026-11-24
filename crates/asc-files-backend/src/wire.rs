// SPDX-License-Identifier: MIT
//! How values travel. Paths and names are JSON strings when they are UTF-8 and
//! `{"bytes": "<base64>", "display": "<lossy text>"}` when they are not, so no name is lost on the
//! way to the client and back; either form is accepted where a path or name is asked for. Errors
//! carry a kind for code to branch on and a sentence to show.

use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use asc_files_engine::error::{Error, ErrorKind};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// The operation was understood and refused; `data.kind` says why.
pub const OPERATION_FAILED: i64 = 1;

/// A JSON-RPC error: `{code, message, data?}`.
#[derive(Clone, Debug, PartialEq)]
pub struct Fault {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl Fault {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message)
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    pub fn unknown_method(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("unknown method {method}"))
    }

    /// An operation the engine refused: the message is its sentence for the user, `data.kind`
    /// its kind.
    pub fn operation(error: &Error) -> Self {
        Self {
            code: OPERATION_FAILED,
            message: error.message().to_owned(),
            data: Some(json!({"kind": kind_name(error.kind())})),
        }
    }

    pub fn reply(&self, id: Value) -> Value {
        let mut error = json!({"code": self.code, "message": self.message});
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        json!({"jsonrpc": "2.0", "id": id, "error": error})
    }
}

impl From<Error> for Fault {
    fn from(error: Error) -> Self {
        Self::operation(&error)
    }
}

pub fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn reply(id: Value, outcome: Result<Value, Fault>) -> Value {
    match outcome {
        Ok(value) => result(id, value),
        Err(fault) => fault.reply(id),
    }
}

pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

pub fn path(path: &Path) -> Value {
    name(path.as_os_str())
}

pub fn name(name: &OsStr) -> Value {
    match name.to_str() {
        Some(text) => json!(text),
        None => json!({
            "bytes": STANDARD.encode(name.as_bytes()),
            "display": name.to_string_lossy(),
        }),
    }
}

/// A string, or `{"bytes": "<base64>"}`; never empty and never holding a NUL, which no path or
/// name on Linux can.
pub fn decode(value: &Value) -> Option<OsString> {
    let decoded = match value {
        Value::String(text) => OsString::from(text),
        Value::Object(object) => {
            let bytes = object.get("bytes")?.as_str()?;
            OsString::from_vec(STANDARD.decode(bytes).ok()?)
        }
        _ => return None,
    };
    (!decoded.is_empty() && !decoded.as_bytes().contains(&0)).then_some(decoded)
}

/// An absolute path: a relative one would resolve against wherever the backend was started.
pub fn absolute_path(value: &Value) -> Option<PathBuf> {
    let path = PathBuf::from(decode(value)?);
    path.is_absolute().then_some(path)
}

/// One name within a folder: no `/`, and not `.` or `..`, so it cannot reach outside it.
pub fn plain_name(value: &Value) -> Option<OsString> {
    let name = decode(value)?;
    let plain = !name.as_bytes().contains(&b'/') && name != "." && name != "..";
    plain.then_some(name)
}

pub fn error(error: &Error) -> Value {
    json!({"kind": kind_name(error.kind()), "message": error.message()})
}

/// Every kind [`kind_name`] gives, as the schema lists them.
pub const ERROR_KINDS: &[&str] = &[
    "failed",
    "notFound",
    "exists",
    "isDirectory",
    "notDirectory",
    "notEmpty",
    "notRegularFile",
    "notSymbolicLink",
    "filenameTooLong",
    "invalidFilename",
    "tooManyLinks",
    "noSpace",
    "invalidArgument",
    "permissionDenied",
    "notSupported",
    "notMounted",
    "cancelled",
    "readOnly",
    "timedOut",
    "busy",
    "wouldBlock",
    "tooManyOpenFiles",
    "invalidData",
    "brokenPipe",
    "noSuchDevice",
    "symlinkUnheld",
];

/// The wire name of an error kind; spelled out so renaming a variant cannot change the protocol.
pub fn kind_name(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Failed => "failed",
        ErrorKind::NotFound => "notFound",
        ErrorKind::Exists => "exists",
        ErrorKind::IsDirectory => "isDirectory",
        ErrorKind::NotDirectory => "notDirectory",
        ErrorKind::NotEmpty => "notEmpty",
        ErrorKind::NotRegularFile => "notRegularFile",
        ErrorKind::NotSymbolicLink => "notSymbolicLink",
        ErrorKind::FilenameTooLong => "filenameTooLong",
        ErrorKind::InvalidFilename => "invalidFilename",
        ErrorKind::TooManyLinks => "tooManyLinks",
        ErrorKind::NoSpace => "noSpace",
        ErrorKind::InvalidArgument => "invalidArgument",
        ErrorKind::PermissionDenied => "permissionDenied",
        ErrorKind::NotSupported => "notSupported",
        ErrorKind::NotMounted => "notMounted",
        ErrorKind::Cancelled => "cancelled",
        ErrorKind::ReadOnly => "readOnly",
        ErrorKind::TimedOut => "timedOut",
        ErrorKind::Busy => "busy",
        ErrorKind::WouldBlock => "wouldBlock",
        ErrorKind::TooManyOpenFiles => "tooManyOpenFiles",
        ErrorKind::InvalidData => "invalidData",
        ErrorKind::BrokenPipe => "brokenPipe",
        ErrorKind::NoSuchDevice => "noSuchDevice",
        ErrorKind::SymlinkUnheld => "symlinkUnheld",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_names_stay_strings_and_others_go_as_bytes() {
        assert_eq!(name(OsStr::new("café")), json!("café"));
        let raw = OsString::from_vec(b"caf\xe9".to_vec());
        let sent = name(&raw);
        assert_eq!(sent["bytes"], json!("Y2Fm6Q=="));
        assert_eq!(sent["display"], json!("caf\u{fffd}"));
        assert_eq!(decode(&sent), Some(raw));
    }

    #[test]
    fn paths_must_be_absolute_and_names_plain() {
        assert_eq!(absolute_path(&json!("/a/b")), Some(PathBuf::from("/a/b")));
        for refused in [
            json!("a/b"),
            json!(""),
            json!("/a\u{0}b"),
            json!(3),
            json!({"bytes": "!"}),
        ] {
            assert_eq!(absolute_path(&refused), None, "{refused}");
        }
        assert_eq!(plain_name(&json!("x.txt")), Some(OsString::from("x.txt")));
        for refused in [json!("a/b"), json!("."), json!(".."), json!("")] {
            assert_eq!(plain_name(&refused), None, "{refused}");
        }
    }

    #[test]
    fn every_kind_has_a_listed_name() {
        use ErrorKind::*;
        let kinds = [
            Failed,
            NotFound,
            Exists,
            IsDirectory,
            NotDirectory,
            NotEmpty,
            NotRegularFile,
            NotSymbolicLink,
            FilenameTooLong,
            InvalidFilename,
            TooManyLinks,
            NoSpace,
            InvalidArgument,
            PermissionDenied,
            NotSupported,
            NotMounted,
            Cancelled,
            ReadOnly,
            TimedOut,
            Busy,
            WouldBlock,
            TooManyOpenFiles,
            InvalidData,
            BrokenPipe,
            NoSuchDevice,
            SymlinkUnheld,
        ];
        let names: Vec<_> = kinds.into_iter().map(kind_name).collect();
        assert_eq!(names, ERROR_KINDS);
    }
}
