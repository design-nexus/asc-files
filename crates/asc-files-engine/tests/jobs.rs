// SPDX-License-Identifier: MIT
//! Black-box tests of the batch runner, using only `asc_files_engine::jobs` and `ops`.

use std::{fs, path::Path, sync::mpsc, time::Duration};

use asc_files_engine::{
    error::ErrorKind,
    jobs::{self, Action, Event, Item, Outcome, Request},
    ops::{Conflict, NameMode},
};

fn isolated_home_dir() -> tempfile::TempDir {
    let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME set by test-engine.sh");
    assert!(
        data.contains("asc-files-test-home"),
        "refusing to touch a real Trash: {data}"
    );
    fs::create_dir_all(&data).unwrap();
    tempfile::tempdir_in(&data).unwrap()
}

fn run(request: Request) -> Vec<Event> {
    let (tx, rx) = mpsc::channel();
    let handle = jobs::start(request, tx);
    handle.join();
    rx.try_iter().collect()
}

fn outcomes(events: &[Event]) -> Vec<&Outcome> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ItemDone { outcome, .. } => Some(outcome),
            _ => None,
        })
        .collect()
}

fn item(source: &Path, action: Action) -> Item {
    Item::new(source, action)
}

/// The legacy app's bug: Replace in a paste where only some items clash did nothing.
#[test]
fn replace_in_a_batch_where_only_some_items_clash_places_every_item() {
    for free_first in [false, true] {
        let root = isolated_home_dir();
        let (source, dest) = (root.path().join("source"), root.path().join("dest"));
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(source.join("a.txt"), b"new a").unwrap();
        fs::write(source.join("b.txt"), b"new b").unwrap();
        fs::write(dest.join("a.txt"), b"old a").unwrap();
        let mut items = vec![
            item(&source.join("a.txt"), Action::Replace),
            item(&source.join("b.txt"), Action::Replace),
        ];
        if free_first {
            items.reverse();
        }

        let events = run(Request {
            into: dest.clone(),
            items,
            names: NameMode::Keep,
            record: None,
            durable: None,
        });

        assert!(
            outcomes(&events)
                .iter()
                .all(|o| matches!(o, Outcome::Done { .. })),
            "free_first={free_first}: {events:?}"
        );
        assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"new a");
        assert_eq!(fs::read(dest.join("b.txt")).unwrap(), b"new b");
    }
}

#[test]
fn a_failed_item_ends_only_itself() {
    let root = tempfile::tempdir().unwrap();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(source.join(name), name).unwrap();
    }
    fs::write(dest.join("b.txt"), b"kept").unwrap();

    let events = run(Request {
        into: dest.clone(),
        items: ["a.txt", "b.txt", "c.txt"]
            .map(|n| item(&source.join(n), Action::Copy(Conflict::Fail)))
            .to_vec(),
        names: NameMode::Keep,
        record: None,
        durable: None,
    });

    let outcomes = outcomes(&events);
    assert!(matches!(outcomes[0], Outcome::Done { .. }));
    assert!(
        matches!(outcomes[1], Outcome::Failed { error, .. } if error.kind() == ErrorKind::Exists),
        "{:?}",
        outcomes[1]
    );
    assert!(
        matches!(outcomes[2], Outcome::Done { .. }),
        "the item after a failure still runs"
    );
    assert_eq!(fs::read(dest.join("b.txt")).unwrap(), b"kept");
    assert!(dest.join("c.txt").exists());
    assert!(
        matches!(
            events.last(),
            Some(Event::Finished {
                done: 2,
                failed: 1,
                not_attempted: 0,
                cancelled: false,
                ..
            })
        ),
        "{:?}",
        events.last()
    );
}

#[test]
fn events_start_once_report_each_item_once_and_finish_last() {
    let root = tempfile::tempdir().unwrap();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    let items: Vec<_> = (0..5)
        .map(|i| {
            let path = source.join(format!("{i}.txt"));
            fs::write(&path, b"x").unwrap();
            item(&path, Action::Move)
        })
        .collect();

    let events = run(Request {
        into: dest,
        items,
        names: NameMode::Keep,
        record: None,
        durable: None,
    });

    // Another test writing to the same drive may hold it: then one Queued comes first.
    let events = match events.first() {
        Some(Event::Queued { ahead, .. }) => {
            assert!(*ahead >= 1);
            &events[1..]
        }
        _ => &events[..],
    };
    assert!(matches!(
        events.first(),
        Some(Event::Started { total: 5, .. })
    ));
    assert!(matches!(
        events.last(),
        Some(Event::Finished { done: 5, .. })
    ));
    let indices: Vec<usize> = events
        .iter()
        .filter_map(|event| match event {
            Event::ItemDone { index, .. } => Some(*index),
            _ => None,
        })
        .collect();
    assert_eq!(indices, [0, 1, 2, 3, 4]);
    let jobs: std::collections::HashSet<_> = events
        .iter()
        .map(|event| match event {
            Event::Queued { job, .. }
            | Event::Started { job, .. }
            | Event::Progress { job, .. }
            | Event::Flushing { job }
            | Event::ItemDone { job, .. }
            | Event::Finished { job, .. } => *job,
        })
        .collect();
    assert_eq!(jobs.len(), 1, "every event names the one job");
}

#[test]
fn cancelling_reports_the_rest_as_not_attempted() {
    let root = tempfile::tempdir().unwrap();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    let items: Vec<_> = (0..40)
        .map(|i| {
            let folder = source.join(format!("f{i}"));
            fs::create_dir_all(&folder).unwrap();
            for j in 0..20 {
                fs::write(folder.join(format!("{j}.bin")), vec![9u8; 16 * 1024]).unwrap();
            }
            item(&folder, Action::Copy(Conflict::Fail))
        })
        .collect();
    let (tx, rx) = mpsc::channel();
    let handle = jobs::start(
        Request {
            into: dest.clone(),
            items,
            names: NameMode::Keep,
            record: None,
            durable: None,
        },
        tx,
    );
    // Cancel once the first item has finished.
    loop {
        match rx.recv_timeout(Duration::from_secs(30)).expect("events") {
            Event::ItemDone { .. } => break,
            _ => continue,
        }
    }
    handle.cancel();
    handle.join();
    let mut events: Vec<_> = rx.try_iter().collect();
    let finished = events.pop();

    let not_attempted = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::ItemDone {
                    outcome: Outcome::NotAttempted,
                    ..
                }
            )
        })
        .count();
    assert!(not_attempted > 0, "some items were never started");
    assert!(
        matches!(
            finished,
            Some(Event::Finished {
                cancelled: true,
                ..
            })
        ),
        "{finished:?}"
    );
    let published = fs::read_dir(&dest).unwrap().count();
    let done = 1 + events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::ItemDone {
                    outcome: Outcome::Done { .. },
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        published, done,
        "only completed items are published; a cancelled one leaves nothing"
    );
}

#[test]
fn progress_never_goes_backwards_within_an_item() {
    let root = tempfile::tempdir().unwrap();
    let Ok(other) = tempfile::tempdir_in("/dev/shm") else {
        return;
    };
    // Fit the copy to the target: a container's /dev/shm is often only 64 MB.
    let free = rustix::fs::statvfs(other.path())
        .map(|fs| fs.f_bavail * fs.f_frsize)
        .unwrap_or(0);
    let size = (256u64 << 20).min(free / 4);
    if size < 8 << 20 {
        eprintln!("SKIP: only {free} bytes free on /dev/shm");
        return;
    }
    let big = root.path().join("big.bin");
    fs::write(&big, vec![5u8; size as usize]).unwrap();

    let events = run(Request {
        into: other.path().to_path_buf(),
        items: vec![item(&big, Action::Copy(Conflict::Fail))],
        names: NameMode::Keep,
        record: None,
        durable: None,
    });

    let samples: Vec<u64> = events
        .iter()
        .filter_map(|event| match event {
            Event::Progress { progress, .. } => Some(progress.bytes),
            _ => None,
        })
        .collect();
    eprintln!("OBSERVED progress samples for a {size}-byte cross-fs copy: {samples:?}");
    assert!(
        samples.windows(2).all(|pair| pair[0] <= pair[1]),
        "{samples:?}"
    );
    let outcome = outcomes(&events)[0];
    assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
}

#[test]
fn a_durable_batch_is_synced_before_it_finishes() {
    let root = isolated_home_dir();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("a.txt"), b"a").unwrap();
    let settled = |durable, action: Action| {
        let events = run(Request {
            into: dest.clone(),
            items: vec![item(&source.join("a.txt"), action)],
            names: NameMode::Fix,
            record: None,
            durable,
        });
        events.into_iter().find_map(|event| match event {
            Event::Finished { settled, .. } => Some(settled),
            _ => None,
        })
    };

    assert_eq!(
        settled(Some(true), Action::Copy(Conflict::KeepBoth)),
        Some(Some(Ok(())))
    );
    assert_eq!(
        settled(Some(false), Action::Copy(Conflict::KeepBoth)),
        Some(None)
    );
    assert_eq!(
        settled(Some(true), Action::Trash),
        Some(None),
        "a batch that only trashes writes nothing to its destination"
    );
}
