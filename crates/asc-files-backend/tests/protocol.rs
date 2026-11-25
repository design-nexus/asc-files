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

mod common;

/// Every reply and notification a test receives is checked against the protocol's schema, and
/// the params of every request that succeeded.
struct Backend {
    child: Child,
    /// Its own session folder, so tests running side by side keep separate undo histories.
    _runtime: tempfile::TempDir,
    stdin: ChildStdin,
    lines: mpsc::Receiver<Value>,
    next: u64,
    checker: common::Checker,
}

impl Backend {
    fn start() -> Self {
        let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME set by the test script");
        assert!(
            data.contains("asc-files-test-home"),
            "refusing to touch a real Trash: {data}"
        );
        let runtime = tempfile::tempdir_in(std::env::var("XDG_RUNTIME_DIR").unwrap()).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_asc-files-backend"))
            .env("XDG_RUNTIME_DIR", runtime.path())
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
            _runtime: runtime,
            stdin,
            lines,
            next: 0,
            checker: common::Checker::default(),
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
            let message = self.receive("a reply");
            if message.get("id") == Some(&json!(id)) {
                self.checker.reply(method, &params, &message);
                return message;
            }
            notes.push(message);
        }
    }

    fn until(&mut self, notes: &mut Vec<Value>, done: impl Fn(&Value) -> bool) {
        while !notes.iter().any(&done) {
            let message = self.receive("a notification");
            notes.push(message);
        }
    }

    /// The next line, its schema checked if it is a notification.
    fn receive(&mut self, what: &str) -> Value {
        let message = self
            .lines
            .recv_timeout(Duration::from_secs(30))
            .expect(what);
        if message.get("method").is_some() {
            self.checker.notification(&message);
        }
        message
    }
}

#[test]
fn the_protocol_end_to_end() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();

    let early = backend.call("list.open", json!({"path": "/"}), &mut notes);
    assert_eq!(early["error"]["code"], -32600, "{early}");

    let init = backend.call("initialize", json!({}), &mut notes);
    assert_eq!(init["result"]["protocolVersion"], 2, "{init}");

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
        failed["params"]["outcome"]["error"]["kind"], "exists",
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
        (&stale["error"]["code"], &stale["error"]["data"]["kind"]),
        (&json!(1), &json!("notFound")),
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

#[test]
fn a_listing_follows_its_folder_and_answers_on_its_own() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let folder = root.path().join("watched");
    fs::create_dir(&folder).unwrap();
    for name in ["b", "a", ".dot"] {
        fs::write(folder.join(name), name).unwrap();
    }

    let open = backend.call(
        "list.open",
        json!({"path": folder, "sort": {"key": "name", "descending": true}}),
        &mut notes,
    );
    let result = &open["result"];
    assert_eq!(
        (
            &result["count"],
            &result["generation"],
            &result["followsChanges"]
        ),
        (&json!(2), &json!(1), &json!(true)),
        "{open}"
    );
    let listing = result["listing"].clone();
    let names = |backend: &mut Backend, notes: &mut Vec<Value>| -> Vec<String> {
        let window = backend.call(
            "list.window",
            json!({"listing": listing, "start": 0, "count": 50}),
            notes,
        );
        window["result"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&mut backend, &mut notes), ["b", "a"]);

    // Reordering keeps what the request leaves out: still descending, now with hidden names.
    let reordered = backend.call(
        "list.reorder",
        json!({"listing": listing, "showHidden": true}),
        &mut notes,
    );
    assert_eq!(reordered["result"]["generation"], 2, "{reordered}");
    assert_eq!(names(&mut backend, &mut notes), ["b", "a", ".dot"]);
    let bad = backend.call(
        "list.reorder",
        json!({"listing": listing, "sort": {"key": "colour"}}),
        &mut notes,
    );
    assert_eq!(bad["error"]["code"], -32602, "{bad}");

    // An outside change is read again and told as a new generation.
    fs::write(folder.join("c"), "c").unwrap();
    backend.until(&mut notes, |n| {
        n["method"] == "list.changed" && n["params"]["listing"] == listing
    });
    let changed = notes
        .iter()
        .find(|n| n["method"] == "list.changed")
        .unwrap();
    assert_eq!(changed["params"]["count"], 4, "{changed}");
    assert!(changed["params"]["generation"].as_u64().unwrap() >= 3);
    assert_eq!(names(&mut backend, &mut notes), ["c", "b", "a", ".dot"]);

    // The folder going sends the view to the nearest folder left.
    fs::remove_dir_all(&folder).unwrap();
    backend.until(&mut notes, |n| n["method"] == "list.gone");
    let gone = notes.iter().find(|n| n["method"] == "list.gone").unwrap();
    assert_eq!(gone["params"]["nearest"], json!(root.path()), "{gone}");

    let closed = backend.call("list.close", json!({"listing": listing}), &mut notes);
    assert_eq!(closed["result"], Value::Null);
    let after = backend.call(
        "list.window",
        json!({"listing": listing, "start": 0, "count": 1}),
        &mut notes,
    );
    assert_eq!(after["error"]["code"], -32602, "{after}");
    let missing = backend.call(
        "list.open",
        json!({"path": root.path().join("missing")}),
        &mut notes,
    );
    assert_eq!(missing["error"]["code"], 1, "{missing}");
    assert_eq!(missing["error"]["data"]["kind"], "notFound", "{missing}");

    let shutdown = backend.call("shutdown", json!({}), &mut notes);
    assert_eq!(shutdown["result"], Value::Null);
    assert!(backend.child.wait().unwrap().success());
}

#[test]
fn a_batch_is_filed_for_undo_and_undone_and_redone_over_the_wire() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    let init = backend.call("initialize", json!({}), &mut notes);
    assert!(
        init["result"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("undo")),
        "{init}"
    );
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("a.txt"), b"a").unwrap();
    fs::write(source.join("b.txt"), b"b").unwrap();

    let empty = backend.call("undo.undo", json!({}), &mut notes);
    assert_eq!(empty["error"]["code"], 1, "{empty}");
    assert_eq!(empty["error"]["data"]["kind"], "notFound", "{empty}");
    assert_eq!(
        empty["error"]["message"], "There is nothing to undo",
        "{empty}"
    );

    let start = backend.call(
        "jobs.start",
        json!({"into": dest, "items": [
            {"source": source.join("a.txt"), "action": "move"},
            {"source": source.join("b.txt"), "action": "move"},
        ]}),
        &mut notes,
    );
    let job = start["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    let finished = notes
        .iter()
        .find(|n| n["method"] == "job.finished")
        .unwrap();
    let entry = finished["params"]["undo"].clone();
    assert!(entry.is_u64(), "{finished}");

    let history = backend.call("undo.history", json!({}), &mut notes);
    assert_eq!(
        history["result"],
        json!({"undo": {"id": entry, "label": "Move 2 items"}, "redo": null}),
        "{history}"
    );

    let undone = backend.call("undo.undo", json!({}), &mut notes);
    assert_eq!(undone["result"]["done"], 2, "{undone}");
    assert_eq!(undone["result"]["left"], json!([]));
    assert!(source.join("a.txt").exists() && source.join("b.txt").exists());
    assert!(!dest.join("a.txt").exists());

    let redone = backend.call("undo.redo", json!({}), &mut notes);
    assert_eq!(redone["result"]["done"], 2, "{redone}");
    assert!(dest.join("a.txt").exists() && !source.join("a.txt").exists());

    let shutdown = backend.call("shutdown", json!({}), &mut notes);
    assert_eq!(shutdown["result"], Value::Null);
    assert!(backend.child.wait().unwrap().success());
}

#[test]
fn a_merge_is_planned_from_its_clashes_and_done_as_planned() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    for (path, content) in [
        ("source/photos/a.jpg", "new a"),
        ("source/photos/b.jpg", "new b"),
        ("source/photos/c.jpg", "new c"),
        ("dest/photos/a.jpg", "old a"),
        ("dest/photos/b.jpg", "old b"),
        ("dest/photos/c.jpg", "old c"),
    ] {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    let (source, dest) = (root.path().join("source/photos"), root.path().join("dest"));

    let clashes = backend.call(
        "jobs.clashes",
        json!({"into": dest, "sources": [source]}),
        &mut notes,
    );
    let paths: Vec<&str> = clashes["result"]["items"][0]["clashes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|clash| clash["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["a.jpg", "b.jpg", "c.jpg"], "{clashes}");

    let start = backend.call(
        "jobs.start",
        json!({"into": dest, "durable": true, "items": [{
            "source": source,
            "action": "merge",
            "choices": {"a.jpg": "replace", "b.jpg": "skip"},
            "otherwise": "keepBoth",
        }]}),
        &mut notes,
    );
    let job = start["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    let finished = notes
        .iter()
        .find(|n| n["method"] == "job.finished" && n["params"]["job"] == job)
        .unwrap();
    assert_eq!(finished["params"]["done"], 1, "{finished}");
    assert_eq!(finished["params"]["durable"], true);
    assert_eq!(finished["params"]["settled"], true, "{finished}");
    let read = |name: &str| fs::read_to_string(dest.join("photos").join(name)).unwrap();
    assert_eq!(read("a.jpg"), "new a");
    assert_eq!(read("b.jpg"), "old b");
    assert_eq!(read("c.jpg"), "old c");
    assert_eq!(read("c (1).jpg"), "new c");

    let bad = backend.call(
        "jobs.start",
        json!({"into": dest, "items": [{"source": source, "action": "merge", "otherwise": "shred"}]}),
        &mut notes,
    );
    assert_eq!(bad["error"]["code"], -32602, "{bad}");

    let undone = backend.call("undo.undo", json!({}), &mut notes);
    assert_eq!(undone["result"]["done"], 2, "{undone}");
    assert_eq!(read("a.jpg"), "old a");
    assert!(!dest.join("photos/c (1).jpg").exists());

    // The merge's replaced original went to the test home's Trash and came back; nothing is left there.
    let shutdown = backend.call("shutdown", json!({}), &mut notes);
    assert_eq!(shutdown["result"], Value::Null);
    assert!(backend.child.wait().unwrap().success());
}

#[test]
fn originals_an_interrupted_replace_left_are_reviewed_and_put_back() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let container = ".asc-files-originals-0123abcd-4567-89ab-cdef-0123456789ab";
    fs::create_dir(root.path().join(container)).unwrap();
    fs::write(root.path().join(container).join("song.flac"), "old").unwrap();
    fs::write(root.path().join("song.flac"), "new").unwrap();

    // A listing names it, though hidden entries are not listed.
    let open = backend.call("list.open", json!({"path": root.path()}), &mut notes);
    assert_eq!(open["result"]["leftovers"], json!([container]), "{open}");
    assert_eq!(open["result"]["count"], 1, "{open}");

    let reviewed = backend.call(
        "leftovers.review",
        json!({"folder": root.path(), "names": [container]}),
        &mut notes,
    );
    assert_eq!(
        reviewed["result"]["leftovers"][0]["inside"],
        json!({"kind": "items", "names": ["song.flac"], "more": 0}),
        "{reviewed}"
    );

    let recovered = backend.call(
        "leftovers.recover",
        json!({"path": root.path().join(container)}),
        &mut notes,
    );
    let result = &recovered["result"];
    // The name is taken, so the original comes back numbered and never over the item there.
    let numbered = root.path().join("song (1).flac");
    assert_eq!(result["restored"], json!([numbered]), "{recovered}");
    assert_eq!(
        result["renamed"][0]["savedAs"],
        json!(numbered),
        "{recovered}"
    );
    assert_eq!(result["removed"], json!(true), "{recovered}");
    assert_eq!(fs::read_to_string(&numbered).unwrap(), "old");
    assert_eq!(
        fs::read_to_string(root.path().join("song.flac")).unwrap(),
        "new"
    );
    assert!(!root.path().join(container).exists());

    let refused = backend.call(
        "leftovers.recover",
        json!({"path": root.path().join("song.flac")}),
        &mut notes,
    );
    assert!(refused.get("error").is_some(), "{refused}");
}

#[test]
fn a_delete_removes_a_tree_for_good_and_files_nothing_for_undo() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    let init = backend.call("initialize", json!({}), &mut notes);
    assert!(
        init["result"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("jobs.delete")),
        "{init}"
    );
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let tree = root.path().join("tree");
    fs::create_dir_all(tree.join("sub")).unwrap();
    fs::write(tree.join("sub/a.txt"), "a").unwrap();
    let outside = root.path().join("outside.txt");
    fs::write(&outside, "kept").unwrap();
    std::os::unix::fs::symlink(&outside, tree.join("link")).unwrap();

    let start = backend.call(
        "jobs.start",
        json!({"into": root.path(), "items": [{"source": tree, "action": "delete"}]}),
        &mut notes,
    );
    let job = start["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    let done = notes
        .iter()
        .find(|n| n["method"] == "job.itemDone" && n["params"]["job"] == job)
        .unwrap();
    assert_eq!(done["params"]["outcome"]["status"], "done", "{done}");
    assert!(!tree.exists());
    // The link went, not what it pointed to.
    assert_eq!(fs::read_to_string(&outside).unwrap(), "kept");
    let history = backend.call("undo.history", json!({}), &mut notes);
    assert_eq!(history["result"]["undo"], Value::Null, "{history}");
}

#[test]
fn a_trashed_item_is_restored_by_a_job_to_the_destination_confirmed() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let item = root.path().join("letter.txt");
    fs::write(&item, "dear").unwrap();
    let run = |backend: &mut Backend, notes: &mut Vec<Value>, items: Value| {
        let start = backend.call(
            "jobs.start",
            json!({"into": root.path(), "items": items}),
            notes,
        );
        let job = start["result"]["job"].clone();
        backend.until(notes, |n| {
            n["method"] == "job.finished" && n["params"]["job"] == job
        });
        notes
            .iter()
            .find(|n| n["method"] == "job.itemDone" && n["params"]["job"] == job)
            .unwrap()["params"]["outcome"]
            .clone()
    };

    let trashed = run(
        &mut backend,
        &mut notes,
        json!([{"source": item, "action": "trash"}]),
    );
    assert_eq!(trashed["status"], "done", "{trashed}");
    assert!(!item.exists());
    let in_trash = trashed["path"].clone();

    let elsewhere = run(
        &mut backend,
        &mut notes,
        json!([{"source": in_trash, "action": "restore", "to": root.path().join("other.txt")}]),
    );
    assert_eq!(elsewhere["status"], "failed", "{elsewhere}");
    assert!(!item.exists());

    let restored = run(
        &mut backend,
        &mut notes,
        json!([{"source": in_trash, "action": "restore", "to": item}]),
    );
    assert_eq!(restored["status"], "done", "{restored}");
    assert_eq!(restored["path"], json!(item));
    assert_eq!(fs::read_to_string(&item).unwrap(), "dear");
}

#[test]
fn a_started_job_says_how_much_it_will_write() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let (source, into) = (root.path().join("source"), root.path().join("into"));
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::create_dir_all(&into).unwrap();
    fs::write(source.join("a.txt"), "12345").unwrap();
    fs::write(source.join("sub/b.txt"), "123").unwrap();

    let start = backend.call(
        "jobs.start",
        json!({"into": into, "items": [{"source": source, "action": "copy"}]}),
        &mut notes,
    );
    let job = start["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    let started = notes
        .iter()
        .find(|n| n["method"] == "job.started" && n["params"]["job"] == job)
        .unwrap();
    assert_eq!(started["params"]["totalBytes"], json!(8), "{started}");
    assert_eq!(started["params"]["totalFiles"], json!(2), "{started}");
}

#[test]
fn names_that_are_not_utf8_survive_the_round_trip() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({"protocolVersion": 2}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let folder = root.path().join("music");
    fs::create_dir(&folder).unwrap();
    let latin1 = OsString::from_vec(b"caf\xe9.txt".to_vec());
    fs::write(folder.join(&latin1), b"x").unwrap();

    let open = backend.call("list.open", json!({"path": folder}), &mut notes);
    let listing = open["result"]["listing"].clone();
    let window = backend.call(
        "list.window",
        json!({"listing": listing, "start": 0, "count": 10}),
        &mut notes,
    );
    let name = &window["result"]["rows"][0]["name"];
    assert_eq!(name["bytes"], "Y2Fm6S50eHQ=", "{window}");
    assert_eq!(name["display"], "caf\u{fffd}.txt");

    // The path goes back as bytes: the folder's text, then the name's.
    let mut bytes = folder.as_os_str().to_owned().into_vec();
    bytes.push(b'/');
    bytes.extend_from_slice(b"caf\xe9.txt");
    use base64::Engine;
    let path = json!({"bytes": base64::engine::general_purpose::STANDARD.encode(&bytes)});
    let renamed = backend.call(
        "ops.rename",
        json!({"path": path, "name": "café.txt"}),
        &mut notes,
    );
    assert_eq!(
        renamed["result"]["path"],
        json!(folder.join("café.txt")),
        "{renamed}"
    );
    assert_eq!(fs::read(folder.join("café.txt")).unwrap(), b"x");
    assert!(!folder.join(&latin1).exists());

    let undone = backend.call("undo.undo", json!({}), &mut notes);
    assert_eq!(undone["result"]["done"], 1, "{undone}");
    assert!(folder.join(&latin1).exists(), "the name it had is back");
    backend.call("shutdown", json!({}), &mut notes);
}

#[test]
fn rename_and_create_are_filed_for_undo() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let dir = root.path();
    fs::write(dir.join("a.txt"), b"a").unwrap();
    fs::write(dir.join("b.txt"), b"b").unwrap();

    // A taken name is refused, with its kind.
    let taken = backend.call(
        "ops.rename",
        json!({"path": dir.join("a.txt"), "name": "b.txt"}),
        &mut notes,
    );
    assert_eq!(taken["error"]["data"]["kind"], "exists", "{taken}");
    assert_eq!(taken["error"]["message"], "“b.txt” already exists");
    let renamed = backend.call(
        "ops.rename",
        json!({"path": dir.join("a.txt"), "name": "c.txt"}),
        &mut notes,
    );
    assert_eq!(
        renamed["result"]["path"],
        json!(dir.join("c.txt")),
        "{renamed}"
    );
    assert!(renamed["result"]["undo"].is_u64());
    let history = backend.call("undo.history", json!({}), &mut notes);
    assert_eq!(history["result"]["undo"]["label"], "Rename", "{history}");

    // A default name is numbered; a typed one that is taken fails.
    let first = backend.call(
        "ops.create",
        json!({"into": dir, "name": "New Folder", "kind": "folder", "unique": true}),
        &mut notes,
    );
    assert_eq!(
        first["result"]["path"],
        json!(dir.join("New Folder")),
        "{first}"
    );
    let second = backend.call(
        "ops.create",
        json!({"into": dir, "name": "New Folder", "kind": "folder", "unique": true}),
        &mut notes,
    );
    assert_eq!(
        second["result"]["path"],
        json!(dir.join("New Folder (1)")),
        "{second}"
    );
    let typed = backend.call(
        "ops.create",
        json!({"into": dir, "name": "c.txt", "kind": "file"}),
        &mut notes,
    );
    assert_eq!(typed["error"]["data"]["kind"], "exists", "{typed}");
    let file = backend.call(
        "ops.create",
        json!({"into": dir, "name": "notes.txt", "kind": "file", "label": "New Document"}),
        &mut notes,
    );
    assert!(dir.join("notes.txt").is_file(), "{file}");

    // Undone newest first: the file, the folders, then the rename.
    let undo = |backend: &mut Backend, notes: &mut Vec<Value>| {
        let undone = backend.call("undo.undo", json!({}), notes);
        assert_eq!(undone["result"]["done"], 1, "{undone}");
        undone["result"]["label"].clone()
    };
    assert_eq!(undo(&mut backend, &mut notes), "New Document");
    assert!(!dir.join("notes.txt").exists());
    assert_eq!(undo(&mut backend, &mut notes), "New Folder");
    assert!(!dir.join("New Folder (1)").exists());
    // A folder something was put into since is left as it is.
    fs::write(dir.join("New Folder/kept.txt"), b"k").unwrap();
    let left = backend.call("undo.undo", json!({}), &mut notes);
    assert_eq!(left["result"]["done"], 0, "{left}");
    assert_eq!(
        left["result"]["left"][0]["path"],
        json!(dir.join("New Folder"))
    );
    assert!(dir.join("New Folder/kept.txt").exists());
    assert_eq!(undo(&mut backend, &mut notes), "Rename");
    assert!(dir.join("a.txt").exists() && !dir.join("c.txt").exists());

    // Redo renames it again.
    let redone = backend.call("undo.redo", json!({}), &mut notes);
    assert_eq!(redone["result"]["done"], 1, "{redone}");
    assert!(dir.join("c.txt").exists());
    backend.call("shutdown", json!({}), &mut notes);
}

#[test]
fn requests_are_checked_before_anything_is_done() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    let old = backend.call("initialize", json!({"protocolVersion": 1}), &mut notes);
    assert_eq!(old["error"]["code"], -32600, "{old}");
    assert_eq!(old["error"]["data"]["supported"], json!([2]));
    backend.call("initialize", json!({}), &mut notes);

    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    fs::write(root.path().join("keep.txt"), b"k").unwrap();
    for (method, params, field) in [
        ("list.open", json!({"path": "relative/dir"}), "path"),
        (
            "jobs.start",
            json!({"into": root.path(), "items": [{"source": "keep.txt", "action": "delete"}]}),
            "items[0].source",
        ),
        (
            "jobs.start",
            json!({"into": root.path(), "items": [{"source": root.path().join("keep.txt"), "action": "copy", "name": "../x"}]}),
            "items[0].name",
        ),
        (
            "leftovers.sweep",
            json!({"folder": root.path(), "names": ["../.asc-files-probe-1"]}),
            "names[0]",
        ),
        (
            "ops.create",
            json!({"into": root.path(), "name": "x", "kind": "pipe"}),
            "kind",
        ),
    ] {
        let refused = backend.call(method, params, &mut notes);
        assert_eq!(refused["error"]["code"], -32602, "{refused}");
        let message = refused["error"]["message"].as_str().unwrap();
        assert!(message.contains(field), "{method}: {message}");
    }
    assert!(root.path().join("keep.txt").exists());

    // Not JSON-RPC 2.0: refused, with the request's id.
    backend.send_raw(r#"{"id": 99, "method": "trash.list"}"#);
    let bare = backend.receive("a reply");
    assert_eq!(
        (&bare["id"], &bare["error"]["code"]),
        (&json!(99), &json!(-32600)),
        "{bare}"
    );
    // A notification gets no reply, even when it is refused; the next request is answered.
    backend.send_raw(r#"{"jsonrpc": "2.0", "method": "list.window", "params": {"listing": "x"}}"#);
    let after = backend.call("undo.history", json!({}), &mut notes);
    assert!(after.get("result").is_some(), "{after}");
    assert!(notes.is_empty(), "{notes:?}");
    backend.call("shutdown", json!({}), &mut notes);
}

#[test]
fn undo_asks_before_putting_back_over_a_newer_item() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let (from, to) = (root.path().join("from"), root.path().join("to"));
    fs::create_dir_all(&from).unwrap();
    fs::create_dir_all(&to).unwrap();
    fs::write(from.join("report.txt"), b"moved").unwrap();
    let start = backend.call(
        "jobs.start",
        json!({"into": to, "items": [{"source": from.join("report.txt"), "action": "move"}]}),
        &mut notes,
    );
    let job = start["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "job.finished" && n["params"]["job"] == job
    });
    // Something new takes the name the move left.
    fs::write(from.join("report.txt"), b"newer").unwrap();

    let preview = backend.call("undo.preview", json!({}), &mut notes);
    assert_eq!(
        preview["result"]["blocked"],
        json!([from.join("report.txt")]),
        "{preview}"
    );
    let overwritten = backend.call("undo.undo", json!({"overwrite": true}), &mut notes);
    assert_eq!(overwritten["result"]["done"], 1, "{overwritten}");
    assert_eq!(fs::read(from.join("report.txt")).unwrap(), b"moved");
    assert!(!to.join("report.txt").exists());
    // The newer item is in the Trash, not lost.
    let listed = backend.call("trash.list", json!({}), &mut notes);
    let entry = listed["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["original"] == json!(from.join("report.txt")))
        .cloned()
        .expect("the newer item in the Trash");
    assert_eq!(fs::read(entry["file"].as_str().unwrap()).unwrap(), b"newer");
    backend.call("trash.delete", json!({"info": entry["info"]}), &mut notes);
    backend.call("shutdown", json!({}), &mut notes);
}

#[test]
fn a_search_streams_what_it_finds_and_a_listing_filters() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    fs::create_dir_all(root.path().join("work/reports")).unwrap();
    for file in [
        "work/reports/q3-report.pdf",
        "work/notes.txt",
        "report.md",
        "photo.jpg",
    ] {
        fs::write(root.path().join(file), b"x").unwrap();
    }
    let started = backend.call(
        "search.start",
        json!({"roots": [root.path()], "query": "report"}),
        &mut notes,
    );
    let search = started["result"]["search"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "search.finished" && n["params"]["search"] == search
    });
    let finished = notes
        .iter()
        .find(|n| n["method"] == "search.finished")
        .unwrap();
    let best: Vec<_> = finished["params"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].clone())
        .collect();
    assert_eq!(
        best.first(),
        Some(&json!(root.path().join("report.md"))),
        "{finished}"
    );
    assert!(best.contains(&json!(root.path().join("work/reports/q3-report.pdf"))));
    assert!(!best.contains(&json!(root.path().join("photo.jpg"))));
    assert!(notes.iter().any(|n| n["method"] == "search.found"));

    // A listing filtered by typing.
    let open = backend.call(
        "list.open",
        json!({"path": root.path(), "filter": "rep"}),
        &mut notes,
    );
    assert_eq!(open["result"]["count"], 1, "{open}");
    let listing = open["result"]["listing"].clone();
    let cleared = backend.call(
        "list.reorder",
        json!({"listing": listing, "filter": ""}),
        &mut notes,
    );
    assert_eq!(cleared["result"]["count"], 3, "{cleared}");
    backend.call("shutdown", json!({}), &mut notes);
}

#[test]
fn a_search_leaves_out_folders_and_stops_at_its_limits() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    for folder in ["work/Archive", "work/live", "private"] {
        fs::create_dir_all(root.path().join(folder)).unwrap();
    }
    for file in [
        "work/Archive/plan.txt",
        "work/live/plan.txt",
        "private/plan.txt",
    ] {
        fs::write(root.path().join(file), b"x").unwrap();
    }
    let finished = |backend: &mut Backend, notes: &mut Vec<Value>, params: Value| {
        notes.clear();
        let started = backend.call("search.start", params, notes);
        let search = started["result"]["search"].clone();
        backend.until(notes, |n| {
            n["method"] == "search.finished" && n["params"]["search"] == search
        });
        notes
            .iter()
            .find(|n| n["method"] == "search.finished")
            .unwrap()["params"]
            .clone()
    };
    let left = finished(
        &mut backend,
        &mut notes,
        json!({"roots": [root.path()], "query": "plan", "exclude": ["archive", root.path().join("private")], "seconds": 0, "entries": 0}),
    );
    let paths: Vec<_> = left["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].clone())
        .collect();
    assert_eq!(
        paths,
        [json!(root.path().join("work/live/plan.txt"))],
        "{left}"
    );
    assert_eq!(left["truncated"], false);
    let few = finished(
        &mut backend,
        &mut notes,
        json!({"roots": [root.path()], "query": "plan", "entries": 2}),
    );
    assert_eq!(few["truncated"], true, "{few}");

    // A rule as it is kept, or why it can't be one.
    let kept = backend.call("search.exclusion", json!({"rule": " Build/ "}), &mut notes);
    assert_eq!(kept["result"]["rule"], "Build");
    let home = backend.call("search.exclusion", json!({"rule": "~"}), &mut notes);
    assert_eq!(home["error"]["code"], -32602, "{home}");
    backend.call("shutdown", json!({}), &mut notes);
}

#[test]
fn a_folder_is_measured_and_permissions_change() {
    use std::os::unix::fs::PermissionsExt;
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    fs::create_dir_all(root.path().join("a/.cache")).unwrap();
    fs::write(root.path().join("a/one"), b"12345").unwrap();
    fs::write(root.path().join("a/.cache/two"), b"12").unwrap();
    let started = backend.call("measure.start", json!({"path": root.path()}), &mut notes);
    let measure = started["result"]["measure"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "measure.finished" && n["params"]["measure"] == measure
    });
    let finished = notes
        .iter()
        .find(|n| n["method"] == "measure.finished")
        .unwrap()["params"]
        .clone();
    assert_eq!(finished["items"], 4, "{finished}");
    assert_eq!(finished["size"], 7);
    assert_eq!(finished["files"], 1);
    assert_eq!(finished["folders"], 1);
    assert_eq!(finished["unreadable"], false);
    // Not a folder: said so.
    notes.clear();
    let started = backend.call(
        "measure.start",
        json!({"path": root.path().join("a/one")}),
        &mut notes,
    );
    let measure = started["result"]["measure"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "measure.finished" && n["params"]["measure"] == measure
    });
    let failed = notes
        .iter()
        .find(|n| n["method"] == "measure.finished")
        .unwrap()["params"]
        .clone();
    assert_eq!(failed["error"]["kind"], "notDirectory", "{failed}");

    let file = root.path().join("a/one");
    let changed = backend.call(
        "ops.chmod",
        json!({"path": file, "bits": 0o750}),
        &mut notes,
    );
    assert_eq!(changed["result"]["mode"], 0o750, "{changed}");
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o7777,
        0o750
    );
    let refused = backend.call(
        "ops.chmod",
        json!({"path": file, "bits": 0o4755}),
        &mut notes,
    );
    assert_eq!(refused["error"]["code"], -32602, "{refused}");
    backend.call("shutdown", json!({}), &mut notes);
}

#[test]
fn an_action_is_written_offered_and_run() {
    let mut backend = Backend::start();
    let mut notes = Vec::new();
    backend.call("initialize", json!({}), &mut notes);
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let file = root.path().join("notes.txt");
    fs::write(&file, b"one\ntwo\n").unwrap();
    let written = backend.call(
        "actions.write",
        json!({
            "definition": {
                "schema_version": 1, "id": "count", "name": "Count lines", "enabled": true,
                "when": {"kinds": ["file"], "extensions": ["txt"]},
                "run": {"runtime": "bash", "entrypoint": "run.sh", "mode": "per-item"},
            },
            // Paths come in a file, NUL-separated, as legacy's contract has them.
            "script": {"fileName": "run.sh", "contents": "#!/usr/bin/env bash\nwhile IFS= read -r -d '' path; do wc -l < \"$path\"; done < \"$NYX_FILES_ACTION_PATHS\"\n"},
            "create": true,
        }),
        &mut notes,
    );
    assert_eq!(written["result"]["id"], "count", "{written}");
    let again = backend.call("actions.write", written_again(), &mut notes);
    assert_eq!(again["error"]["data"]["kind"], "exists", "{again}");
    // Read as a manifest is: [when] is required there too.
    let mut unsaid = written_again();
    unsaid["definition"].as_object_mut().unwrap().remove("when");
    let refused = backend.call("actions.write", unsaid, &mut notes);
    assert_eq!(
        refused["error"]["data"]["kind"], "invalidArgument",
        "{refused}"
    );

    let listed = backend.call(
        "actions.list",
        json!({"inputs": [{"path": file, "type": "text/plain"}]}),
        &mut notes,
    );
    assert_eq!(listed["result"]["matched"], json!(["count"]), "{listed}");
    let photo = backend.call(
        "actions.list",
        json!({"inputs": [{"path": root.path().join("a.png")}]}),
        &mut notes,
    );
    assert_eq!(photo["result"]["matched"], json!([]));
    if listed["result"]["actions"][0]["available"] != true {
        eprintln!("skipping the run: bash is not installed");
        backend.call("shutdown", json!({}), &mut notes);
        return;
    }

    let run = backend.call(
        "actions.run",
        json!({"id": "count", "inputs": [{"path": file}], "parent": root.path()}),
        &mut notes,
    );
    let job = run["result"]["job"].clone();
    backend.until(&mut notes, |n| {
        n["method"] == "action.job"
            && n["params"]["job"] == job
            && ["succeeded", "failed", "cancelled"]
                .contains(&n["params"]["status"].as_str().unwrap_or(""))
    });
    let done = notes
        .iter()
        .rev()
        .find(|n| n["method"] == "action.job")
        .unwrap()["params"]
        .clone();
    assert_eq!(done["status"], "succeeded", "{done}");
    assert_eq!(done["succeeded"], 1, "{done}");
    assert!(done["log"].as_str().unwrap().contains('2'), "{done}");

    // Disabled: no longer run.
    backend.call(
        "actions.setEnabled",
        json!({"id": "count", "enabled": false}),
        &mut notes,
    );
    let refused = backend.call(
        "actions.run",
        json!({"id": "count", "inputs": [{"path": file}], "parent": root.path()}),
        &mut notes,
    );
    assert!(refused["error"].is_object(), "{refused}");
    let copied = backend.call("actions.duplicate", json!({"id": "count"}), &mut notes);
    assert_eq!(copied["result"]["id"], "count-copy");
    backend.call("actions.delete", json!({"id": "count"}), &mut notes);
    let left = backend.call("actions.list", json!({}), &mut notes);
    assert_eq!(
        left["result"]["actions"].as_array().unwrap().len(),
        1,
        "{left}"
    );
    backend.call("shutdown", json!({}), &mut notes);
}

fn written_again() -> Value {
    json!({
        "definition": {"schema_version": 1, "id": "count", "name": "Count lines", "when": {}, "run": {"runtime": "bash", "entrypoint": "run.sh"}},
        "script": {"fileName": "run.sh", "contents": "#!/usr/bin/env bash\n"},
        "create": true,
    })
}
