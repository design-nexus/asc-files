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
    /// Its own session folder, so tests running side by side keep separate undo histories.
    _runtime: tempfile::TempDir,
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
