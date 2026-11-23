// SPDX-License-Identifier: MIT
//! Drives the real backend binary over its stdio protocol.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

struct Backend {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<Value>,
    next: u64,
}

impl Backend {
    fn start() -> Self {
        let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME set by the test script");
        assert!(
            data.contains("asc-files-test-home"),
            "refusing to touch a real Trash: {data}"
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_asc-files-backend"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("backend starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.expect("utf-8 protocol line");
                tx.send(serde_json::from_str(&line).expect("every stdout line is JSON"))
                    .ok();
            }
        });
        Self {
            child,
            stdin,
            lines,
            next: 0,
        }
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Sends a request and returns its response; notifications that arrive first are kept.
    fn call(&mut self, method: &str, params: Value, notes: &mut Vec<Value>) -> Value {
        self.next += 1;
        let id = self.next;
        self.send_raw(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
        );
        loop {
            let message = self
                .lines
                .recv_timeout(Duration::from_secs(30))
                .expect("a reply");
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
            notes.push(message);
        }
    }

    fn until(&self, notes: &mut Vec<Value>, done: impl Fn(&Value) -> bool) {
        while !notes.iter().any(&done) {
            notes.push(
                self.lines
                    .recv_timeout(Duration::from_secs(30))
                    .expect("a notification"),
            );
        }
    }
}

#[test]
fn the_protocol_end_to_end() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();

    let early = backend.call("list.open", json!({"path": "/"}), &mut notes);
    assert_eq!(early["error"]["code"], -32600, "{early}");

    let init = backend.call("initialize", json!({}), &mut notes);
    assert_eq!(init["result"]["protocolVersion"], 1, "{init}");

    let unknown = backend.call("no.such", json!({}), &mut notes);
    assert_eq!(unknown["error"]["code"], -32601, "{unknown}");
    backend.send_raw("{not json");
    let parse = backend.lines.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(parse["error"]["code"], -32700, "{parse}");

    // Listing: one directory read at open, stats only for the window asked for.
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let big = root.path().join("big");
    fs::create_dir(&big).unwrap();
    for i in 0..20_000 {
        fs::File::create(big.join(format!("f{i:05}.txt"))).unwrap();
    }
    let started = Instant::now();
    let open = backend.call("list.open", json!({"path": big}), &mut notes);
    let open_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(open["result"]["count"], 20_000, "{open}");
    let listing = open["result"]["listing"].clone();
    let started = Instant::now();
    let window = backend.call(
        "list.window",
        json!({"listing": listing, "start": 19_900, "count": 200}),
        &mut notes,
    );
    let window_ms = started.elapsed().as_secs_f64() * 1000.0;
    let rows = window["result"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 100, "clamped to the end");
    assert_eq!(rows[0]["name"], "f19900.txt");
    assert_eq!(rows[0]["kind"], "file");
    eprintln!(
        "OBSERVED list.open 20k: {open_ms:.1} ms, list.window(200 at 19900): {window_ms:.1} ms (debug build, incl. pipe round trip)"
    );

    // The user's paste: Replace where only one of two items clashes.
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("a.txt"), b"new a").unwrap();
    fs::write(source.join("b.txt"), b"new b").unwrap();
    fs::write(dest.join("a.txt"), b"old a").unwrap();
    let start = backend.call(
        "jobs.start",
        json!({"into": dest, "items": [
            {"source": source.join("b.txt"), "action": "replace"},
            {"source": source.join("a.txt"), "action": "replace"},
        ]}),
        &mut notes,
    );
    let job = start["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    let mine: Vec<&Value> = notes.iter().filter(|n| n["params"]["job"] == job).collect();
    assert_eq!(mine.first().unwrap()["method"], "job.started");
    let done: Vec<&Value> = mine
        .iter()
        .copied()
        .filter(|n| n["method"] == "job.itemDone")
        .collect();
    assert_eq!(done.len(), 2);
    assert!(
        done.iter()
            .all(|n| n["params"]["outcome"]["status"] == "done"),
        "{done:?}"
    );
    let finished = mine.last().unwrap();
    assert_eq!(finished["method"], "job.finished");
    assert_eq!(finished["params"]["done"], 2);
    for note in &done {
        let outcome = &note["params"]["outcome"];
        assert_eq!(outcome["renamed"], json!([]), "{outcome}");
        assert_eq!(outcome["skipped"], json!([]), "{outcome}");
    }

    // Names are fixed for the destination unless the caller asks to keep them; nothing else.
    let unknown = backend.call(
        "jobs.start",
        json!({"into": dest, "names": "mangle", "items": []}),
        &mut notes,
    );
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");
    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"new a");
    assert_eq!(fs::read(dest.join("b.txt")).unwrap(), b"new b");

    // A refused item reports its kind and sentence, and the batch still finishes.
    let refused = backend.call(
        "jobs.start",
        json!({"into": dest, "items": [{"source": source.join("a.txt"), "action": "copy", "conflict": "fail"}]}),
        &mut notes,
    );
    let job = refused["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    let failed = notes
        .iter()
        .find(|n| n["method"] == "job.itemDone" && n["params"]["job"] == job)
        .unwrap();
    assert_eq!(failed["params"]["outcome"]["status"], "failed");
    assert_eq!(
        failed["params"]["outcome"]["error"]["kind"], "Exists",
        "{failed}"
    );
    assert!(
        failed["params"]["outcome"]["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty())
    );

    // Trash round trip. trash.empty is never called: it would empty every Trash this user sees.
    let doomed = root.path().join("doomed.txt");
    fs::write(&doomed, b"bye").unwrap();
    let start = backend.call(
        "jobs.start",
        json!({"into": root.path(), "items": [{"source": doomed, "action": "trash"}]}),
        &mut notes,
    );
    let job = start["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    assert!(!doomed.exists());
    let find = |backend: &mut Backend, notes: &mut Vec<Value>| {
        let listed = backend.call("trash.list", json!({}), notes);
        listed["result"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["original"] == json!(doomed))
            .cloned()
    };
    let entry = find(&mut backend, &mut notes).expect("listed in the Trash");
    let restored = backend.call("trash.restore", json!({"info": entry["info"]}), &mut notes);
    assert_eq!(restored["result"]["path"], json!(doomed), "{restored}");
    assert_eq!(fs::read(&doomed).unwrap(), b"bye");
    let stale = backend.call("trash.restore", json!({"info": entry["info"]}), &mut notes);
    assert_eq!(
        stale["error"]["code"], 1,
        "a restored entry is gone: {stale}"
    );
    let start = backend.call(
        "jobs.start",
        json!({"into": root.path(), "items": [{"source": doomed, "action": "trash"}]}),
        &mut notes,
    );
    let job = start["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    let entry = find(&mut backend, &mut notes).unwrap();
    let deleted = backend.call("trash.delete", json!({"info": entry["info"]}), &mut notes);
    assert!(deleted.get("result").is_some(), "{deleted}");
    assert!(find(&mut backend, &mut notes).is_none(), "deleted for good");

    let bye = backend.call("shutdown", json!({}), &mut notes);
    assert!(bye.get("result").is_some(), "{bye}");
    drop(backend.stdin);
    let status = backend.child.wait().unwrap();
    assert!(status.success(), "clean exit: {status}");
}
