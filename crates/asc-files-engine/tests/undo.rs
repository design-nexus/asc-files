// SPDX-License-Identifier: MIT
//! Undo and redo through jobs, on folders inside the test home so the Trash they use is the test
//! home's own. Each test keeps its journal in a folder of its own.

use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::mpsc,
};

use asc_files_engine::{
    jobs::{self, Action, Event, Item, Record, Request},
    ops::{Cancel, Conflict, NameMode, Progress},
    trash,
    undo::{History, Journal},
};

fn isolated_dir() -> tempfile::TempDir {
    let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME set by scripts/test.sh");
    assert!(
        data.contains("asc-files-test-home"),
        "refusing to touch a real Trash: {data}"
    );
    fs::create_dir_all(&data).unwrap();
    tempfile::tempdir_in(&data).unwrap()
}

struct Fixture {
    root: tempfile::TempDir,
    journal: Journal,
}

impl Fixture {
    fn new() -> Self {
        let root = isolated_dir();
        let journal = Journal::at(&root.path().join("journal")).unwrap();
        for folder in ["source", "dest"] {
            fs::create_dir(root.path().join(folder)).unwrap();
        }
        Self { root, journal }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    /// Runs a batch into `dest`, filed under `label`; returns its undo entry.
    fn run(&self, label: &str, items: Vec<(PathBuf, Action)>) -> Option<u64> {
        let (send, events) = mpsc::channel();
        jobs::start(
            Request {
                into: self.path("dest"),
                items: items
                    .into_iter()
                    .map(|(source, action)| Item::new(source, action))
                    .collect(),
                names: NameMode::Fix,
                record: Some(Record {
                    journal: self.journal.clone(),
                    label: label.to_owned(),
                }),
                durable: None,
            },
            send,
        )
        .join();
        events
            .into_iter()
            .find_map(|event| match event {
                Event::Finished { undo, .. } => Some(undo.expect("the history was written")),
                _ => None,
            })
            .unwrap()
    }

    fn undo(&self) -> asc_files_engine::undo::Outcome {
        self.journal
            .undo(&Cancel::new(), &Progress::new())
            .expect("undo")
    }

    fn redo(&self) -> asc_files_engine::undo::Outcome {
        self.journal
            .redo(&Cancel::new(), &Progress::new())
            .expect("redo")
    }

    fn in_trash(&self) -> Vec<trash::Entry> {
        trash::list()
            .into_iter()
            .filter(|entry| entry.original.starts_with(self.root.path()))
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = trash::delete_all(self.in_trash());
    }
}

#[test]
fn a_copy_is_undone_to_the_trash_and_redone() {
    let fixture = Fixture::new();
    fs::write(fixture.path("source/a.txt"), b"a").unwrap();
    fs::create_dir(fixture.path("source/folder")).unwrap();
    fs::write(fixture.path("source/folder/inner"), b"i").unwrap();

    let id = fixture.run(
        "Copy 2 items",
        vec![
            (fixture.path("source/a.txt"), Action::Copy(Conflict::Fail)),
            (fixture.path("source/folder"), Action::Copy(Conflict::Fail)),
        ],
    );
    assert!(id.is_some());
    assert_eq!(
        fixture.journal.history().unwrap().undo,
        Some((id.unwrap(), "Copy 2 items".to_owned()))
    );

    let undone = fixture.undo();
    assert_eq!((undone.done, undone.left.len()), (2, 0), "{undone:?}");
    assert!(!fixture.path("dest/a.txt").exists());
    assert!(!fixture.path("dest/folder").exists());
    assert_eq!(fixture.in_trash().len(), 2, "the copies went to the Trash");
    assert!(
        fixture.path("source/a.txt").exists(),
        "the source is untouched"
    );
    assert_eq!(
        fixture.journal.history().unwrap(),
        History {
            undo: None,
            redo: Some((id.unwrap(), "Copy 2 items".to_owned())),
        }
    );

    let redone = fixture.redo();
    assert_eq!(redone.done, 2);
    assert_eq!(fs::read(fixture.path("dest/folder/inner")).unwrap(), b"i");
    assert!(fixture.journal.history().unwrap().undo.is_some());
    assert_eq!(fixture.undo().done, 2, "a redone copy can be undone again");
}

#[test]
fn a_move_is_undone_back_to_where_it_was_and_redone() {
    let fixture = Fixture::new();
    fs::write(fixture.path("source/m.txt"), b"m").unwrap();
    let inode = fs::metadata(fixture.path("source/m.txt")).unwrap().ino();

    fixture.run("Move", vec![(fixture.path("source/m.txt"), Action::Move)]);
    assert!(fixture.path("dest/m.txt").exists());

    assert_eq!(fixture.undo().done, 1);
    assert_eq!(
        fs::metadata(fixture.path("source/m.txt")).unwrap().ino(),
        inode,
        "the same file, renamed back"
    );
    assert!(!fixture.path("dest/m.txt").exists());

    assert_eq!(fixture.redo().done, 1);
    assert!(fixture.path("dest/m.txt").exists());
}

#[test]
fn a_trashed_item_is_restored_and_trashed_again() {
    let fixture = Fixture::new();
    fs::write(fixture.path("source/t.txt"), b"t").unwrap();

    fixture.run(
        "Move to Trash",
        vec![(fixture.path("source/t.txt"), Action::Trash)],
    );
    assert!(!fixture.path("source/t.txt").exists());

    assert_eq!(fixture.undo().done, 1);
    assert_eq!(fs::read(fixture.path("source/t.txt")).unwrap(), b"t");
    assert!(fixture.in_trash().is_empty());

    assert_eq!(fixture.redo().done, 1);
    assert!(!fixture.path("source/t.txt").exists());
    assert_eq!(fixture.in_trash().len(), 1);
}

#[test]
fn a_replace_is_undone_by_putting_the_original_back() {
    let fixture = Fixture::new();
    fs::write(fixture.path("source/r.txt"), b"new").unwrap();
    fs::write(fixture.path("dest/r.txt"), b"old").unwrap();
    let original = fs::metadata(fixture.path("dest/r.txt")).unwrap().ino();

    fixture.run(
        "Replace",
        vec![(fixture.path("source/r.txt"), Action::Replace)],
    );
    assert_eq!(fs::read(fixture.path("dest/r.txt")).unwrap(), b"new");

    let undone = fixture.undo();
    assert_eq!((undone.done, undone.left.len()), (1, 0), "{undone:?}");
    assert_eq!(fs::read(fixture.path("dest/r.txt")).unwrap(), b"old");
    assert_eq!(
        fs::metadata(fixture.path("dest/r.txt")).unwrap().ino(),
        original
    );
    let trashed: Vec<_> = fixture.in_trash();
    assert_eq!(
        trashed.len(),
        1,
        "the copy that replaced it is in the Trash"
    );
    assert_eq!(fs::read(&trashed[0].file).unwrap(), b"new");

    assert_eq!(fixture.redo().done, 1);
    assert_eq!(fs::read(fixture.path("dest/r.txt")).unwrap(), b"new");
}

#[test]
fn an_item_changed_since_is_left_and_reported_while_the_rest_is_undone() {
    let fixture = Fixture::new();
    for name in ["one", "two"] {
        fs::write(fixture.path(&format!("source/{name}")), name).unwrap();
    }
    fixture.run(
        "Copy",
        vec![
            (fixture.path("source/one"), Action::Copy(Conflict::Fail)),
            (fixture.path("source/two"), Action::Copy(Conflict::Fail)),
        ],
    );
    // Someone puts another file where the copy was.
    fs::remove_file(fixture.path("dest/one")).unwrap();
    fs::write(fixture.path("dest/one"), b"someone else's").unwrap();
    // Keep the old inode from being handed straight back to the new file.
    let _held = fs::File::create(fixture.path("dest/spacer")).unwrap();

    let undone = fixture.undo();

    assert_eq!(undone.done, 1);
    assert_eq!(undone.left.len(), 1, "{undone:?}");
    assert_eq!(undone.left[0].0, fixture.path("dest/one"));
    assert_eq!(
        fs::read(fixture.path("dest/one")).unwrap(),
        b"someone else's",
        "left as it is"
    );
    assert!(!fixture.path("dest/two").exists());
}

#[test]
fn a_merge_into_a_folder_that_was_there_is_undone_without_touching_that_folder() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.path("source/photos")).unwrap();
    fs::write(fixture.path("source/photos/new.jpg"), b"n").unwrap();
    fs::create_dir(fixture.path("dest/photos")).unwrap();
    fs::write(fixture.path("dest/photos/mine.jpg"), b"m").unwrap();
    let folder = fs::metadata(fixture.path("dest/photos")).unwrap().ino();

    let id = fixture.run(
        "Copy",
        vec![(fixture.path("source/photos"), Action::Copy(Conflict::Merge))],
    );
    assert!(id.is_some(), "a merge is filed item by item");
    assert!(fixture.path("dest/photos/new.jpg").exists());

    assert_eq!(fixture.undo().done, 1);
    assert!(!fixture.path("dest/photos/new.jpg").exists());
    assert_eq!(
        fs::metadata(fixture.path("dest/photos")).unwrap().ino(),
        folder
    );
    assert_eq!(
        fs::read(fixture.path("dest/photos/mine.jpg")).unwrap(),
        b"m"
    );
}

#[test]
fn a_new_operation_clears_redo_and_history_is_shared_and_bounded() {
    let fixture = Fixture::new();
    fs::write(fixture.path("source/x"), b"x").unwrap();
    fixture.run(
        "First",
        vec![(fixture.path("source/x"), Action::Copy(Conflict::KeepBoth))],
    );
    fixture.undo();
    assert!(fixture.journal.history().unwrap().redo.is_some());

    fixture.run(
        "Second",
        vec![(fixture.path("source/x"), Action::Copy(Conflict::KeepBoth))],
    );
    let other_window = Journal::at(&fixture.path("journal")).unwrap();
    assert_eq!(
        other_window.history().unwrap(),
        History {
            undo: Some((2, "Second".to_owned())),
            redo: None,
        }
    );

    for n in 0..asc_files_engine::undo::DEPTH + 5 {
        fixture.run(
            &format!("Copy {n}"),
            vec![(fixture.path("source/x"), Action::Copy(Conflict::KeepBoth))],
        );
    }
    let mut undone = 0;
    while fixture
        .journal
        .undo(&Cancel::new(), &Progress::new())
        .is_ok()
    {
        undone += 1;
    }
    assert_eq!(undone, asc_files_engine::undo::DEPTH);
}

#[test]
fn an_unreadable_history_starts_afresh_and_an_open_folder_is_refused() {
    let fixture = Fixture::new();
    fs::write(fixture.path("journal/undo-journal"), b"{not json").unwrap();
    assert_eq!(fixture.journal.history().unwrap(), History::default());

    let shared = fixture.path("shared");
    fs::create_dir(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(Journal::at(&shared).is_err());
    let link = fixture.path("link");
    std::os::unix::fs::symlink(fixture.path("journal"), &link).unwrap();
    assert!(Journal::at(&link).is_err());
}

#[test]
fn a_move_across_drives_is_undone_across_them() {
    let fixture = Fixture::new();
    let shm = Path::new("/dev/shm");
    let Ok(other) = tempfile::tempdir_in(shm) else {
        eprintln!("skipped: no /dev/shm");
        return;
    };
    if fs::metadata(shm).unwrap().dev() == fs::metadata(fixture.root.path()).unwrap().dev() {
        eprintln!("skipped: /dev/shm is not another drive here");
        return;
    }
    fs::create_dir(fixture.path("source/tree")).unwrap();
    fs::write(fixture.path("source/tree/f"), b"f").unwrap();
    let (send, events) = mpsc::channel();
    jobs::start(
        Request {
            into: other.path().to_path_buf(),
            items: vec![Item::new(fixture.path("source/tree"), Action::Move)],
            names: NameMode::Fix,
            record: Some(Record {
                journal: fixture.journal.clone(),
                label: "Move".to_owned(),
            }),
            durable: None,
        },
        send,
    )
    .join();
    drop(events);
    assert!(other.path().join("tree/f").exists());

    assert_eq!(fixture.undo().done, 1);
    assert_eq!(fs::read(fixture.path("source/tree/f")).unwrap(), b"f");
    assert!(!other.path().join("tree").exists());
}
