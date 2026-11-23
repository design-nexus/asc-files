// SPDX-License-Identifier: MIT
//! Listings and watches through the public API, on throwaway folders.

use std::{
    fs,
    path::Path,
    sync::{Arc, mpsc},
    time::Duration,
};

use asc_files_engine::listing::{
    Change, Kind, Listing, Options, Order, READERS_PER_DRIVE, Readers, SortKey, Watch,
};

fn names(listing: &Listing) -> Vec<String> {
    listing
        .window(0, listing.len())
        .into_iter()
        .map(|row| row.name.to_string_lossy().into_owned())
        .collect()
}

fn folder() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for name in ["b.txt", "A.txt", "file_10", "file_2", ".dotfile", "secret"] {
        fs::write(root.path().join(name), name).unwrap();
    }
    fs::create_dir(root.path().join("zeta")).unwrap();
    fs::create_dir(root.path().join("alpha")).unwrap();
    fs::write(root.path().join(".hidden"), "secret\n").unwrap();
    // A copy in progress, as the engine stages it.
    fs::write(
        root.path().join(".asc-files-replacement-0123456789abcdef"),
        "",
    )
    .unwrap();
    std::os::unix::fs::symlink(root.path().join("alpha"), root.path().join("to-alpha")).unwrap();
    root
}

#[test]
fn a_listing_puts_folders_first_in_finder_order_and_hides_what_it_should() {
    let root = folder();

    let listing = Listing::read(root.path(), Options::default()).unwrap();

    assert_eq!(
        names(&listing),
        [
            "alpha", "to-alpha", "zeta", "A.txt", "b.txt", "file_2", "file_10"
        ]
    );
    let rows = listing.window(0, 3);
    assert_eq!(rows[0].kind, Kind::Folder);
    assert_eq!((rows[1].kind, rows[1].link_to_folder), (Kind::Link, true));
    assert_eq!(listing.generation(), 1);
}

#[test]
fn hidden_entries_are_listed_when_asked_and_engine_names_among_them() {
    let root = folder();
    let options = Options {
        show_hidden: true,
        ..Options::default()
    };

    let listing = Listing::read(root.path(), options).unwrap();

    let listed = names(&listing);
    for name in [
        ".dotfile",
        ".hidden",
        "secret",
        ".asc-files-replacement-0123456789abcdef",
    ] {
        assert!(listed.contains(&name.to_owned()), "{listed:?}");
    }
    let secret = listing.position("secret".as_ref()).unwrap();
    assert!(listing.window(secret, 1)[0].hidden);
}

#[test]
fn size_and_date_orders_stat_every_entry_and_can_run_descending() {
    let root = tempfile::tempdir().unwrap();
    for (name, size) in [("small", 1), ("big", 300), ("middle", 20)] {
        fs::write(root.path().join(name), vec![0; size]).unwrap();
    }
    fs::create_dir(root.path().join("folder")).unwrap();
    let by_size = |descending| Options {
        order: Order {
            key: SortKey::Size,
            descending,
            folders_first: true,
        },
        ..Options::default()
    };

    let ascending = Listing::read(root.path(), by_size(false)).unwrap();
    let descending = Listing::read(root.path(), by_size(true)).unwrap();

    assert_eq!(names(&ascending), ["folder", "small", "middle", "big"]);
    assert_eq!(
        names(&descending),
        ["folder", "big", "middle", "small"],
        "folders stay first either way"
    );

    let old = root.path().join("small");
    let past = std::time::SystemTime::now() - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(past)
        .unwrap();
    let by_date = Listing::read(
        root.path(),
        Options {
            order: Order {
                key: SortKey::Modified,
                descending: false,
                folders_first: false,
            },
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(names(&by_date)[0], "small");
}

#[test]
fn a_window_is_clamped_and_says_when_an_entry_went_since_the_read() {
    let root = folder();
    let listing = Listing::read(root.path(), Options::default()).unwrap();

    assert!(listing.window(listing.len() + 5, 10).is_empty());
    assert_eq!(listing.window(5, 100).len(), listing.len() - 5);

    fs::remove_file(root.path().join("b.txt")).unwrap();
    let at = listing.position("b.txt".as_ref()).unwrap();
    let row = &listing.window(at, 1)[0];
    assert_eq!(row.stat, None);
    let a = &listing.window(listing.position("A.txt".as_ref()).unwrap(), 1)[0];
    assert_eq!(a.stat.unwrap().size, 5);
}

#[test]
fn reading_again_or_under_new_options_is_a_new_generation() {
    let root = folder();
    let mut listing = Listing::read(root.path(), Options::default()).unwrap();
    fs::write(root.path().join("new.txt"), "").unwrap();

    listing.refresh().unwrap();
    assert_eq!(listing.generation(), 2);
    assert!(listing.position("new.txt".as_ref()).is_some());

    listing
        .set_options(Options {
            show_hidden: true,
            ..Options::default()
        })
        .unwrap();
    assert_eq!(listing.generation(), 3);
    assert!(listing.position(".dotfile".as_ref()).is_some());
}

#[test]
fn a_folder_that_cannot_be_read_is_an_error_and_a_failed_refresh_keeps_the_listing() {
    let root = tempfile::tempdir().unwrap();
    assert!(Listing::read(&root.path().join("missing"), Options::default()).is_err());

    let inner = root.path().join("inner");
    fs::create_dir(&inner).unwrap();
    fs::write(inner.join("kept"), "").unwrap();
    let mut listing = Listing::read(&inner, Options::default()).unwrap();
    fs::remove_dir_all(&inner).unwrap();
    assert!(listing.refresh().is_err());
    assert_eq!((listing.len(), listing.generation()), (1, 1));
}

fn watch(folder: &Path) -> (Watch, mpsc::Receiver<Change>) {
    let (send, changes) = mpsc::channel();
    let send = std::sync::Mutex::new(send);
    let watch = Watch::start(folder, move |change| {
        let _ = send.lock().unwrap().send(change);
    });
    assert!(watch.follows_changes());
    (watch, changes)
}

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn an_outside_change_is_told_once_per_burst() {
    let root = tempfile::tempdir().unwrap();
    let (_watch, changes) = watch(root.path());

    for n in 0..200 {
        fs::write(root.path().join(format!("f{n}")), "x").unwrap();
    }

    assert_eq!(changes.recv_timeout(WAIT), Ok(Change::Changed));
    let mut more = 0;
    while changes.recv_timeout(Duration::from_millis(400)).is_ok() {
        more += 1;
    }
    assert!(more <= 3, "200 writes were told {} times", more + 1);
}

#[test]
fn a_file_still_being_written_is_told_when_it_is_closed() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let mut file = fs::File::create(root.path().join("growing")).unwrap();
    let (_watch, changes) = watch(root.path());

    file.write_all(b"more").unwrap();
    assert!(
        changes.recv_timeout(Duration::from_millis(500)).is_err(),
        "a write alone changes nothing a listing shows"
    );
    drop(file);
    assert_eq!(changes.recv_timeout(WAIT), Ok(Change::Changed));
}

#[test]
fn a_watched_folder_that_is_deleted_sends_the_view_to_the_nearest_folder_left() {
    let root = tempfile::tempdir().unwrap();
    let watched = root.path().join("a/b");
    fs::create_dir_all(&watched).unwrap();
    let (_watch, changes) = watch(&watched);

    fs::remove_dir_all(root.path().join("a")).unwrap();

    let told = std::iter::from_fn(|| changes.recv_timeout(WAIT).ok())
        .find(|change| *change != Change::Changed);
    // The removal may still be under way when it is told, so `a` may yet be standing.
    let Some(Change::Gone { nearest }) = told else {
        panic!("{told:?}");
    };
    assert!(
        nearest == root.path() || nearest == root.path().join("a"),
        "{nearest:?}"
    );
    assert!(nearest.is_dir() || nearest == root.path().join("a"));
}

#[test]
fn a_watch_stops_promptly_when_dropped() {
    let root = tempfile::tempdir().unwrap();
    let (watch, _changes) = watch(root.path());
    let started = std::time::Instant::now();
    drop(watch);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn a_drive_gives_out_a_bounded_number_of_read_places() {
    let root = tempfile::tempdir().unwrap();
    let readers = Arc::new(Readers::default());

    let held: Vec<_> = (0..READERS_PER_DRIVE)
        .map(|_| readers.claim(root.path()).expect("a place"))
        .collect();
    assert!(readers.claim(root.path()).is_none());
    drop(held);
    assert!(readers.claim(root.path()).is_some());
}
