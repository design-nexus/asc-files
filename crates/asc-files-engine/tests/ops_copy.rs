// SPDX-License-Identifier: MIT
//! Black-box tests of the public copy API, using only `asc_files_engine::ops`.

use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

use asc_files_engine::{
    error::ErrorKind,
    ops::{self, Cancel, Conflict, NameMode, Progress},
};

fn copy(
    source: &Path,
    into: &Path,
    conflict: Conflict,
) -> Result<ops::Copied, asc_files_engine::error::Error> {
    ops::copy(
        source,
        into,
        conflict,
        NameMode::Keep,
        &Cancel::new(),
        &Progress::new(),
    )
}

fn dirs() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("fixture");
    let (source, dest) = (root.path().join("source"), root.path().join("dest"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    (root, source, dest)
}

#[test]
fn a_file_is_copied_with_its_contents_mode_and_time() {
    let (_root, source, dest) = dirs();
    let file = source.join("a.txt");
    fs::write(&file, b"hello").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    let old = filetime(&file, 1_000_000_000);

    let copied = copy(&file, &dest, Conflict::Fail).expect("copy");

    assert_eq!(copied.name, "a.txt");
    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"hello");
    let meta = fs::metadata(dest.join("a.txt")).unwrap();
    assert_eq!(meta.mode() & 0o7777, 0o640);
    assert_eq!(meta.mtime(), old);
    assert_eq!(copied.progress.bytes, 5);
}

#[test]
fn fail_refuses_a_taken_name_and_leaves_what_holds_it() {
    let (_root, source, dest) = dirs();
    fs::write(source.join("a.txt"), b"new").unwrap();
    fs::write(dest.join("a.txt"), b"old").unwrap();

    let error = copy(&source.join("a.txt"), &dest, Conflict::Fail).expect_err("refused");

    assert_eq!(error.kind(), ErrorKind::Exists, "{error}");
    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"old");
    assert_eq!(names(&dest), ["a.txt"], "no staging copy is left behind");
}

#[test]
fn keep_both_takes_the_next_free_number() {
    let (_root, source, dest) = dirs();
    fs::write(source.join("a.txt"), b"new").unwrap();
    fs::write(dest.join("a.txt"), b"old").unwrap();
    fs::write(dest.join("a (1).txt"), b"older").unwrap();

    let copied = copy(&source.join("a.txt"), &dest, Conflict::KeepBoth).expect("copy");

    assert_eq!(copied.name, "a (2).txt");
    assert_eq!(fs::read(dest.join("a (2).txt")).unwrap(), b"new");
    assert_eq!(fs::read(dest.join("a.txt")).unwrap(), b"old");
}

#[test]
fn a_folder_tree_keeps_links_hard_links_xattrs_and_holes() {
    let (_root, source, dest) = dirs();
    let tree = source.join("tree");
    fs::create_dir_all(tree.join("sub")).unwrap();
    fs::write(tree.join("sub/a.txt"), b"a").unwrap();
    fs::hard_link(tree.join("sub/a.txt"), tree.join("a-link.txt")).unwrap();
    std::os::unix::fs::symlink("sub/a.txt", tree.join("to-a")).unwrap();
    rustix::fs::setxattr(
        tree.join("sub/a.txt").as_path(),
        "user.nyx.test",
        b"kept",
        rustix::fs::XattrFlags::empty(),
    )
    .unwrap();
    let sparse = fs::File::create(tree.join("sparse.bin")).unwrap();
    sparse.set_len(64 << 20).unwrap();
    drop(sparse);

    copy(&tree, &dest, Conflict::Fail).expect("copy");

    let out = dest.join("tree");
    assert_eq!(fs::read(out.join("sub/a.txt")).unwrap(), b"a");
    assert_eq!(
        fs::metadata(out.join("sub/a.txt")).unwrap().ino(),
        fs::metadata(out.join("a-link.txt")).unwrap().ino(),
        "hard link kept"
    );
    assert_eq!(
        fs::read_link(out.join("to-a")).unwrap(),
        Path::new("sub/a.txt")
    );
    let mut value = [0u8; 8];
    let len = rustix::fs::getxattr(out.join("sub/a.txt").as_path(), "user.nyx.test", &mut value)
        .expect("xattr kept");
    assert_eq!(&value[..len], b"kept");
    assert!(
        fs::metadata(out.join("sparse.bin")).unwrap().blocks() * 512 < 1 << 20,
        "holes kept"
    );
}

#[test]
fn merge_puts_new_items_into_the_folder_already_there() {
    let (_root, source, dest) = dirs();
    fs::create_dir_all(source.join("photos")).unwrap();
    fs::create_dir_all(dest.join("photos")).unwrap();
    fs::write(source.join("photos/new.jpg"), b"new").unwrap();
    fs::write(dest.join("photos/kept.jpg"), b"kept").unwrap();

    copy(&source.join("photos"), &dest, Conflict::Merge).expect("merge");

    assert_eq!(names(&dest.join("photos")), ["kept.jpg", "new.jpg"]);
}

/// Records what a merge does with a file that clashes inside it, rather than assuming.
#[test]
fn merge_with_a_clashing_file_inside() {
    let (_root, source, dest) = dirs();
    fs::create_dir_all(source.join("photos")).unwrap();
    fs::create_dir_all(dest.join("photos")).unwrap();
    fs::write(source.join("photos/same.jpg"), b"incoming").unwrap();
    fs::write(dest.join("photos/same.jpg"), b"existing").unwrap();

    let result = copy(&source.join("photos"), &dest, Conflict::Merge);

    eprintln!(
        "OBSERVED merge clash: result={:?} names={:?} same.jpg={:?}",
        result
            .as_ref()
            .map(|c| &c.name)
            .map_err(|e| (e.kind(), e.message().to_owned())),
        names(&dest.join("photos")),
        fs::read(dest.join("photos/same.jpg")).map(String::from_utf8)
    );
    assert_eq!(
        fs::read(dest.join("photos/same.jpg")).unwrap(),
        b"existing",
        "a merge without a replace plan never overwrites a file"
    );
}

#[test]
fn a_cancelled_copy_leaves_nothing_behind() {
    let (_root, source, dest) = dirs();
    let tree = source.join("big");
    fs::create_dir_all(&tree).unwrap();
    for i in 0..200 {
        fs::write(tree.join(format!("{i}.bin")), vec![7u8; 64 * 1024]).unwrap();
    }
    let cancel = Cancel::new();
    cancel.cancel();

    let error = ops::copy(
        &tree,
        &dest,
        Conflict::Fail,
        NameMode::Keep,
        &cancel,
        &Progress::new(),
    )
    .expect_err("cancelled");

    assert_eq!(error.kind(), ErrorKind::Cancelled, "{error}");
    assert!(names(&dest).is_empty(), "left: {:?}", names(&dest));
}

#[test]
fn a_folder_is_not_copied_into_itself() {
    let (_root, source, _dest) = dirs();
    let folder = source.join("folder");
    fs::create_dir_all(folder.join("inner")).unwrap();

    let result = copy(&folder, &folder.join("inner"), Conflict::Fail);

    assert!(result.is_err(), "copied into itself: {result:?}");
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn filetime(path: &Path, seconds: i64) -> i64 {
    let file = fs::File::options().write(true).open(path).unwrap();
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds as u64);
    file.set_modified(time).unwrap();
    seconds
}
