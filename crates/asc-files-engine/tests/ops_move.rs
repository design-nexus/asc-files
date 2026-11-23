// SPDX-License-Identifier: MIT
//! Black-box tests of the public move API, using only `asc_files_engine::ops`.

use std::{fs, os::unix::fs::MetadataExt, path::Path};

use asc_files_engine::{
    error::ErrorKind,
    ops::{self, Cancel, Progress},
};

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A disposable folder on another filesystem than the test's temp dir, or None to skip.
fn other_filesystem(than: &Path) -> Option<tempfile::TempDir> {
    let shm = Path::new("/dev/shm");
    let dir = tempfile::tempdir_in(shm).ok()?;
    (fs::metadata(dir.path()).ok()?.dev() != fs::metadata(than).ok()?.dev()).then_some(dir)
}

#[test]
fn a_move_within_a_filesystem_is_a_rename() {
    let root = tempfile::tempdir().unwrap();
    let (source, dest) = (root.path().join("a.txt"), root.path().join("dest"));
    fs::write(&source, b"hello").unwrap();
    fs::create_dir(&dest).unwrap();
    let inode = fs::metadata(&source).unwrap().ino();

    let moved = ops::move_item(&source, &dest, &Cancel::new(), &Progress::new()).expect("move");

    assert!(!moved.copied);
    assert!(!source.exists());
    assert_eq!(
        fs::metadata(dest.join("a.txt")).unwrap().ino(),
        inode,
        "renamed, not copied"
    );
}

#[test]
fn a_move_never_replaces_what_holds_the_name() {
    let root = tempfile::tempdir().unwrap();
    let (source, dest) = (root.path().join("a.txt"), root.path().join("dest"));
    fs::write(&source, b"new").unwrap();
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("a.txt"), b"old").unwrap();

    let error =
        ops::move_item(&source, &dest, &Cancel::new(), &Progress::new()).expect_err("refused");

    assert_eq!(error.kind(), ErrorKind::Exists, "{error}");
    assert_eq!(fs::read(&source).unwrap(), b"new");
    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"old");
}

#[test]
fn a_move_across_filesystems_copies_then_deletes_the_source() {
    let root = tempfile::tempdir().unwrap();
    let Some(other) = other_filesystem(root.path()) else {
        eprintln!("SKIP: no second filesystem at /dev/shm");
        return;
    };
    let tree = root.path().join("tree");
    fs::create_dir_all(tree.join("sub")).unwrap();
    fs::write(tree.join("sub/a.txt"), b"a").unwrap();
    fs::write(tree.join("b.txt"), vec![3u8; 1 << 20]).unwrap();
    std::os::unix::fs::symlink("b.txt", tree.join("to-b")).unwrap();

    let moved =
        ops::move_item(&tree, other.path(), &Cancel::new(), &Progress::new()).expect("move");

    assert!(moved.copied);
    assert!(moved.kept.is_empty(), "kept: {:?}", moved.kept);
    assert!(!tree.exists(), "source removed once the copy was confirmed");
    let out = other.path().join("tree");
    assert_eq!(fs::read(out.join("sub/a.txt")).unwrap(), b"a");
    assert_eq!(fs::read(out.join("b.txt")).unwrap().len(), 1 << 20);
    assert_eq!(fs::read_link(out.join("to-b")).unwrap(), Path::new("b.txt"));
    // File bytes, plus the five bytes of the link's target, which the engine counts as copied.
    assert_eq!(moved.progress.bytes, (1 << 20) + 1 + "b.txt".len() as u64);
}

#[test]
fn a_cancelled_move_across_filesystems_keeps_the_source() {
    let root = tempfile::tempdir().unwrap();
    let Some(other) = other_filesystem(root.path()) else {
        eprintln!("SKIP: no second filesystem at /dev/shm");
        return;
    };
    let tree = root.path().join("tree");
    fs::create_dir_all(&tree).unwrap();
    for i in 0..50 {
        fs::write(tree.join(format!("{i}.bin")), vec![1u8; 32 * 1024]).unwrap();
    }
    let cancel = Cancel::new();
    cancel.cancel();

    let error =
        ops::move_item(&tree, other.path(), &cancel, &Progress::new()).expect_err("cancelled");

    assert_eq!(error.kind(), ErrorKind::Cancelled, "{error}");
    assert_eq!(names(&tree).len(), 50, "every source entry stays");
    assert!(
        names(other.path()).is_empty(),
        "nothing published: {:?}",
        names(other.path())
    );
}
