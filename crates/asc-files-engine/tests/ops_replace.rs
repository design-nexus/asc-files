// SPDX-License-Identifier: MIT
//! Black-box tests of trash and replace, using only `asc_files_engine::ops`.
//! They refuse to run unless `XDG_DATA_HOME` is the spike's isolated home (see test-engine.sh).

use std::{fs, os::unix::fs::MetadataExt, path::Path};

use asc_files_engine::ops::{self, Cancel, Progress};

fn isolated_trash() -> std::path::PathBuf {
    let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME set by test-engine.sh");
    assert!(
        data.contains("asc-files-test-home"),
        "refusing to touch a real Trash: {data}"
    );
    Path::new(&data).join("Trash")
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn home_fs_dir() -> tempfile::TempDir {
    let trash = isolated_trash();
    fs::create_dir_all(&trash).unwrap();
    tempfile::tempdir_in(trash.parent().unwrap()).unwrap()
}

#[test]
fn trash_records_the_original_path_and_numbers_a_taken_name() {
    let trash = isolated_trash();
    let root = home_fs_dir();
    let first = root.path().join("report 1%.txt");
    fs::write(&first, b"one").unwrap();

    let one = ops::trash(&first).expect("trash");
    fs::write(&first, b"two").unwrap();
    let two = ops::trash(&first).expect("trash again");

    assert!(!first.exists());
    assert_eq!(fs::read(&one.file).unwrap(), b"one");
    assert_eq!(fs::read(&two.file).unwrap(), b"two");
    assert_ne!(one.file, two.file, "the second takes a numbered name");
    assert!(one.file.starts_with(trash.join("files")));
    let info = fs::read_to_string(&one.info).unwrap();
    let encoded = first
        .to_str()
        .unwrap()
        .replace('%', "%25")
        .replace(' ', "%20");
    assert!(info.starts_with("[Trash Info]\n"), "{info}");
    assert!(info.contains(&format!("Path={encoded}\n")), "{info}");
    assert!(
        info.lines().any(|line| line.starts_with("DeletionDate=")
            && line.len() == "DeletionDate=2026-01-01T00:00:00".len()),
        "{info}"
    );
}

#[test]
fn replace_swaps_in_the_copy_and_trashes_the_original() {
    let root = home_fs_dir();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("a.txt"), b"new").unwrap();
    fs::write(dest.join("a.txt"), b"old").unwrap();
    let old_inode = fs::metadata(dest.join("a.txt")).unwrap().ino();

    let replaced = ops::replace(
        &source.join("a.txt"),
        &dest,
        &Cancel::new(),
        &Progress::new(),
    )
    .expect("replace");

    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"new");
    assert_eq!(names(&dest), ["a.txt"], "no stage left behind");
    let original = replaced.original.expect("the original went to the Trash");
    assert_eq!(fs::read(&original.file).unwrap(), b"old");
    assert_eq!(
        fs::metadata(&original.file).unwrap().ino(),
        old_inode,
        "the original itself, not a copy"
    );
    assert_eq!(
        fs::read(source.join("a.txt")).unwrap(),
        b"new",
        "a replace copies; the source stays"
    );
}

#[test]
fn replace_swaps_a_whole_folder() {
    let root = home_fs_dir();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(source.join("photos")).unwrap();
    fs::create_dir_all(dest.join("photos")).unwrap();
    fs::write(source.join("photos/new.jpg"), b"new").unwrap();
    fs::write(dest.join("photos/old.jpg"), b"old").unwrap();

    let replaced = ops::replace(
        &source.join("photos"),
        &dest,
        &Cancel::new(),
        &Progress::new(),
    )
    .expect("replace");

    assert_eq!(names(&dest.join("photos")), ["new.jpg"]);
    assert_eq!(names(&replaced.original.unwrap().file), ["old.jpg"]);
    assert_eq!(names(&dest), ["photos"]);
}

#[test]
fn replace_with_nothing_at_the_name_is_a_plain_copy() {
    let root = home_fs_dir();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("b.txt"), b"b").unwrap();

    let replaced = ops::replace(
        &source.join("b.txt"),
        &dest,
        &Cancel::new(),
        &Progress::new(),
    )
    .expect("copy");

    assert!(replaced.original.is_none());
    assert_eq!(fs::read(dest.join("b.txt")).unwrap(), b"b");
}
