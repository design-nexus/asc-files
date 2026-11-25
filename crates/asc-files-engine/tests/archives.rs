// SPDX-License-Identifier: MIT
//! Archives through jobs: compress and extract as items, their progress, password failures, and
//! undo and redo. Folders are inside the test home, so the Trash they use is its own.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
};

use asc_files_engine::{
    error::ErrorKind,
    jobs::{self, Action, Compress, Event, Item, Outcome, Password, Record, Request},
    ops::{Cancel, NameMode, Progress},
    services::{ArchiveFormat, TransferConflict},
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
        fs::write(root.path().join("source/a.txt"), b"a").unwrap();
        fs::create_dir(root.path().join("source/folder")).unwrap();
        fs::write(root.path().join("source/folder/inner.txt"), b"inner").unwrap();
        Self { root, journal }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    /// Runs one item into `dest`, filed for undo; returns every event.
    fn run(&self, source: PathBuf, action: Action) -> Vec<Event> {
        let (send, events) = mpsc::channel();
        jobs::start(
            Request {
                into: self.path("dest"),
                items: vec![Item::new(source, action)],
                names: NameMode::Fix,
                record: Some(Record {
                    journal: self.journal.clone(),
                    label: "Archive".to_owned(),
                }),
                durable: None,
            },
            send,
        )
        .join();
        events.into_iter().collect()
    }

    fn compress(
        &self,
        name: &str,
        password: Option<&str>,
        conflict: TransferConflict,
    ) -> Vec<Event> {
        let sources = vec![self.path("source/a.txt"), self.path("source/folder")];
        self.run(
            sources[0].clone(),
            Action::Compress(Box::new(Compress {
                sources,
                name: name.to_owned(),
                format: ArchiveFormat::Zip,
                password: password.map(|password| Password(password.to_owned())),
                conflict,
            })),
        )
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

fn outcome(events: &[Event]) -> &Outcome {
    events
        .iter()
        .find_map(|event| match event {
            Event::ItemDone { outcome, .. } => Some(outcome),
            _ => None,
        })
        .expect("the item ended")
}

fn members(archive: &Path) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    let mut names: Vec<String> = (0..zip.len())
        .map(|index| {
            zip.by_index_raw(index)
                .unwrap()
                .name()
                .unwrap()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn a_compress_job_makes_an_archive_counts_its_members_and_undo_takes_it_away() {
    let fixture = Fixture::new();
    let events = fixture.compress("bundle", None, TransferConflict::FailIfExists);
    let archive = fixture.path("dest/bundle.zip");
    assert!(
        matches!(outcome(&events), Outcome::Done { name, .. } if *name == archive),
        "{events:?}"
    );
    assert_eq!(members(&archive), ["a.txt", "folder/", "folder/inner.txt"]);
    // What goes in was measured, and the members written are the item's files.
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Started { files: Some(2), .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Progress { progress, .. } if progress.files == 2)),
        "{events:?}"
    );

    let undone = fixture
        .journal
        .undo(&Cancel::new(), &Progress::new())
        .unwrap();
    assert_eq!((undone.done, undone.left.len()), (1, 0), "{undone:?}");
    assert!(!archive.exists());
    assert_eq!(fixture.in_trash().len(), 1);
    assert!(
        fixture.path("source/a.txt").exists(),
        "what went in is untouched"
    );

    assert_eq!(
        fixture
            .journal
            .redo(&Cancel::new(), &Progress::new())
            .unwrap()
            .done,
        1
    );
    assert_eq!(members(&archive).len(), 3);
}

#[test]
fn a_replacing_compress_is_undone_by_putting_the_original_back() {
    let fixture = Fixture::new();
    fs::write(fixture.path("dest/bundle.zip"), b"old").unwrap();
    let events = fixture.compress("bundle", None, TransferConflict::ReplaceExisting);
    assert!(
        matches!(outcome(&events), Outcome::Done { .. }),
        "{events:?}"
    );
    assert_eq!(members(&fixture.path("dest/bundle.zip")).len(), 3);

    let undone = fixture
        .journal
        .undo(&Cancel::new(), &Progress::new())
        .unwrap();
    assert_eq!((undone.done, undone.left.len()), (1, 0), "{undone:?}");
    assert_eq!(fs::read(fixture.path("dest/bundle.zip")).unwrap(), b"old");

    assert_eq!(
        fixture
            .journal
            .redo(&Cancel::new(), &Progress::new())
            .unwrap()
            .done,
        1
    );
    assert_eq!(members(&fixture.path("dest/bundle.zip")).len(), 3);
}

#[test]
fn an_extract_job_makes_the_archive_s_items_and_undo_takes_them_away() {
    let fixture = Fixture::new();
    fixture.compress("pack", None, TransferConflict::FailIfExists);
    fs::rename(
        fixture.path("dest/pack.zip"),
        fixture.path("source/pack.zip"),
    )
    .unwrap();

    let events = fixture.run(fixture.path("source/pack.zip"), Action::Extract(None));
    // Two top-level members are bundled under the archive's name.
    let made = fixture.path("dest/pack");
    assert!(
        matches!(outcome(&events), Outcome::Done { name, .. } if *name == made),
        "{events:?}"
    );
    assert_eq!(fs::read(made.join("folder/inner.txt")).unwrap(), b"inner");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Progress { progress, .. } if progress.files > 0))
    );

    let undone = fixture
        .journal
        .undo(&Cancel::new(), &Progress::new())
        .unwrap();
    assert_eq!((undone.done, undone.left.len()), (1, 0), "{undone:?}");
    assert!(!made.exists());

    assert_eq!(
        fixture
            .journal
            .redo(&Cancel::new(), &Progress::new())
            .unwrap()
            .done,
        1
    );
    assert_eq!(fs::read(made.join("a.txt")).unwrap(), b"a");
}

#[test]
fn a_locked_archive_asks_for_its_password_and_refuses_a_wrong_one() {
    let fixture = Fixture::new();
    fixture.compress("locked", Some("secret"), TransferConflict::FailIfExists);
    fs::rename(
        fixture.path("dest/locked.zip"),
        fixture.path("source/locked.zip"),
    )
    .unwrap();
    let archive = fixture.path("source/locked.zip");

    for (password, kind) in [
        (None, ErrorKind::PasswordRequired),
        (Some("wrong"), ErrorKind::PasswordIncorrect),
    ] {
        let events = fixture.run(
            archive.clone(),
            Action::Extract(password.map(|password| Password(password.to_owned()))),
        );
        assert!(
            matches!(outcome(&events), Outcome::Failed { error, .. } if error.kind() == kind),
            "{password:?}: {events:?}"
        );
        assert!(
            !fixture.path("dest/locked").exists(),
            "nothing is left behind"
        );
    }
    let events = fixture.run(
        archive,
        Action::Extract(Some(Password("secret".to_owned()))),
    );
    assert!(
        matches!(outcome(&events), Outcome::Done { .. }),
        "{events:?}"
    );
    assert_eq!(fs::read(fixture.path("dest/locked/a.txt")).unwrap(), b"a");
    // The password is never shown.
    assert_eq!(
        format!("{:?}", Password("secret".to_owned())),
        "Password(…)"
    );
}

/// Legacy's `an_archive_replaced_immediately_after_publication_is_not_owned_for_undo`.
#[test]
fn an_archive_replaced_after_it_was_made_is_not_undone() {
    let fixture = Fixture::new();
    fixture.compress("bundle", None, TransferConflict::FailIfExists);
    let archive = fixture.path("dest/bundle.zip");
    fs::remove_file(&archive).unwrap();
    fs::write(&archive, b"someone else's").unwrap();

    let undone = fixture
        .journal
        .undo(&Cancel::new(), &Progress::new())
        .unwrap();
    assert_eq!((undone.done, undone.left.len()), (0, 1), "{undone:?}");
    assert_eq!(fs::read(&archive).unwrap(), b"someone else's");
    assert!(fixture.in_trash().is_empty());
}

/// A RAR archive, read by the separately licensed helper in its sandbox (built and named by
/// scripts/test.sh); skipped where the helper isn't installed.
#[test]
fn a_rar_archive_is_extracted_by_its_helper() {
    if asc_files_engine::adapters::local_operations::archive::rar_helper().is_none() {
        eprintln!("skipped: no RAR helper (ASC_FILES_RAR)");
        return;
    }
    let fixture = Fixture::new();
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../asc-files-rar/tests/fixtures/rar");
    fs::copy(
        fixtures.join("version.rar"),
        fixture.path("source/version.rar"),
    )
    .unwrap();
    fs::copy(
        fixtures.join("encrypted.rar"),
        fixture.path("source/encrypted.rar"),
    )
    .unwrap();

    let events = fixture.run(fixture.path("source/version.rar"), Action::Extract(None));
    let Outcome::Done { name, .. } = outcome(&events) else {
        panic!("{events:?}");
    };
    assert!(
        name.starts_with(fixture.path("dest")) && name.exists(),
        "{name:?}"
    );

    let events = fixture.run(fixture.path("source/encrypted.rar"), Action::Extract(None));
    assert!(
        matches!(outcome(&events), Outcome::Failed { error, .. } if error.kind() == ErrorKind::PasswordRequired),
        "{events:?}"
    );
    let events = fixture.run(
        fixture.path("source/encrypted.rar"),
        Action::Extract(Some(Password("unrar".to_owned()))),
    );
    assert!(
        matches!(outcome(&events), Outcome::Done { .. }),
        "{events:?}"
    );
}
