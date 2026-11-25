// SPDX-License-Identifier: MIT
//! The Files backend (`asc-files-backend`): JSON-RPC 2.0, one message per line, on stdin and
//! stdout. stdout carries only protocol lines; diagnostics go to stderr. See `docs/PROTOCOL.md`.

use std::{
    io::{BufRead, Write},
    sync::mpsc,
    thread,
};

use serde_json::Value;

pub mod actions;
pub mod call;
mod listing;
mod session;
pub mod wire;

pub use session::Session;

/// Bumped on any change a client must know about; `initialize` reports it.
pub const PROTOCOL_VERSION: u64 = 2;

/// Serves one client on `input` and `output` until it shuts the session down or closes `input`.
pub fn serve(input: impl BufRead, output: impl Write + Send + 'static) {
    let (out, lines) = mpsc::channel::<Value>();
    let writer = thread::spawn(move || {
        let mut output = output;
        for message in lines {
            if writeln!(output, "{message}")
                .and_then(|()| output.flush())
                .is_err()
            {
                break;
            }
        }
    });
    let mut session = Session::new(out.clone());
    for line in input.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        session.handle(call::parse(&line));
        if session.shutting_down() {
            break;
        }
    }
    session.finish();
    drop(session);
    drop(out);
    let _ = writer.join();
}
