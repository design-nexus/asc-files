// SPDX-License-Identifier: MIT
//! A replace whose original cannot go to the Trash leaves the original in place. Its own test
//! binary, since it makes the test home's Trash unusable for the whole process while it runs.

use std::{fs, os::unix::fs::MetadataExt, path::PathBuf};

use asc_files_engine::ops::{self, Cancel, Progress};

/// Turns the test home's Trash into a plain file until dropped, so no Trash folder can be made.
struct NoTrash(PathBuf);

impl NoTrash {
    fn new() -> Self {
        let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME set by scripts/test.sh");
        assert!(
            data.contains("asc-files-test-home"),
            "refusing to touch a real Trash: {data}"
        );
        let trash = PathBuf::from(&data).join("Trash");
        fs::create_dir_all(&data).unwrap();
        let _ = fs::remove_dir_all(&trash);
        fs::write(&trash, b"not a folder").unwrap();
        Self(trash)
    }
}

impl Drop for NoTrash {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn a_replace_whose_original_cannot_be_trashed_leaves_it_in_place() {
    let _no_trash = NoTrash::new();
    let root = tempfile::tempdir_in(std::env::var("XDG_DATA_HOME").unwrap()).unwrap();
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("a.txt"), b"new").unwrap();
    fs::write(dest.join("a.txt"), b"old").unwrap();
    let old_inode = fs::metadata(dest.join("a.txt")).unwrap().ino();

    let result = ops::replace(
        &source.join("a.txt"),
        &dest,
        &Cancel::new(),
        &Progress::new(),
    );

    assert!(result.is_err(), "the Trash is unusable: {result:?}");
    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"old");
    assert_eq!(
        fs::metadata(dest.join("a.txt")).unwrap().ino(),
        old_inode,
        "the original is back at its name"
    );
    let left: Vec<_> = fs::read_dir(&dest)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["a.txt"], "the copy and its stage are gone");
}
