// SPDX-License-Identifier: MIT
//! A merge into a folder on a drive that stores names its own way meets clashes by those names.

use super::fs_quirks::{FsQuirks, Quirks};
use super::*;
use crate::merge::{self, Clash, MergePlan};

#[test]
fn inner_clashes_are_found_and_met_by_the_names_the_drive_stores()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let (from, into) = (root.path().join("from"), root.path().join("into"));
    fs::create_dir_all(from.join("photos/2024"))?;
    fs::create_dir_all(into.join("photos/2024"))?;
    // `a:b.txt` lands on `a_b.txt`, which the FAT stick already holds.
    fs::write(from.join("photos/2024/a:b.txt"), b"incoming")?;
    fs::write(into.join("photos/2024/a_b.txt"), b"existing")?;
    fs::write(from.join("photos/new.txt"), b"new")?;
    let _sources = FsQuirks::install(&from, Quirks::default());
    let _stick = FsQuirks::install(&into, Quirks::vfat());

    let clashes = merge::clashes(&from.join("photos"), &into)?;
    assert_eq!(clashes.len(), 1);
    assert_eq!(clashes[0].path, std::path::Path::new("2024/a:b.txt"));

    // Skipped by the path the clash was listed under: the one there stays, the rest comes in.
    let plan = MergePlan {
        choices: [(std::path::PathBuf::from("2024/a:b.txt"), Clash::Skip)].into(),
        otherwise: Clash::KeepBoth,
    };
    let merged = merge::merge(
        &from.join("photos"),
        &into,
        &plan,
        crate::ops::NameMode::Fix,
        &crate::ops::Cancel::default(),
        &crate::ops::Progress::default(),
    )?;
    assert!(merged.failed.is_empty(), "{:?}", merged.failed);
    assert_eq!(fs::read(into.join("photos/2024/a_b.txt"))?, b"existing");
    assert_eq!(fs::read(into.join("photos/new.txt"))?, b"new");
    Ok(())
}
