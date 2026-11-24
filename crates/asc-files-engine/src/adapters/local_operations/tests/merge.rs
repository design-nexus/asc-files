// SPDX-License-Identifier: MIT

use std::{os::unix::fs::MetadataExt, sync::atomic::Ordering};

use rustix::io::Errno;

use super::super::{fs_ops::System, placement::probe};
use super::fs_quirks::{Op, OverEmptyDirectory, Racer, Support, Trash, hold};
use super::*;
use crate::{
    adapters::volume::learn_mount_capability,
    services::{KeptCause, KeptItem, PlacedSnapshot, ReplacedOriginal},
};

fn no_plan(_: MergeWritten) {}

fn recorded_plans() -> (Rc<RefCell<Vec<MergeWritten>>>, impl Fn(MergeWritten)) {
    let plans = Rc::new(RefCell::new(Vec::new()));
    let recorded = plans.clone();
    (plans, move |plan| recorded.borrow_mut().push(plan))
}

#[test]
fn a_new_subdirectory_is_reported_as_a_single_created_entry() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(source.join("sub/deep"))?;
    fs::create_dir_all(&target)?;
    fs::write(source.join("sub/deep/incoming.txt"), b"new")?;
    let _scope = FsQuirks::install(root.path(), without_trash(Quirks::default()));
    let (plans, on_merged) = recorded_plans();

    let result = merge_local(&source, &target, false, Cancellable::new(), &on_merged);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(fs::read(target.join("sub/deep/incoming.txt"))?, b"new");
    let plans = plans.borrow();
    // Legacy reported a second plan of placed snapshots for undo without a Trash; the undo
    // journal records identities instead.
    assert_eq!(
        plans[0].created,
        vec![Location::local(target.join("sub"))],
        "undo removes the topmost created directory, not each nested child"
    );
    assert!(plans[0].overwritten.is_empty());
    Ok(())
}

#[test]
fn a_merge_stage_goes_once_another_program_closes_a_file_it_kept_there()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&target)?;
    fs::write(source.join("new.txt"), b"new")?;
    let holds = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (held, live) = (holds.clone(), target.clone());
    let hold_staged: Racer = Box::new(move || {
        for stage in fs::read_dir(&live).expect("list the target").flatten() {
            if stage
                .file_name()
                .to_string_lossy()
                .starts_with(".asc-files-copy-")
            {
                let staged = hold(&stage.path().join("new.txt"));
                held.lock().expect("the holds").push(staged);
            }
        }
    });
    let release: Racer = Box::new(move || holds.lock().expect("the holds").clear());
    let _scope = FsQuirks::install(
        root.path(),
        without_trash(Quirks {
            before: vec![(Op::Unlink, "new.txt", 0, hold_staged)],
            after: vec![(Op::Names, ".asc-files-copy-*", 0, release)],
            links: Support::Rejected(Errno::MLINK),
            rename_over_empty_dir: OverEmptyDirectory::NotEmpty,
            ..Quirks::nfs3()
        }),
    );
    let (_plans, on_merged) = recorded_plans();

    let result = merge_local(&source, &target, false, Cancellable::new(), &on_merged);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(names(&target)?, ["new.txt"]);
    assert_eq!(fs::read(target.join("new.txt"))?, b"new");
    Ok(())
}

#[test]
fn merge_places_a_read_only_incoming_folder() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(source.join("incoming"))?;
    fs::create_dir_all(&target)?;
    fs::write(source.join("incoming/file.txt"), b"new")?;
    fs::write(target.join("stays.txt"), b"keep me")?;
    let incoming = source.join("incoming");
    fs::set_permissions(&incoming, fs::Permissions::from_mode(0o555))?;

    let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

    let placed = fs::metadata(target.join("incoming")).map(|metadata| metadata.mode() & 0o7777);
    for folder in [&incoming, &target.join("incoming")] {
        if folder.is_dir() {
            fs::set_permissions(folder, fs::Permissions::from_mode(0o755))?;
        }
    }
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(placed?, 0o555);
    assert_eq!(fs::read(target.join("incoming/file.txt"))?, b"new");
    assert_eq!(fs::read(target.join("stays.txt"))?, b"keep me");
    Ok(())
}

fn visible_names(folder: &Path) -> io::Result<Vec<String>> {
    Ok(names(folder)?
        .into_iter()
        .filter(|name| !name.starts_with(".asc-files-"))
        .collect())
}

#[test]
fn a_merge_keeps_hard_links_only_within_one_merge_stage() -> Result<(), Box<dyn Error>> {
    use super::fs_quirks::Racer;
    use std::os::unix::fs::MetadataExt;
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(source.join("new"))?;
    fs::create_dir_all(source.join("sub"))?;
    fs::create_dir_all(target.join("sub"))?;
    fs::write(source.join("a"), b"linked")?;
    for name in ["a2", "new/c", "sub/b"] {
        fs::hard_link(source.join("a"), source.join(name))?;
    }
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (live, seen_at_link) = (target.clone(), seen.clone());
    let racer: Racer = Box::new(move || {
        let names = visible_names(&live).expect("list the merged folder");
        seen_at_link.lock().expect("record").extend(names);
    });
    let scope = FsQuirks::install(
        &target,
        Quirks {
            before: vec![(Op::Link, "*", 0, racer)],
            ..Quirks::default()
        },
    );
    learn_mount_capability(scope.key(), |capabilities| {
        capabilities.hard_links = Some(true);
    });

    let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        *seen.lock().map_err(|error| error.to_string())?,
        ["sub"],
        "names are linked before their stage is placed"
    );
    let [a, a2, c, b] = ["a", "a2", "new/c", "sub/b"].map(|name| {
        fs::metadata(target.join(name)).map(|metadata| (metadata.ino(), metadata.nlink()))
    });
    let (a, b) = (a?, b?);
    assert_eq!((a2?, c?), (a, a));
    assert_eq!(a.1, 3);
    assert_ne!(b.0, a.0);
    assert_eq!(b.1, 1);
    assert_eq!(fs::read(target.join("sub/b"))?, b"linked");
    Ok(())
}

#[test]
fn a_merged_copy_replaced_before_linking_is_not_linked() -> Result<(), Box<dyn Error>> {
    use super::fs_quirks::Racer;
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source)?;
    fs::create_dir(&target)?;
    fs::write(source.join("a"), b"linked")?;
    fs::hard_link(source.join("a"), source.join("a2"))?;
    let live = target.clone();
    let racer: Racer = Box::new(move || {
        let stage = names(&live)
            .expect("list the merged folder")
            .into_iter()
            .find(|name| name.starts_with(".asc-files-copy-"))
            .map(|name| live.join(name));
        for copied in stage
            .iter()
            .flat_map(|stage| [stage.join("a"), stage.join("a2")])
        {
            if copied.exists() {
                let other = live.join("other");
                fs::write(&other, b"other").expect("write the other item");
                fs::rename(&other, &copied).expect("replace the copy");
            }
        }
    });
    let scope = FsQuirks::install(
        &target,
        Quirks {
            before: vec![(Op::Link, "*", 0, racer)],
            ..Quirks::default()
        },
    );
    learn_mount_capability(scope.key(), |capabilities| {
        capabilities.hard_links = Some(true);
    });

    let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

    assert!(result.is_err(), "{result:?}");
    assert_eq!(names(&target)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn merge_never_overwrites_an_entry_created_during_the_merge() -> Result<(), Box<dyn Error>> {
    use super::fs_quirks::Racer;
    use crate::services::RenameCause;
    let _serial = serial()?;
    type Renamed = &'static [(&'static str, &'static str, RenameCause)];
    let rows: [(&str, &str, Preset, Renamed); 2] = [
        (
            "notes.txt",
            "notes (1).txt",
            Quirks::default,
            &[("notes.txt", "notes (1).txt", RenameCause::Taken)],
        ),
        (
            "photos",
            "photos (1)",
            Quirks::vfat,
            &[
                ("photos", "photos (1)", RenameCause::Taken),
                (
                    "photos (1)/makefile",
                    "photos (1)/makefile (1)",
                    RenameCause::Alias,
                ),
            ],
        ),
    ];
    for (taken, saved_as, preset, reported) in rows {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir_all(source.join("photos"))?;
        fs::create_dir(&target)?;
        fs::write(source.join("notes.txt"), b"incoming")?;
        for name in ["Makefile", "makefile"] {
            fs::write(source.join("photos").join(name), name)?;
        }
        let live = target.clone();
        let racer: Racer = Box::new(move || {
            fs::write(live.join(taken), b"racer").expect("take the name");
        });
        let _scope = FsQuirks::install(
            &target,
            Quirks {
                before: vec![(Op::Rename, taken, 0, racer)],
                ..preset()
            },
        );

        let (merged, report) = merge_reporting(&source, &target, None);

        assert!(merged.is_ok(), "{taken}: {merged:?}");
        assert_eq!(fs::read(target.join(taken))?, b"racer", "{taken}");
        let mut expected = vec!["notes.txt", "photos", saved_as];
        expected.sort_unstable();
        assert_eq!(names(&target)?, expected, "{taken}");
        let ours = target.join(saved_as);
        if taken == "photos" {
            assert_eq!(fs::read(ours.join("Makefile"))?, b"Makefile");
            assert_eq!(fs::read(ours.join("makefile (1)"))?, b"makefile");
        } else {
            assert_eq!(fs::read(ours)?, b"incoming");
        }
        let reported = reported
            .iter()
            .map(|(requested, saved_as, cause)| {
                (requested.to_string(), saved_as.to_string(), *cause)
            })
            .collect::<Vec<_>>();
        assert_eq!(renamed_beneath(&target, &report), reported, "{taken}");
    }
    Ok(())
}

#[test]
fn a_stopped_merge_reports_only_what_it_placed() -> Result<(), Box<dyn Error>> {
    use super::fs_quirks::Racer;
    use crate::services::RenameCause;
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(source.join("z"))?;
    fs::create_dir(&target)?;
    fs::write(source.join("a.txt"), b"incoming")?;
    for name in ["Makefile", "makefile"] {
        fs::write(source.join("z").join(name), name)?;
    }
    let live = target.clone();
    let racer: Racer = Box::new(move || {
        fs::write(live.join("a.txt"), b"racer").expect("take the name");
    });
    let _scope = FsQuirks::install(
        &target,
        Quirks {
            before: vec![(Op::Rename, "a.txt", 0, racer)],
            fail: vec![(Op::Rename, "z", 0, Errno::IO)],
            ..Quirks::vfat()
        },
    );

    let (merged, report) = merge_reporting(&source, &target, None);

    assert!(merged.is_err(), "{merged:?}");
    assert_eq!(names(&target)?, ["a (1).txt", "a.txt"]);
    assert_eq!(fs::read(target.join("a (1).txt"))?, b"incoming");
    assert_eq!(
        renamed_beneath(&target, &report),
        [("a.txt".into(), "a (1).txt".into(), RenameCause::Taken)]
    );
    Ok(())
}

#[test]
fn an_unconfirmed_placement_keeps_its_stage() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source)?;
    fs::create_dir(&target)?;
    fs::write(source.join("leaf"), b"incoming")?;
    // The placing rename's reply is lost, and the leaf is found at neither
    // name: it went back into the stage under another one.
    let live = target.clone();
    let racer: super::fs_quirks::Racer = Box::new(move || {
        let stage = names(&live)
            .expect("live folder")
            .into_iter()
            .find(|name| name.starts_with(".asc-files-copy-"))
            .expect("stage");
        fs::rename(live.join("leaf"), live.join(stage).join("moved")).expect("move the leaf");
    });
    let _scope = FsQuirks::install(
        &target,
        Quirks {
            lost_reply: vec![(Op::Rename, "leaf", Errno::IO)],
            after: vec![(Op::Rename, "leaf", 0, racer)],
            ..Quirks::default()
        },
    );

    let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

    assert!(result.is_err(), "{result:?}");
    let names = names(&target)?;
    let [stage] = names.as_slice() else {
        panic!("only the stage that holds the leaf stays: {names:?}");
    };
    assert_eq!(fs::read(target.join(stage).join("moved"))?, b"incoming");
    Ok(())
}

#[test]
fn a_merge_copies_folders_in_place_where_the_mount_cannot_place_them() -> Result<(), Box<dyn Error>>
{
    use crate::services::RenameCause;
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(source.join("new"))?;
    fs::create_dir(&target)?;
    for name in ["Makefile", "makefile"] {
        fs::write(source.join("new").join(name), name)?;
    }
    fs::write(target.join("new"), b"taken")?;
    let scope = FsQuirks::install(
        &target,
        Quirks {
            mount_type: Some(("ext4", "rw")),
            case_insensitive: true,
            ..Quirks::without_folder_placement()
        },
    );
    learn_mount_capability(scope.key(), |capabilities| {
        capabilities.no_replace = Some(false);
        capabilities.hard_links = Some(true);
        capabilities.rename_over_empty_directory = Some(false);
    });

    // Without claims, one worker makes the folder's files last first, so
    // `Makefile` is the alias.
    let (merged, report) = merge_reporting(&source, &target, Some(1));

    assert!(merged.is_ok(), "{merged:?}");
    assert_eq!(names(&target)?, ["new", "new (1)"]);
    assert_eq!(fs::read(target.join("new"))?, b"taken");
    assert_eq!(
        names(&target.join("new (1)"))?,
        ["Makefile (1)", "makefile"]
    );
    assert_eq!(fs::read(target.join("new (1)/Makefile (1)"))?, b"Makefile");
    assert!(!scope.used_name(".asc-files-*"), "{:?}", scope.calls());
    assert_eq!(
        renamed_beneath(&target, &report),
        [
            ("new".into(), "new (1)".into(), RenameCause::Taken),
            (
                "new (1)/Makefile".into(),
                "new (1)/Makefile (1)".into(),
                RenameCause::Alias
            ),
        ]
    );
    Ok(())
}

#[test]
fn a_merged_item_is_invisible_until_complete() -> Result<(), Box<dyn Error>> {
    use super::fs_quirks::Racer;
    let _serial = serial()?;
    // Placed by rename, by the rungs that cannot replace, and copied out of
    // the stage where nothing can place it.
    let presets: [Preset; 3] = [Quirks::default, Quirks::vboxsf, Quirks::without_placement];
    for (row, preset) in presets.into_iter().enumerate() {
        let root = tempfile::tempdir()?;
        let sources = root.path().join("sources");
        let destination = root.path().join("destination");
        let source = sources.join("folder");
        let target = destination.join("folder");
        fs::create_dir_all(source.join("new"))?;
        fs::create_dir_all(&target)?;
        fs::write(source.join("incoming.bin"), vec![0x5a; 256 * 1024])?;
        fs::write(source.join("new/inner.bin"), vec![0xa5; 256 * 1024])?;
        fs::write(target.join("stays.txt"), b"keep me")?;
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (live, seen_at_write) = (target.clone(), seen.clone());
        let racer: Racer = Box::new(move || {
            let names = visible_names(&live).expect("list the merged folder");
            seen_at_write.lock().expect("record").extend(names);
        });
        // Separate mounts, so the data is written rather than cloned.
        let _sources = FsQuirks::install(&sources, Quirks::default());
        let _destination = FsQuirks::install(
            &destination,
            Quirks {
                before: vec![(Op::Write, "*", 0, racer)],
                ..preset()
            },
        );

        let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

        assert!(result.is_ok(), "row {row}: {result:?}");
        assert_eq!(
            *seen.lock().map_err(|error| error.to_string())?,
            ["stays.txt"],
            "row {row}"
        );
        assert_eq!(
            names(&target)?,
            ["incoming.bin", "new", "stays.txt"],
            "row {row}"
        );
        assert_eq!(
            fs::read(target.join("incoming.bin"))?,
            vec![0x5a; 256 * 1024]
        );
        assert_eq!(
            fs::read(target.join("new/inner.bin"))?,
            vec![0xa5; 256 * 1024]
        );
        assert_eq!(fs::read(target.join("stays.txt"))?, b"keep me");
    }
    Ok(())
}

#[test]
fn merge_does_not_follow_a_target_folder_swapped_for_a_link() -> Result<(), Box<dyn Error>> {
    use super::fs_quirks::Racer;
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    let outside = root.path().join("outside");
    fs::create_dir_all(source.join("sub"))?;
    fs::create_dir_all(target.join("sub"))?;
    fs::create_dir(&outside)?;
    fs::write(source.join("sub/new.txt"), b"new")?;
    fs::write(target.join("sub/old.txt"), b"old")?;
    let (live, elsewhere) = (target.clone(), outside.clone());
    let racer: Racer = Box::new(move || {
        fs::rename(live.join("sub"), live.join("moved")).expect("move the folder away");
        std::os::unix::fs::symlink(&elsewhere, live.join("sub")).expect("link elsewhere");
    });
    let _scope = FsQuirks::install(
        &target,
        Quirks {
            before: vec![(Op::OpenDir, "sub", 0, racer)],
            ..Quirks::default()
        },
    );

    let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

    assert!(result.is_err(), "{result:?}");
    assert_eq!(names(&outside)?, Vec::<String>::new());
    assert_eq!(names(&target.join("moved"))?, ["old.txt"]);
    Ok(())
}

#[test]
fn a_merge_move_removes_the_emptied_source() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&target)?;
    fs::write(source.join("incoming.txt"), b"new")?;
    fs::write(target.join("stays.txt"), b"keep me")?;

    let result = merge_local(&source, &target, true, Cancellable::new(), &no_plan);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(fs::read(target.join("incoming.txt"))?, b"new");
    assert_eq!(fs::read(target.join("stays.txt"))?, b"keep me");
    assert!(!source.exists());
    Ok(())
}

#[test]
fn merging_a_file_is_rejected_without_touching_either_side() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.txt");
    let target = root.path().join("target.txt");
    fs::write(&source, b"new")?;
    fs::write(&target, b"old")?;

    let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

    assert!(result.is_err());
    assert_eq!(fs::read(&source)?, b"new");
    assert_eq!(fs::read(&target)?, b"old");
    Ok(())
}

#[test]
fn merging_onto_a_file_is_rejected_without_touching_either_side() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source)?;
    fs::write(source.join("incoming.txt"), b"new")?;
    fs::write(&target, b"old")?;

    let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

    assert!(result.is_err());
    assert!(source.join("incoming.txt").exists());
    assert_eq!(fs::read(&target)?, b"old");
    Ok(())
}

fn without_trash(quirks: Quirks) -> Quirks {
    Quirks {
        trash: Trash::Unsupported,
        ..quirks
    }
}

fn aside_folder(target: &Path) -> Result<Option<String>, io::Error> {
    Ok(names(target)?
        .into_iter()
        .find(|name| name.starts_with(".asc-files-merge-")))
}

fn write_files(root: &Path, files: &[(&str, &str)]) -> Result<(), io::Error> {
    for (path, contents) in files {
        let path = root.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }
    Ok(())
}

/// The item in the Trash that came from `original`, the newest of them.
fn in_trash(original: &Path) -> Option<PathBuf> {
    crate::trash::list()
        .into_iter()
        .filter(|entry| entry.original == original)
        .max_by_key(|entry| entry.deleted.clone())
        .map(|entry| entry.file)
}

#[test]
fn merging_folders_unites_contents_and_overwrites_shared_names() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&target)?;
    fs::write(source.join("incoming.txt"), b"new")?;
    fs::write(source.join("shared.txt"), b"incoming wins")?;
    fs::write(target.join("stays.txt"), b"keep me")?;
    fs::write(target.join("shared.txt"), b"old")?;
    let (plans, on_merged) = recorded_plans();

    let result = merge_local(&source, &target, false, Cancellable::new(), &on_merged);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(fs::read(target.join("incoming.txt"))?, b"new");
    assert_eq!(fs::read(target.join("shared.txt"))?, b"incoming wins");
    assert_eq!(fs::read(target.join("stays.txt"))?, b"keep me");
    assert!(source.exists(), "a copy merge keeps the source");
    // Legacy staged the original through a hook; the engine sends it to the Trash.
    let original = in_trash(&target.join("shared.txt")).ok_or("the original in the Trash")?;
    assert_eq!(fs::read(original)?, b"old");
    // Legacy also reported placed snapshots for its own undo; the engine files journal steps.
    let plans = plans.borrow();
    assert_eq!(plans.len(), 1);
    assert_eq!(
        plans[0].created,
        vec![Location::local(target.join("incoming.txt"))]
    );
    assert_eq!(
        plans[0].overwritten,
        vec![Location::local(target.join("shared.txt"))]
    );
    Ok(())
}

#[test]
fn merging_recurses_into_subdirectories_shared_by_both_sides() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(source.join("nested"))?;
    fs::create_dir_all(target.join("nested"))?;
    fs::write(source.join("nested/incoming.txt"), b"new")?;
    fs::write(source.join("nested/shared.txt"), b"incoming wins")?;
    fs::write(target.join("nested/stays.txt"), b"keep me")?;
    fs::write(target.join("nested/shared.txt"), b"old")?;
    let (plans, on_merged) = recorded_plans();
    let listed_on = Arc::new(std::sync::Mutex::new(Vec::new()));
    // Legacy listed in chunks; the engine's copy lists a folder whole. Either goes through the seam.
    let listing = |op: Op, folder: &'static str| -> (Op, &'static str, usize, Racer) {
        let listed_on = listed_on.clone();
        let racer: Racer = Box::new(move || {
            listed_on
                .lock()
                .expect("listings")
                .push((folder, thread::current().id()));
        });
        (op, folder, 0, racer)
    };
    let _scope = FsQuirks::install(
        root.path(),
        Quirks {
            before: [Op::List, Op::ListChunks]
                .into_iter()
                .flat_map(|op| [listing(op, "source"), listing(op, "nested")])
                .collect(),
            ..Quirks::default()
        },
    );

    let result = merge_local(&source, &target, false, Cancellable::new(), &on_merged);

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(fs::read(target.join("nested/incoming.txt"))?, b"new");
    assert_eq!(
        fs::read(target.join("nested/shared.txt"))?,
        b"incoming wins"
    );
    assert_eq!(fs::read(target.join("nested/stays.txt"))?, b"keep me");
    let listed_on = listed_on.lock().expect("listings");
    let folders: Vec<_> = listed_on.iter().map(|(folder, _)| *folder).collect();
    assert!(
        folders.contains(&"source") && folders.contains(&"nested"),
        "each local source folder is listed through the filesystem seam: {folders:?}"
    );
    let plans = plans.borrow();
    assert_eq!(
        plans[0].created,
        vec![Location::local(target.join("nested/incoming.txt"))]
    );
    assert_eq!(
        plans[0].overwritten,
        vec![Location::local(target.join("nested/shared.txt"))]
    );
    Ok(())
}

#[test]
fn a_merge_replaces_nothing_with_a_link_the_destination_cannot_hold() -> Result<(), Box<dyn Error>>
{
    let _serial = serial()?;
    // The FAT row runs without a Trash: a simulated drive's Trash would be the home one, which
    // a real FAT drive's is not, and legacy staged the original through a hook here.
    for (row, quirks, links) in [
        ("vfat", (|| without_trash(Quirks::vfat())) as Preset, false),
        ("ext4", Quirks::ext4, true),
    ] {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir_all(&source)?;
        fs::create_dir_all(&target)?;
        fs::write(source.join("a.txt"), b"a")?;
        std::os::unix::fs::symlink("a.txt", source.join("link"))?;
        fs::write(target.join("link"), b"earlier")?;
        let _target = FsQuirks::install(&target, quirks());

        merge_local(&source, &target, false, Cancellable::new(), &no_plan)?;

        assert_eq!(fs::read(target.join("a.txt"))?, b"a", "{row}");
        let trashed = in_trash(&target.join("link"));
        if links {
            assert!(trashed.is_some(), "{row}");
            assert_eq!(fs::read_link(target.join("link"))?, Path::new("a.txt"));
        } else {
            assert_eq!(trashed, None, "{row}");
            assert_eq!(fs::read(target.join("link"))?, b"earlier");
        }
    }
    Ok(())
}

#[test]
fn an_original_that_cannot_go_back_stays_aside_and_is_named() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&target)?;
    std::os::unix::fs::symlink("elsewhere", source.join("link"))?;
    fs::write(target.join("link"), b"earlier")?;
    // FAT holds no link, so the original set aside for it must go back, and that fails.
    let _target = FsQuirks::install(
        &target,
        Quirks {
            fail: vec![(Op::Rename, "link", 1, Errno::IO)],
            ..without_trash(Quirks::vfat())
        },
    );

    let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

    let message = result
        .expect_err("the original could not go back")
        .message()
        .to_owned();
    let aside = aside_folder(&target)?.ok_or("the aside folder stays")?;
    assert_eq!(fs::read(target.join(&aside).join("link"))?, b"earlier");
    assert!(
        message.contains(aside.as_str()),
        "the error says where: {message}"
    );
    Ok(())
}

/// Legacy's merge through its Trash-less staging: the engine's merge, and what it wrote.
fn merge_through_trash(
    source: &Path,
    target: &Path,
    move_source: bool,
) -> (Result<(), EngineError>, Vec<MergeWritten>) {
    let (plans, on_merged) = recorded_plans();
    let result = merge_local(source, target, move_source, Cancellable::new(), &on_merged);
    (result, plans.take())
}

#[test]
fn a_failed_aside_puts_every_original_back_before_merging() -> Result<(), Box<dyn Error>> {
    use super::super::{fs_ops::System, placement::probe};
    use crate::adapters::volume::learn_mount_capability;
    let _serial = serial()?;
    let target_files = [
        ("a.txt", "old a"),
        ("b.txt", "old b"),
        ("mount/c.txt", "old c"),
    ];
    for case in [
        "no safe placement",
        "second aside fails",
        "nested mount",
        "strict open crosses mount",
    ] {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let target = root.path().join("target");
        write_files(
            &source,
            &[
                ("a.txt", "new a"),
                ("b.txt", "new b"),
                ("mount/c.txt", "new c"),
            ],
        )?;
        write_files(&target, &target_files)?;
        let quirks = match case {
            "no safe placement" => Quirks::without_placement(),
            "second aside fails" => Quirks {
                fail: vec![(Op::Rename, "*", 1, Errno::IO)],
                ..Quirks::default()
            },
            "strict open crosses mount" => Quirks {
                fail: vec![(Op::OpenAt, ".asc-files-merge-*", 0, Errno::XDEV)],
                ..Quirks::default()
            },
            _ => Quirks::default(),
        };
        let quirks = FsQuirks::install(root.path(), without_trash(quirks));
        if case == "second aside fails" {
            let folder = fs::File::open(root.path())?;
            let no_replace = probe(&System, folder.as_fd(), true, false, false)?.no_replace;
            learn_mount_capability(quirks.key(), |capabilities| {
                capabilities.no_replace = no_replace;
            });
        }
        let _nested = (case == "nested mount")
            .then(|| FsQuirks::install(&target.join("mount"), without_trash(Quirks::default())));

        let (result, plans) = merge_through_trash(&source, &target, false);

        let error = result.expect_err(case);
        assert!(
            error.message().contains("out of the way"),
            "{case}: {error}"
        );
        assert_eq!(
            quirks.used_name(".asc-files-merge-*"),
            case != "no safe placement",
            "{case}"
        );
        // Nothing was copied, and every original is back at its name.
        for (path, contents) in target_files {
            assert_eq!(fs::read(target.join(path))?, contents.as_bytes(), "{case}");
        }
        assert_eq!(names(&target)?, ["a.txt", "b.txt", "mount"], "{case}");
        assert!(plans.iter().all(|plan| plan.overwritten.is_empty()));
    }
    Ok(())
}

#[test]
fn merge_without_trash_sets_originals_aside_and_removes_them_after_success()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    // Legacy also ran a GIO Trash that reported failure without a reason, which it took for no
    // Trash; the native Trash's failure is an I/O error, which stops the merge.
    for (trash, quirks, move_source) in [
        (Trash::Unsupported, Quirks::default(), false),
        (Trash::Unsupported, Quirks::without_rename_flags(), true),
        (Trash::Unsupported, Quirks::sshfs(), false),
        (Trash::Unsupported, Quirks::vboxsf(), true),
        (Trash::Unsupported, Quirks::exfat(), false),
        (Trash::Unsupported, Quirks::exfat(), true),
    ] {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let target = root.path().join("target");
        write_files(
            &source,
            &[
                ("shared.txt", "new"),
                ("nested/deep/shared.txt", "new deep"),
                ("incoming.txt", "incoming"),
            ],
        )?;
        write_files(
            &target,
            &[
                ("shared.txt", "old"),
                ("nested/deep/shared.txt", "old deep"),
                ("nested/stays.txt", "keep me"),
            ],
        )?;
        let quirks = FsQuirks::install(root.path(), Quirks { trash, ..quirks });

        let (result, plans) = merge_through_trash(&source, &target, move_source);

        assert!(result.is_ok(), "{trash:?}: {result:?}");
        // A merging move within one drive replaces item by item, each original set aside by
        // the replace; legacy set every one aside in the merge's folder.
        assert!(
            quirks.used_name(".asc-files-merge-*") || quirks.used_name(".asc-files-originals-*"),
            "{trash:?}"
        );
        assert_eq!(fs::read(target.join("shared.txt"))?, b"new");
        assert_eq!(
            fs::read(target.join("nested/deep/shared.txt"))?,
            b"new deep"
        );
        assert_eq!(fs::read(target.join("nested/stays.txt"))?, b"keep me");
        assert_eq!(names(&target)?, ["incoming.txt", "nested", "shared.txt"]);
        assert_eq!(source.exists(), !move_source);
        assert_eq!(
            plans[0].created,
            vec![Location::local(target.join("incoming.txt"))],
            "{trash:?}: only what undo can take away is filed"
        );
        assert!(
            plans[0].overwritten.is_empty(),
            "originals deleted without Trash cannot be restored by undo"
        );
    }
    Ok(())
}

#[test]
fn without_trash_an_original_the_merge_left_nothing_in_place_of_stays() -> Result<(), Box<dyn Error>>
{
    use super::fs_quirks::Racer;
    let _serial = serial()?;
    let rewrite = |path: PathBuf, contents: &'static [u8]| -> Racer {
        Box::new(move || fs::write(&path, contents).expect("rewrite the source"))
    };
    for row in [
        "left out",
        "kept at its source",
        "put back fails",
        "name taken meanwhile",
    ] {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir(&source)?;
        write_files(&target, &[("x", "original"), ("y", "kept")])?;
        let moving = row == "kept at its source";
        let mut source_quirks = Quirks::default();
        let _socket = if moving {
            fs::write(source.join("x"), b"incoming")?;
            source_quirks.before = vec![
                (Op::Read, "x", 0, rewrite(source.join("x"), b"second")),
                (Op::Read, "x", 1, rewrite(source.join("x"), b"third")),
            ];
            None
        } else {
            Some(std::os::unix::net::UnixListener::bind(source.join("x"))?)
        };
        let mut target_quirks = without_trash(Quirks::default());
        if row == "put back fails" {
            target_quirks.fail = vec![(Op::Rename, "x", 1, Errno::IO)];
        }
        if row == "name taken meanwhile" {
            target_quirks.before = vec![(Op::Rename, "x", 1, rewrite(target.join("x"), b"theirs"))];
        }
        let _source = FsQuirks::install(&source, source_quirks);
        let _target = FsQuirks::install(&target, target_quirks);

        let (result, _) = merge_through_trash(&source, &target, moving);

        if matches!(row, "put back fails" | "name taken meanwhile") {
            let aside = aside_folder(&target)?.ok_or("the original stays aside")?;
            assert!(result.is_err(), "{row}");
            assert_eq!(
                fs::read(target.join(aside).join("x"))?,
                b"original",
                "{row}"
            );
            if row == "name taken meanwhile" {
                assert_eq!(fs::read(target.join("x"))?, b"theirs", "{row}");
            }
            continue;
        }
        assert!(result.is_ok(), "{row}: {result:?}");
        assert_eq!(names(&target)?, ["x", "y"], "{row}");
        assert_eq!(fs::read(target.join("x"))?, b"original", "{row}");
        if moving {
            assert_eq!(fs::read(source.join("x"))?, b"third", "{row}");
        }
    }
    Ok(())
}

#[test]
fn a_merge_without_trash_reports_its_folder_kept_by_an_original_still_open()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let editor = Some("Text Editor");
    let without_links = || Quirks {
        links: Support::Rejected(Errno::PERM),
        ..Quirks::nfs3()
    };
    let renumbered_links = || Quirks {
        link_inodes: true,
        ..Quirks::nfs3()
    };
    for (row, move_sources, original, preset, holder) in [
        ("copy fails", false, "x", Quirks::nfs3 as Preset, editor),
        ("copy fails", false, "sub/x", Quirks::nfs3, editor),
        ("copy fails, no links", false, "x", without_links, editor),
        (
            "copy fails, links numbered apart",
            false,
            "x",
            renumbered_links,
            editor,
        ),
        ("left out", false, "x", Quirks::nfs3, editor),
        ("left out", true, "sub/x", Quirks::nfs3, editor),
        ("replaced", true, "x", Quirks::nfs3, editor),
        ("replaced", true, "x", Quirks::cifs_posix, editor),
        ("replaced", true, "x", Quirks::cifs_posix, None),
        ("replaced", true, "sub/x", Quirks::nfs3, None),
        ("replaced", true, "sub/x", Quirks::cifs_posix, None),
    ] {
        let holder = holder.map(|program| crate::services::Holder::Program(program.to_owned()));
        let root = tempfile::tempdir()?;
        let linked_back = matches!(preset().links, Support::Supported)
            && !preset().link_inodes
            && base_links_keep_their_number(root.path())?;
        let put_back_by_rename = !row.starts_with("replaced") && !linked_back;
        let sources = root.path().join("sources");
        let destination = root.path().join("destination");
        let (source, target) = (sources.join("run"), destination.join("run"));
        write_files(&source, &[("y.txt", "new y")])?;
        write_files(&target, &[(original, "original")])?;
        if row == "replaced" {
            write_files(&source, &[("also/z", "incoming")])?;
            write_files(&target, &[("also/z", "original")])?;
        }
        fs::create_dir_all(source.join(original).parent().ok_or("no parent")?)?;
        let mut source_quirks = Quirks::default();
        let _socket = if row == "left out" {
            match std::os::unix::net::UnixListener::bind(source.join(original)) {
                Ok(socket) => Some(socket),
                Err(error)
                    if matches!(Errno::from_io_error(&error), Some(Errno::PERM | Errno::IO)) =>
                {
                    eprintln!(
                        "skipped on a base that refuses Unix sockets: {row} {original}, moving: {move_sources}"
                    );
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            fs::write(source.join(original), b"incoming")?;
            None
        };
        if row.starts_with("copy fails") {
            source_quirks.fail = vec![(Op::OpenSource, "x", 0, Errno::IO)];
        }
        let _sources = FsQuirks::install(&sources, source_quirks);
        let _destination = FsQuirks::install(
            &destination,
            without_trash(Quirks {
                holder: holder.clone(),
                ..preset()
            }),
        );
        let _clock = ClockGuard::set(std::time::SystemTime::now());
        let _held = hold(&target.join(original));
        let events = Rc::new(RefCell::new(Vec::new()));
        let emitted = events.clone();
        let _operation = LocalOperationProvider.paste(
            PasteRequest {
                id: OperationRequestId(1729),
                destination: Location::local(&destination),
                items: vec![PasteItem {
                    source: Location::local(&source),
                    conflict: TransferConflict::Merge,
                    target_name: None,
                }],
                move_sources,
            },
            Rc::new(move |event| emitted.borrow_mut().push(event)),
        );

        drive_until_transfer_settles(&events);

        let listed = !matches!(
            preset().silly_rename,
            Some(super::fs_quirks::Silly {
                aside: super::fs_quirks::Aside::Unlisted(_),
                ..
            })
        );
        let row = format!(
            "{row} {original}, moving: {move_sources}, {:?}",
            preset().mount_type
        );
        let events = events.take();
        let folder = aside_folder(&target)?;
        assert_eq!(folder.is_none(), put_back_by_rename, "{row}: {events:?}");
        let kept = events
            .iter()
            .filter_map(|event| match event {
                OperationEvent::ItemsReported { report, .. } => Some(report.kept.clone()),
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        let cause = match holder {
            None if !listed => KeptCause::Undeletable(Errno::ACCESS.raw_os_error()),
            holder => KeptCause::StillOpen(holder),
        };
        let mut expected: Vec<_> = folder
            .as_ref()
            .map(|folder| KeptItem {
                location: Location::local(target.join(folder)),
                cause,
            })
            .into_iter()
            .collect();
        if row.starts_with("left out") && move_sources {
            expected.push(KeptItem {
                location: Location::local(source.join(original)),
                cause: KeptCause::Skipped(crate::services::SpecialKind::Socket),
            });
        }
        // In any order: the engine reports what it left out as it goes.
        let (mut kept, mut expected) = (kept, expected);
        for list in [&mut kept, &mut expected] {
            list.sort_by_key(|item| format!("{item:?}"));
        }
        assert_eq!(kept, expected, "{row}: {events:?}");
        let now_there: &[u8] = if row.starts_with("replaced") {
            b"incoming"
        } else {
            b"original"
        };
        assert_eq!(fs::read(target.join(original))?, now_there, "{row}");
        if let Some(folder) = &folder {
            let aside = target.join(folder).join(original);
            let leftovers = names(aside.parent().ok_or("no parent")?)?;
            assert!(
                leftovers.len() == usize::from(listed)
                    && leftovers.iter().all(|name| name.starts_with(".nfs")),
                "{row}: {leftovers:?}"
            );
        }
        match terminal_transfer(&events) {
            Some(OperationEvent::TransferFailed { message, .. }) if row.starts_with("copy") => {
                assert!(
                    folder.iter().all(|folder| !message.contains(folder)),
                    "{row}: {message}"
                );
                assert_eq!(fs::read(source.join(original))?, b"incoming", "{row}");
            }
            Some(OperationEvent::Pasted { .. }) if !row.starts_with("copy") => {
                assert_eq!(fs::read(target.join("y.txt"))?, b"new y", "{row}");
                for name in ["y.txt", original] {
                    let gone = move_sources && (name == "y.txt" || row.starts_with("replaced"));
                    assert_eq!(source.join(name).exists(), !gone, "{row}: {name}");
                }
                assert_eq!(
                    item_moves(&events).len(),
                    usize::from(move_sources),
                    "{row}"
                );
            }
            other => panic!("{row}: {other:?}"),
        }
    }
    Ok(())
}

#[test]
fn a_failed_merge_without_trash_puts_back_what_it_can_and_names_the_rest()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    write_files(&source, &[("a.txt", "new a")])?;
    let _socket = std::os::unix::net::UnixListener::bind(source.join("b.txt"))?;
    write_files(
        &target,
        &[
            ("a.txt", "old a"),
            ("b.txt", "old b"),
            ("stays.txt", "keep me"),
        ],
    )?;
    // Legacy injected a copy that wrote part of a.txt and failed. Here a.txt is copied, b.txt is
    // a socket the copy leaves out, and the drive then cannot confirm the writes: b.txt's
    // original goes back to its free name, a.txt's waits where the error says.
    let _root = FsQuirks::install(
        root.path(),
        Quirks {
            fail: vec![(Op::SyncFs, "*", 0, Errno::IO)],
            ..without_trash(Quirks::default())
        },
    );

    let (result, plans) = merge_through_trash(&source, &target, false);

    let error = result.expect_err("the copy failed");
    let folder = aside_folder(&target)?.ok_or("the displaced original was not kept")?;
    assert!(error.message().contains(&folder), "{error}");
    assert_eq!(fs::read(target.join(&folder).join("a.txt"))?, b"old a");
    assert_eq!(names(&target.join(&folder))?, ["a.txt"]);
    assert_eq!(fs::read(target.join("a.txt"))?, b"new a");
    assert_eq!(fs::read(target.join("b.txt"))?, b"old b");
    assert_eq!(fs::read(target.join("stays.txt"))?, b"keep me");
    assert!(plans.iter().all(|plan| plan.overwritten.is_empty()));
    Ok(())
}

#[test]
fn merge_originals_are_deleted_only_once_the_merge_is_flushed() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    for flushed in [true, false] {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let target = root.path().join("target");
        write_files(&source, &[("shared.txt", "new")])?;
        write_files(&target, &[("shared.txt", "old")])?;
        let aside_during_flush = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (record, watched) = (aside_during_flush.clone(), target.clone());
        // The merge's aside folder, or within one drive the replace's originals folder.
        let set_aside = |target: &Path| -> io::Result<Option<String>> {
            Ok(names(target)?.into_iter().find(|name| {
                name.starts_with(".asc-files-merge-") || name.starts_with(".asc-files-originals-")
            }))
        };
        let racer: super::fs_quirks::Racer = Box::new(move || {
            let kept = set_aside(&watched).is_ok_and(|folder| folder.is_some());
            record
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(kept);
        });
        let _quirks = FsQuirks::install(
            root.path(),
            without_trash(Quirks {
                before: vec![(Op::SyncFs, "*", 0, racer)],
                fail: if flushed {
                    Vec::new()
                } else {
                    vec![(Op::SyncFs, "*", 0, Errno::IO)]
                },
                ..Quirks::default()
            }),
        );

        let (result, _) = merge_through_trash(&source, &target, true);

        assert_eq!(
            *aside_during_flush
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            [true],
            "one flush before the originals are deleted"
        );
        assert_eq!(fs::read(target.join("shared.txt"))?, b"new");
        // Legacy copied a merging move within one drive, so a failed flush kept the source too;
        // the engine renames there, and the moved item is the one at the name.
        assert!(!source.join("shared.txt").exists(), "flushed: {flushed}");
        if flushed {
            assert!(result.is_ok(), "{result:?}");
            assert_eq!(names(&target)?, ["shared.txt"]);
            continue;
        }
        let aside = set_aside(&target)?.ok_or("the originals stay aside")?;
        let kept = names(&target.join(&aside))?;
        let [original] = kept.as_slice() else {
            return Err(format!("one original aside: {kept:?}").into());
        };
        assert_eq!(fs::read(target.join(&aside).join(original))?, b"old");
        let error = result.expect_err("the originals are kept");
        assert!(error.message().contains(&aside), "{error}");
    }
    Ok(())
}

#[test]
fn a_failed_merge_preserves_the_destinations_existing_contents() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source)?;
    fs::create_dir_all(&target)?;
    fs::write(source.join("incoming.txt"), b"new")?;
    fs::write(target.join("stays.txt"), b"keep me")?;
    // Legacy injected a cancel from its copy hook; here a cancel lands as the copy opens the file.
    let cancellable = Cancellable::new();
    let cancel = cancellable.clone();
    let _quirks = FsQuirks::install(
        root.path(),
        Quirks {
            before: vec![(
                Op::OpenSource,
                "incoming.txt",
                0,
                Box::new(move || cancel.cancel()),
            )],
            ..Quirks::default()
        },
    );

    let result = merge_local(&source, &target, false, cancellable, &no_plan);

    assert!(result.is_err());
    assert_eq!(fs::read(target.join("stays.txt"))?, b"keep me");
    assert!(source.join("incoming.txt").exists());
    Ok(())
}

#[test]
fn a_staging_failure_aborts_the_merge_and_reports_only_what_was_staged()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    for errno in [Errno::IO, Errno::ROFS] {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir_all(&source)?;
        fs::create_dir_all(&target)?;
        fs::write(source.join("a.txt"), b"new a")?;
        fs::write(source.join("b.txt"), b"new b")?;
        fs::write(target.join("a.txt"), b"old a")?;
        fs::write(target.join("b.txt"), b"old b")?;
        // Legacy failed its staging hook for b.txt; here the Trash refuses it. The scope takes in
        // the (isolated) Trash too, so the drive and its Trash are one simulated mount and the
        // original of a.txt can go back.
        let data = PathBuf::from(std::env::var("XDG_DATA_HOME")?);
        let common = root
            .path()
            .ancestors()
            .find(|folder| data.starts_with(folder))
            .ok_or("a folder holding the test and its Trash")?;
        let _quirks = FsQuirks::install(
            common,
            Quirks {
                fail: vec![(Op::Trash, "b.txt", 0, errno)],
                ..Quirks::default()
            },
        );

        let result = merge_local(&source, &target, false, Cancellable::new(), &no_plan);

        let error = result.expect_err("staging failure aborts the merge");
        assert!(
            error
                .message()
                .starts_with("Could not move b.txt to Trash before merging: "),
            "{errno:?}: {error}"
        );
        assert_eq!(fs::read(source.join("a.txt"))?, b"new a");
        assert_eq!(fs::read(source.join("b.txt"))?, b"new b");
        assert_eq!(fs::read(target.join("b.txt"))?, b"old b", "{errno:?}");
        // Legacy left a.txt staged if it came first; the engine puts it back.
        assert_eq!(fs::read(target.join("a.txt"))?, b"old a", "{errno:?}");
        assert_eq!(in_trash(&target.join("a.txt")), None, "{errno:?}");
    }
    Ok(())
}
