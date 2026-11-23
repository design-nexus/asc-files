// SPDX-License-Identifier: MIT
//! Black-box tests of `asc_files_engine::trash`. They refuse to run unless `XDG_DATA_HOME` is the
//! test home set by scripts/test.sh, and they only ever delete their own entries: a listing also
//! shows each drive's real Trash, which tests must not touch.

use std::{fs, os::unix::fs::MetadataExt, path::Path};

use asc_files_engine::{error::ErrorKind, trash};

fn isolated_dir() -> tempfile::TempDir {
    let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME set by scripts/test.sh");
    assert!(
        data.contains("asc-files-test-home"),
        "refusing to touch a real Trash: {data}"
    );
    fs::create_dir_all(&data).unwrap();
    tempfile::tempdir_in(&data).unwrap()
}

fn mine(root: &Path) -> Vec<trash::Entry> {
    trash::list()
        .into_iter()
        .filter(|entry| entry.original.starts_with(root))
        .collect()
}

#[test]
fn an_item_goes_to_the_trash_lists_and_comes_back() {
    let root = isolated_dir();
    let item = root.path().join("notes 1%.txt");
    fs::write(&item, b"one").unwrap();

    let trashed = trash::put(&item).expect("put");

    assert!(!item.exists());
    let entries = mine(root.path());
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.original, item);
    assert_eq!(entry.file, trashed.file);
    assert_eq!(
        entry.deleted.len(),
        "2026-01-01T00:00:00".len(),
        "{}",
        entry.deleted
    );
    let back = trash::restore(entry).expect("restore");
    assert_eq!(back, item);
    assert_eq!(fs::read(&item).unwrap(), b"one");
    assert!(!trashed.info.exists(), "the record goes with the restore");
    assert!(mine(root.path()).is_empty());
}

#[test]
fn a_second_item_of_the_same_name_takes_a_number() {
    let root = isolated_dir();
    let item = root.path().join("same.txt");
    fs::write(&item, b"first").unwrap();
    let first = trash::put(&item).unwrap();
    fs::write(&item, b"second").unwrap();
    let second = trash::put(&item).unwrap();

    assert_ne!(first.file, second.file);
    assert_eq!(fs::read(&first.file).unwrap(), b"first");
    assert_eq!(fs::read(&second.file).unwrap(), b"second");
    assert_eq!(mine(root.path()).len(), 2);
    trash::delete_all(mine(root.path()));
}

#[test]
fn restore_never_replaces_what_now_has_the_name() {
    let root = isolated_dir();
    let item = root.path().join("a.txt");
    fs::write(&item, b"old").unwrap();
    trash::put(&item).unwrap();
    fs::write(&item, b"newer").unwrap();
    let entry = mine(root.path()).pop().unwrap();

    let error = trash::restore(&entry).expect_err("refused");

    assert_eq!(error.kind(), ErrorKind::Exists, "{error}");
    assert_eq!(fs::read(&item).unwrap(), b"newer");
    assert_eq!(
        fs::read(&entry.file).unwrap(),
        b"old",
        "the entry stays in the Trash"
    );
    trash::delete(&entry).unwrap();
}

#[test]
fn restore_and_delete_refuse_an_entry_that_changed_since_it_was_listed() {
    let root = isolated_dir();
    let item = root.path().join("b.txt");
    fs::write(&item, b"listed").unwrap();
    trash::put(&item).unwrap();
    let entry = mine(root.path()).pop().unwrap();
    fs::remove_file(&entry.file).unwrap();
    fs::write(&entry.file, b"someone else's").unwrap();

    assert!(trash::restore(&entry).is_err());
    assert!(trash::delete(&entry).is_err());
    assert_eq!(
        fs::read(&entry.file).unwrap(),
        b"someone else's",
        "nothing done to the new item"
    );
    let fresh = mine(root.path()).pop().unwrap();
    trash::delete(&fresh).unwrap();
}

#[test]
fn delete_removes_the_item_its_record_and_its_cached_size() {
    let root = isolated_dir();
    let folder = root.path().join("folder");
    fs::create_dir_all(folder.join("inner")).unwrap();
    fs::write(folder.join("inner/x"), b"x").unwrap();
    let trashed = trash::put(&folder).unwrap();
    let entry = mine(root.path()).pop().unwrap();
    let cache = entry.trash.join("directorysizes");
    let name = trashed
        .file
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    fs::write(
        &cache,
        format!("4096 1795000000000 {name}\n4096 1795000000000 keep-me\n"),
    )
    .unwrap();

    trash::delete(&entry).unwrap();

    assert!(!trashed.file.exists() && !trashed.info.exists());
    assert_eq!(
        fs::read_to_string(&cache).unwrap(),
        "4096 1795000000000 keep-me\n"
    );
}

#[test]
fn a_record_that_is_not_a_trash_record_is_not_listed() {
    let root = isolated_dir();
    let item = root.path().join("c.txt");
    fs::write(&item, b"c").unwrap();
    let trashed = trash::put(&item).unwrap();
    let real = fs::read(&trashed.info).unwrap();
    fs::write(&trashed.info, b"[Trash Info]\nPath=relative/in/home.txt\n").unwrap();

    assert!(
        !trash::list().iter().any(|entry| entry.info == trashed.info),
        "a relative Path is not valid in the home Trash"
    );
    fs::write(&trashed.info, real).unwrap();
    trash::delete(&mine(root.path()).pop().unwrap()).unwrap();
}

#[test]
fn an_item_on_another_drive_goes_to_that_drives_trash() {
    let data = std::env::var("XDG_DATA_HOME").unwrap();
    assert!(data.contains("asc-files-test-home"));
    let shm = Path::new("/dev/shm");
    let Ok(root) = tempfile::tempdir_in(shm) else {
        eprintln!("SKIP: no /dev/shm");
        return;
    };
    if fs::metadata(shm).unwrap().dev() == fs::metadata(&data).unwrap().dev() {
        eprintln!("SKIP: /dev/shm is on the home drive here");
        return;
    }
    let uid = rustix::process::getuid().as_raw();
    let drive_trash = shm.join(format!(".Trash-{uid}"));
    let existed = drive_trash.exists();
    let item = root.path().join("on-shm.txt");
    fs::write(&item, b"shm").unwrap();

    let trashed = trash::put(&item).expect("put");

    assert!(trashed.file.starts_with(&drive_trash), "{:?}", trashed.file);
    let record = fs::read_to_string(&trashed.info).unwrap();
    let relative = item.strip_prefix(shm).unwrap().to_str().unwrap();
    assert!(
        record.contains(&format!("Path={relative}\n")),
        "a drive's record is relative: {record}"
    );
    assert_eq!(fs::metadata(&drive_trash).unwrap().mode() & 0o777, 0o700);
    let entry = mine(root.path())
        .pop()
        .expect("listed with its absolute original");
    assert_eq!(entry.original, item);
    trash::restore(&entry).expect("restore");
    assert_eq!(fs::read(&item).unwrap(), b"shm");

    if !existed {
        fs::remove_dir_all(&drive_trash).unwrap();
    }
}
