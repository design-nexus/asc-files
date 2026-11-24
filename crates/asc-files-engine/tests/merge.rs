// SPDX-License-Identifier: MIT
//! Merges with a plan, through the public API and through jobs with undo, on folders inside the
//! test home so the Trash they use is the test home's own.

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::mpsc,
};

use asc_files_engine::{
    error::ErrorKind,
    jobs::{self, Action, Event, Item, Outcome, Record, Request},
    merge::{self, Clash, MergePlan},
    ops::{Cancel, Conflict, NameMode, Progress},
    trash,
    undo::Journal,
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

/// `source/photos` and `dest/photos`, clashing at `a.jpg`, `b.jpg` and `2024/c.jpg`.
struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = isolated_dir();
        let write = |relative: &str, content: &str| {
            let path = root.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        };
        for name in [
            "a.jpg",
            "b.jpg",
            "2024/c.jpg",
            "only-new.jpg",
            "new-folder/d.jpg",
        ] {
            write(&format!("source/photos/{name}"), &format!("new {name}"));
        }
        for name in ["a.jpg", "b.jpg", "2024/c.jpg", "only-mine.jpg"] {
            write(&format!("dest/photos/{name}"), &format!("old {name}"));
        }
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).unwrap()
    }

    fn in_trash(&self) -> Vec<trash::Entry> {
        trash::list()
            .into_iter()
            .filter(|entry| entry.original.starts_with(self.root.path()))
            .collect()
    }

    /// Replace `a.jpg`, skip `2024/c.jpg`, keep both otherwise.
    fn plan() -> MergePlan {
        MergePlan {
            choices: BTreeMap::from([
                (PathBuf::from("a.jpg"), Clash::Replace),
                (PathBuf::from("2024/c.jpg"), Clash::Skip),
            ]),
            otherwise: Clash::KeepBoth,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = trash::delete_all(self.in_trash());
    }
}

#[test]
fn clashes_are_the_items_whose_names_are_taken_inside_folders_that_merge() {
    let fixture = Fixture::new();

    let clashes = merge::clashes(&fixture.path("source/photos"), &fixture.path("dest")).unwrap();

    let paths: Vec<_> = clashes.iter().map(|clash| clash.path.clone()).collect();
    assert_eq!(paths, ["2024/c.jpg", "a.jpg", "b.jpg"].map(PathBuf::from));
    assert!(!clashes[0].incoming.is_folder && !clashes[0].existing.is_folder);
    assert_eq!(clashes[1].incoming.size, "new a.jpg".len() as u64);
    assert!(
        merge::clashes(&fixture.path("source/photos"), &fixture.path("elsewhere"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_plan_replaces_skips_and_keeps_both_as_it_says() {
    let fixture = Fixture::new();
    let original_a = fs::metadata(fixture.path("dest/photos/a.jpg"))
        .unwrap()
        .ino();

    let merged = merge::merge(
        &fixture.path("source/photos"),
        &fixture.path("dest"),
        &Fixture::plan(),
        NameMode::Fix,
        &Cancel::new(),
        &Progress::new(),
    )
    .unwrap();

    assert!(merged.failed.is_empty(), "{:?}", merged.failed);
    assert_eq!(fixture.read("dest/photos/a.jpg"), "new a.jpg");
    let trashed = fixture.in_trash();
    assert_eq!(trashed.len(), 1);
    assert_eq!(fs::metadata(&trashed[0].file).unwrap().ino(), original_a);
    assert_eq!(fixture.read("dest/photos/b.jpg"), "old b.jpg");
    assert_eq!(fixture.read("dest/photos/b (1).jpg"), "new b.jpg");
    assert_eq!(fixture.read("dest/photos/2024/c.jpg"), "old 2024/c.jpg");
    assert_eq!(merged.left_out, [PathBuf::from("2024/c.jpg")]);
    assert_eq!(fixture.read("dest/photos/only-new.jpg"), "new only-new.jpg");
    assert_eq!(
        fixture.read("dest/photos/new-folder/d.jpg"),
        "new new-folder/d.jpg"
    );
    assert_eq!(
        fixture.read("dest/photos/only-mine.jpg"),
        "old only-mine.jpg"
    );
    assert_eq!(merged.replaced(), 1);
    assert_eq!(merged.created(), 3, "b (1), only-new and new-folder");
}

#[test]
fn a_merge_with_no_folder_to_merge_into_is_a_copy_and_into_a_file_is_refused() {
    let fixture = Fixture::new();

    let merged = merge::merge(
        &fixture.path("source/photos"),
        &fixture.path("source/photos/new-folder"),
        &MergePlan::default(),
        NameMode::Fix,
        &Cancel::new(),
        &Progress::new(),
    );
    assert!(merged.is_err(), "into itself");

    fs::create_dir(fixture.path("empty")).unwrap();
    let merged = merge::merge(
        &fixture.path("source/photos"),
        &fixture.path("empty"),
        &MergePlan::default(),
        NameMode::Fix,
        &Cancel::new(),
        &Progress::new(),
    )
    .unwrap();
    assert_eq!(merged.created(), 1);
    assert!(fixture.path("empty/photos/2024/c.jpg").exists());

    fs::create_dir(fixture.path("file-dest")).unwrap();
    fs::write(fixture.path("file-dest/photos"), b"").unwrap();
    let error = merge::merge(
        &fixture.path("source/photos"),
        &fixture.path("file-dest"),
        &MergePlan::default(),
        NameMode::Fix,
        &Cancel::new(),
        &Progress::new(),
    )
    .expect_err("a file is not merged into");
    assert_eq!(error.kind(), ErrorKind::Exists);
}

fn run(fixture: &Fixture, journal: &Journal, action: Action) -> Vec<Event> {
    let (send, events) = mpsc::channel();
    jobs::start(
        Request {
            into: fixture.path("dest"),
            items: vec![Item::new(fixture.path("source/photos"), action)],
            names: NameMode::Fix,
            record: Some(Record {
                journal: journal.clone(),
                label: "Merge".to_owned(),
            }),
            durable: None,
        },
        send,
    )
    .join();
    events.into_iter().collect()
}

#[test]
fn a_merge_is_undone_item_by_item_and_the_folder_merged_into_stays() {
    let fixture = Fixture::new();
    let journal = Journal::at(&fixture.path("journal")).unwrap();
    let mine = fs::metadata(fixture.path("dest/photos")).unwrap().ino();

    let events = run(&fixture, &journal, Action::Merge(Fixture::plan()));
    let undo = events.iter().find_map(|event| match event {
        Event::Finished { undo, .. } => Some(undo.clone().unwrap()),
        _ => None,
    });
    assert!(undo.flatten().is_some(), "a merge is filed: {events:?}");

    let undone = journal.undo(&Cancel::new(), &Progress::new()).unwrap();

    assert_eq!((undone.done, undone.left.len()), (4, 0), "{undone:?}");
    assert_eq!(
        fs::metadata(fixture.path("dest/photos")).unwrap().ino(),
        mine
    );
    let mut left: Vec<_> = fs::read_dir(fixture.path("dest/photos"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(left, ["2024", "a.jpg", "b.jpg", "only-mine.jpg"]);
    assert_eq!(
        fixture.read("dest/photos/a.jpg"),
        "old a.jpg",
        "the original is back"
    );
    assert_eq!(fixture.read("dest/photos/2024/c.jpg"), "old 2024/c.jpg");

    let redone = journal.redo(&Cancel::new(), &Progress::new()).unwrap();
    assert_eq!(redone.done, 4, "{redone:?}");
    assert_eq!(fixture.read("dest/photos/a.jpg"), "new a.jpg");
    assert!(fixture.path("dest/photos/new-folder/d.jpg").exists());
}

#[test]
fn a_plain_merge_through_a_job_keeps_both_and_can_be_undone() {
    let fixture = Fixture::new();
    let journal = Journal::at(&fixture.path("journal")).unwrap();

    run(&fixture, &journal, Action::Copy(Conflict::Merge));

    assert_eq!(fixture.read("dest/photos/a (1).jpg"), "new a.jpg");
    assert_eq!(fixture.read("dest/photos/2024/c (1).jpg"), "new 2024/c.jpg");
    assert_eq!(
        journal.undo(&Cancel::new(), &Progress::new()).unwrap().done,
        5
    );
    assert!(!fixture.path("dest/photos/a (1).jpg").exists());
    assert_eq!(fixture.read("dest/photos/a.jpg"), "old a.jpg");
}

#[test]
fn a_merge_that_stops_on_an_unreadable_item_files_what_it_placed() {
    let fixture = Fixture::new();
    let journal = Journal::at(&fixture.path("journal")).unwrap();
    let unreadable = fixture.path("source/photos/only-new.jpg");
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&unreadable).is_ok() {
        eprintln!("skipped: running with permission to read anything");
        return;
    }

    let events = run(&fixture, &journal, Action::Merge(MergePlan::default()));

    let outcome = events.iter().find_map(|event| match event {
        Event::ItemDone { outcome, .. } => Some(outcome),
        _ => None,
    });
    assert!(
        matches!(outcome, Some(Outcome::Failed { .. })),
        "{outcome:?}"
    );
    // The engine's merge stops at the item it cannot read, as legacy's did; what it placed
    // before stays, and is filed, so undo takes away exactly that.
    let placed: Vec<_> = ["a (1).jpg", "b (1).jpg", "new-folder"]
        .into_iter()
        .filter(|name| fixture.path("dest/photos").join(name).exists())
        .collect();
    if !placed.is_empty() {
        let undone = journal.undo(&Cancel::new(), &Progress::new()).unwrap();
        assert!(undone.left.is_empty(), "{undone:?}");
    }
    for name in placed {
        assert!(!fixture.path("dest/photos").join(name).exists(), "{name}");
    }
    assert_eq!(fixture.read("dest/photos/a.jpg"), "old a.jpg");
    assert_eq!(
        fixture.read("dest/photos/only-mine.jpg"),
        "old only-mine.jpg"
    );
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn clash_paths_survive_the_journal() {
    let plan = Fixture::plan();
    let fixture = Fixture::new();
    let journal_dir = fixture.path("journal");
    let journal = Journal::at(&journal_dir).unwrap();
    run(&fixture, &journal, Action::Merge(plan));
    let text = fs::read_to_string(Path::new(&journal_dir).join("undo-journal")).unwrap();
    assert!(text.contains("b (1).jpg"), "{text}");
}

#[test]
fn a_merge_under_another_name_replaces_only_in_the_folder_merged_into() {
    let fixture = Fixture::new();
    // The source beside the folder it merges into, and a bystander of the source's own name.
    let (source, target) = (fixture.path("dest/photos"), fixture.path("dest/album"));
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("a.jpg"), "album a").unwrap();
    fs::write(source.join("extra.jpg"), "new extra").unwrap();
    let bystander = fixture.path("elsewhere/photos");
    fs::create_dir_all(&bystander).unwrap();
    fs::write(bystander.join("a.jpg"), "bystander a").unwrap();
    for (from, into) in [
        (&source, fixture.path("dest")),
        (&fixture.path("source/photos"), fixture.path("dest")),
    ] {
        let plan = MergePlan {
            otherwise: Clash::Replace,
            ..MergePlan::default()
        };
        merge::merge_as(
            from,
            (&into, "album".as_ref()),
            &plan,
            NameMode::Fix,
            &Cancel::new(),
            &Progress::new(),
        )
        .unwrap();
        assert!(
            from.join("a.jpg").exists(),
            "the source keeps its own files"
        );
    }
    assert_eq!(fixture.read("dest/album/a.jpg"), "new a.jpg");
    assert_eq!(fixture.read("dest/photos/extra.jpg"), "new extra");
    assert_eq!(fixture.read("elsewhere/photos/a.jpg"), "bystander a");
}
