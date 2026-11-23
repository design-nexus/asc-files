// SPDX-License-Identifier: MIT

use super::*;
use crate::services::ShownName;

#[test]
fn cutting_in_the_same_folder_remains_a_noop() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let destination = root.path().to_path_buf();
    let file = destination.join("document.txt");
    let directory = destination.join("folder");
    fs::write(&file, b"content")?;
    fs::create_dir_all(&directory)?;

    let events = RefCell::new(run_paste_events(PasteRequest {
        id: OperationRequestId(14),
        destination: Location::local(&destination),
        items: vec![
            PasteItem {
                source: Location::local(&file),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            },
            PasteItem {
                source: Location::local(&directory),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            },
        ],
        move_sources: true,
    }));

    assert!(matches!(
        events.borrow().last(),
        Some(OperationEvent::Pasted { .. })
    ));
    assert!(item_moves(&events.borrow()).is_empty());
    assert!(file.exists());
    assert!(directory.is_dir());
    assert!(!destination.join("document (1).txt").exists());
    assert!(!destination.join("folder (1)").exists());
    Ok(())
}

#[test]
fn keeping_both_in_a_cross_folder_paste_generates_a_unique_name() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source_dir = root.path().join("source");
    let source = source_dir.join("report.txt");
    let destination = root.path().join("dest");
    fs::create_dir_all(&source_dir)?;
    fs::create_dir_all(&destination)?;
    fs::write(&source, b"incoming")?;
    fs::write(destination.join("report.txt"), b"existing")?;

    let created = run_paste_collecting_created(PasteRequest {
        id: OperationRequestId(74),
        destination: Location::local(&destination),
        items: vec![PasteItem {
            source: Location::local(&source),
            conflict: TransferConflict::KeepBoth,
            target_name: None,
        }],
        move_sources: false,
    })?;

    assert_eq!(
        created.into_iter().flatten().collect::<Vec<_>>(),
        vec![Location::local(destination.join("report (1).txt"))]
    );
    assert_eq!(fs::read(destination.join("report.txt"))?, b"existing");
    assert_eq!(fs::read(destination.join("report (1).txt"))?, b"incoming");
    assert!(source.exists());
    Ok(())
}

#[test]
fn keeping_both_while_moving_renames_the_destination_instead_of_replacing_it()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source_dir = root.path().join("source");
    let source = source_dir.join("report.txt");
    let destination = root.path().join("dest");
    fs::create_dir_all(&source_dir)?;
    fs::create_dir_all(&destination)?;
    fs::write(&source, b"incoming")?;
    fs::write(destination.join("report.txt"), b"existing")?;

    let events = run_paste(PasteRequest {
        id: OperationRequestId(75),
        destination: Location::local(&destination),
        items: vec![PasteItem {
            source: Location::local(&source),
            conflict: TransferConflict::KeepBoth,
            target_name: None,
        }],
        move_sources: true,
    });

    assert_eq!(
        item_moves(&events),
        [(
            Location::local(&source),
            Location::local(destination.join("report (1).txt"))
        )]
    );
    assert_eq!(fs::read(destination.join("report.txt"))?, b"existing");
    assert_eq!(fs::read(destination.join("report (1).txt"))?, b"incoming");
    assert!(!source.exists());
    Ok(())
}

#[test]
fn keeping_both_numbers_a_long_name_within_the_limit_of_its_mount() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let repeat = |text: &str, count| text.repeat(count);
    let rows = vec![
        (
            format!("{}.txt", repeat("a", 251)),
            false,
            format!("{} (1).txt", repeat("a", 247)),
        ),
        (repeat("d", 254), true, format!("{} (1)", repeat("d", 251))),
        (
            format!("{}.tar.gz", repeat("a", 247)),
            false,
            format!("{}.tar (1).gz", repeat("a", 244)),
        ),
    ];
    let samba_folder = vec![(repeat("d", 212), true, format!("{} (1)", repeat("d", 209)))];
    let ntfs_3g = || Quirks {
        rename_flags: fs_quirks::Support::Rejected(rustix::io::Errno::INVAL),
        ..Quirks::ntfs_3g()
    };
    let mounts: [(&str, Option<Preset>, _); 4] = [
        ("host", None, &rows),
        ("vfat", Some(Quirks::vfat), &rows),
        ("cifs", Some(Quirks::cifs), &samba_folder),
        ("ntfs-3g", Some(ntfs_3g), &rows),
    ];
    for (mount, preset, rows) in mounts {
        for moving in [false, true] {
            for (name, folder, numbered) in rows {
                let context = format!("{mount}, moving: {moving}, {name}");
                let root = tempfile::tempdir()?;
                let _quirks = preset.map(|preset| FsQuirks::install(root.path(), preset()));
                let sources = root.path().join("sources");
                let source = sources.join(name);
                let destination = root.path().join("destination");
                fs::create_dir(&sources)?;
                fs::create_dir(&destination)?;
                let incoming = if *folder {
                    fs::create_dir(&source)?;
                    fs::create_dir(destination.join(name))?;
                    fs::write(source.join("inside.txt"), b"incoming")?;
                    destination.join(numbered).join("inside.txt")
                } else {
                    fs::write(&source, b"incoming")?;
                    fs::write(destination.join(name), b"existing")?;
                    destination.join(numbered)
                };

                run_paste(PasteRequest {
                    id: OperationRequestId(78),
                    destination: Location::local(&destination),
                    items: vec![PasteItem {
                        source: Location::local(&source),
                        conflict: TransferConflict::KeepBoth,
                        target_name: None,
                    }],
                    move_sources: moving,
                });

                let mut expected = vec![name.clone(), numbered.clone()];
                expected.sort();
                assert_eq!(names(&destination)?, expected, "{context}");
                assert_eq!(fs::read(incoming)?, b"incoming", "{context}");
                assert_eq!(source.exists(), !moving, "{context}");
            }
        }
    }
    Ok(())
}

#[test]
fn mixed_conflict_choices_apply_independently_across_a_multi_item_paste()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let sources = root.path().join("sources");
    let destination = root.path().join("destination");
    fs::create_dir_all(&sources)?;
    fs::create_dir_all(&destination)?;
    fs::write(sources.join("new.txt"), b"brand new")?;
    fs::write(sources.join("replace.txt"), b"new replacement")?;
    fs::write(destination.join("replace.txt"), b"old replacement")?;
    fs::write(sources.join("keep.txt"), b"new keep")?;
    fs::write(destination.join("keep.txt"), b"old keep")?;

    let events = RefCell::new(run_paste_events(PasteRequest {
        id: OperationRequestId(76),
        destination: Location::local(&destination),
        items: vec![
            PasteItem {
                source: Location::local(sources.join("new.txt")),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            },
            PasteItem {
                source: Location::local(sources.join("replace.txt")),
                conflict: TransferConflict::ReplaceExisting,
                target_name: None,
            },
            PasteItem {
                source: Location::local(sources.join("keep.txt")),
                conflict: TransferConflict::KeepBoth,
                target_name: None,
            },
        ],
        move_sources: false,
    }));

    assert!(matches!(
        events.borrow().last(),
        Some(OperationEvent::Pasted { .. })
    ));
    assert_eq!(fs::read(destination.join("new.txt"))?, b"brand new");
    assert_eq!(
        fs::read(destination.join("replace.txt"))?,
        b"new replacement"
    );
    assert_eq!(fs::read(destination.join("keep.txt"))?, b"old keep");
    assert_eq!(fs::read(destination.join("keep (1).txt"))?, b"new keep");
    Ok(())
}

#[test]
fn pasting_onto_itself_with_replace_is_a_noop() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let destination = root.path().to_path_buf();
    let source = destination.join("todo.txt");
    fs::write(&source, b"existing\n")?;

    let events = RefCell::new(run_paste_events(PasteRequest {
        id: OperationRequestId(32),
        destination: Location::local(&destination),
        items: vec![PasteItem {
            source: Location::local(&source),
            conflict: TransferConflict::ReplaceExisting,
            target_name: None,
        }],
        move_sources: false,
    }));

    assert!(matches!(
        events.borrow().last(),
        Some(OperationEvent::Pasted { .. })
    ));
    assert!(source.exists());
    assert_eq!(fs::read(&source)?, b"existing\n");
    assert!(!destination.join("todo (1).txt").exists());
    Ok(())
}

#[test]
fn pasting_onto_itself_with_keep_both_creates_numbered_copy() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let destination = root.path().to_path_buf();
    let source = destination.join("todo.txt");
    fs::write(&source, b"existing\n")?;

    let events = RefCell::new(run_paste_events(PasteRequest {
        id: OperationRequestId(33),
        destination: Location::local(&destination),
        items: vec![PasteItem {
            source: Location::local(&source),
            conflict: TransferConflict::KeepBoth,
            target_name: None,
        }],
        move_sources: false,
    }));

    assert!(matches!(
        events.borrow().last(),
        Some(OperationEvent::Pasted { .. })
    ));
    assert!(source.exists());
    assert_eq!(fs::read(&source)?, b"existing\n");
    assert_eq!(fs::read(destination.join("todo (1).txt"))?, b"existing\n");
    Ok(())
}

fn taking(path: &Path) -> fs_quirks::Racer {
    let path = path.to_path_buf();
    Box::new(move || {
        if fs::symlink_metadata(&path).is_err() {
            fs::write(&path, b"taken meanwhile").expect("take the name");
        }
    })
}

#[test]
fn keep_both_copy_renumbers_when_its_name_is_taken_meanwhile() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let unnamed_files_refused: Preset = || Quirks {
        tmpfile: fs_quirks::Support::Rejected(rustix::io::Errno::OPNOTSUPP),
        ..Quirks::ext4()
    };
    let rows: [(&str, &str, Preset); 3] = [
        ("report.txt", "report (1).txt", Quirks::default),
        ("report.txt", "report (1).txt", unnamed_files_refused),
        ("docs", "docs (1)", Quirks::default),
    ];
    for (name, first, quirks) in rows {
        let root = tempfile::tempdir()?;
        let sources = root.path().join("sources");
        let destination = root.path().join("destination");
        fs::create_dir_all(&sources)?;
        fs::create_dir_all(&destination)?;
        let source = sources.join(name);
        if name == "docs" {
            fs::create_dir(&source)?;
            fs::write(source.join("inside.txt"), b"incoming")?;
            fs::create_dir(destination.join(name))?;
        } else {
            fs::write(&source, b"incoming")?;
            fs::write(destination.join(name), b"existing")?;
        }
        let taken = destination.join(first);
        let _scope = FsQuirks::install(
            root.path(),
            Quirks {
                before: vec![
                    (fs_quirks::Op::LinkFd, first, 0, taking(&taken)),
                    (fs_quirks::Op::Rename, first, 0, taking(&taken)),
                ],
                ..quirks()
            },
        );

        let created = run_paste_collecting_created(PasteRequest {
            id: OperationRequestId(77),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(&source),
                conflict: TransferConflict::KeepBoth,
                target_name: None,
            }],
            move_sources: false,
        })?;

        let second = first.replace("(1)", "(2)");
        assert_eq!(
            created.into_iter().flatten().collect::<Vec<_>>(),
            [Location::local(destination.join(&second))],
            "{name}"
        );
        assert_eq!(fs::read(&taken)?, b"taken meanwhile", "{name}");
        let copied = destination.join(&second);
        let copied = if name == "docs" {
            copied.join("inside.txt")
        } else {
            copied
        };
        assert_eq!(fs::read(copied)?, b"incoming", "{name}");
    }
    Ok(())
}

#[test]
fn keep_both_takes_the_next_number_past_names_already_taken() -> Result<(), Box<dyn Error>> {
    use crate::adapters::volume::learn_mount_capability;
    use fs_quirks::Op;
    use std::os::unix::fs::FileTypeExt;
    let _serial = serial()?;
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        File,
        Folder,
        Link,
        Pipe,
    }
    /// A move renames within one volume, so its folder is numbered as one;
    /// copies cross volumes, and a folder is made in place where the mount
    /// is known to place none.
    #[derive(Clone, Copy, PartialEq)]
    enum Way {
        Moved,
        Copied,
        InPlace,
    }
    // Where a stage cannot be placed, it is copied out to a name another
    // program takes first.
    type Row = (
        &'static str,
        Kind,
        Way,
        Preset,
        &'static [&'static str],
        Option<(Op, &'static str, usize)>,
        &'static str,
    );
    let rows: [Row; 7] = [
        (
            "my.photos",
            Kind::Folder,
            Way::Moved,
            Quirks::default,
            &["my.photos"],
            None,
            "my.photos (1)",
        ),
        (
            "x (2).txt",
            Kind::File,
            Way::Copied,
            Quirks::default,
            &["x (2).txt"],
            None,
            "x (3).txt",
        ),
        (
            "link",
            Kind::Link,
            Way::Copied,
            Quirks::default,
            &["link", "link (1)"],
            None,
            "link (2)",
        ),
        (
            "pipe",
            Kind::Pipe,
            Way::Copied,
            Quirks::default,
            &["pipe", "pipe (1)"],
            None,
            "pipe (2)",
        ),
        (
            "docs",
            Kind::Folder,
            Way::InPlace,
            Quirks::without_folder_placement,
            &["docs", "docs (1)"],
            None,
            "docs (2)",
        ),
        (
            "docs",
            Kind::Folder,
            Way::Copied,
            Quirks::without_placement,
            &["docs"],
            Some((Op::Mkdir, "docs (1)", 1)),
            "docs (2)",
        ),
        (
            "a.txt",
            Kind::File,
            Way::Copied,
            Quirks::without_placement,
            &["a.txt"],
            Some((Op::Create, "a (1).txt", 0)),
            "a (2).txt",
        ),
    ];
    for (name, kind, way, preset, taken, racer, expected) in rows {
        let row = format!("{name} -> {expected}");
        let root = tempfile::tempdir()?;
        let sources = root.path().join("sources");
        let destination = root.path().join("destination");
        fs::create_dir_all(&sources)?;
        fs::create_dir_all(&destination)?;
        let source = sources.join(name);
        match kind {
            Kind::File => fs::write(&source, b"incoming")?,
            Kind::Folder => {
                fs::create_dir(&source)?;
                fs::write(source.join("inside.txt"), b"incoming")?;
            }
            Kind::Link => std::os::unix::fs::symlink("incoming", &source)?,
            Kind::Pipe => rustix::fs::mknodat(
                rustix::fs::CWD,
                &source,
                rustix::fs::FileType::Fifo,
                rustix::fs::Mode::from_raw_mode(0o600),
                0,
            )?,
        }
        let mut existing = taken.to_vec();
        for name in taken {
            fs::write(destination.join(name), b"existing")?;
        }
        let mut quirks = preset();
        if let Some((op, first, skip)) = racer {
            existing.push(first);
            quirks.before = vec![(op, first, skip, taking(&destination.join(first)))];
        }
        let moving = way == Way::Moved;
        let _sources = (!moving).then(|| FsQuirks::install(&sources, Quirks::default()));
        let scope = FsQuirks::install(if moving { root.path() } else { &destination }, quirks);
        if way == Way::InPlace {
            learn_mount_capability(scope.key(), |capabilities| {
                capabilities.no_replace = Some(false);
                capabilities.hard_links = Some(true);
                capabilities.rename_over_empty_directory = Some(false);
            });
        }

        run_paste(PasteRequest {
            id: OperationRequestId(79),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(&source),
                conflict: TransferConflict::KeepBoth,
                target_name: None,
            }],
            move_sources: moving,
        });

        existing.push(expected);
        existing.sort_unstable();
        assert_eq!(names(&destination)?, existing, "{row}");
        let copied = destination.join(expected);
        match kind {
            Kind::File => assert_eq!(fs::read(&copied)?, b"incoming", "{row}"),
            Kind::Folder => assert_eq!(fs::read(copied.join("inside.txt"))?, b"incoming", "{row}"),
            Kind::Link => assert_eq!(fs::read_link(&copied)?, Path::new("incoming"), "{row}"),
            Kind::Pipe => assert!(
                fs::symlink_metadata(&copied)?.file_type().is_fifo(),
                "{row}"
            ),
        }
    }
    Ok(())
}

#[test]
fn replace_and_merge_act_on_the_name_the_prompt_confirmed() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let (replace, merge) = (TransferConflict::ReplaceExisting, TransferConflict::Merge);
    let (vfat, ntfs3, cifs): (Preset, Preset, Preset) = (Quirks::vfat, Quirks::ntfs3, Quirks::cifs);
    for (preset, quirks, confirmed, fixed, conflict) in [
        ("vfat", vfat, "CON", Some("CON_"), replace),
        ("ntfs3", ntfs3, "a:b", Some("a_b"), replace),
        ("ntfs3", ntfs3, "a:b", None, replace),
        ("cifs", cifs, "a:b", Some("a_b"), replace),
        ("ntfs3", ntfs3, "x:y", Some("x_y"), merge),
        ("ntfs3", ntfs3, "x:y", None, merge),
    ] {
        let context = format!("{preset} {confirmed} beside {fixed:?}, {conflict:?}");
        let root = tempfile::tempdir()?;
        let (source, disk) = (root.path().join("source"), root.path().join("disk"));
        fs::create_dir_all(&source)?;
        fs::create_dir_all(&disk)?;
        let merging = conflict == TransferConflict::Merge;
        let file = |item: &str| {
            if merging {
                format!("{item}/same.txt")
            } else {
                item.to_owned()
            }
        };
        for item in std::iter::once(confirmed).chain(fixed) {
            if merging {
                fs::create_dir_all(disk.join(item))?;
            }
            fs::write(disk.join(file(item)), item)?;
        }
        if merging {
            fs::create_dir(source.join(confirmed))?;
        }
        fs::write(source.join(file(confirmed)), "incoming")?;
        let _disk = FsQuirks::install(&disk, quirks());

        run_paste(PasteRequest {
            id: OperationRequestId(1615),
            destination: Location::local(&disk),
            items: vec![PasteItem {
                source: Location::local(source.join(confirmed)),
                conflict,
                target_name: None,
            }],
            move_sources: false,
        });

        assert_eq!(
            fs::read(disk.join(file(confirmed)))?,
            b"incoming",
            "{context}"
        );
        if let Some(fixed) = fixed {
            assert_eq!(
                fs::read(disk.join(file(fixed)))?,
                fixed.as_bytes(),
                "{context}"
            );
        }
        assert_eq!(
            names(&disk)?.len(),
            1 + usize::from(fixed.is_some()),
            "{context}"
        );
    }
    Ok(())
}

#[test]
fn a_paste_gives_an_item_the_name_its_prompt_showed() -> Result<(), Box<dyn Error>> {
    use crate::services::RenameCause::{Alias, Invalid};
    let _serial = serial()?;
    let (replace, keep_both, merge, fail) = (
        TransferConflict::ReplaceExisting,
        TransferConflict::KeepBoth,
        TransferConflict::Merge,
        TransferConflict::FailIfExists,
    );
    // The item, how it collided, the name the prompt showed, whether it is
    // moved, where it lands and why.
    for (item, conflict, shown, moving, landed, cause) in [
        ("a:b", replace, "a_b", false, "a_b", Invalid),
        ("a:b", keep_both, "a_b", false, "a_b (1)", Invalid),
        ("a:b", keep_both, "a_b", true, "a_b (1)", Invalid),
        ("a:b", fail, "a_b (1)", false, "a_b (1)", Invalid),
        ("b.jpg", fail, "b (1).jpg", false, "b (1).jpg", Alias),
        ("x:y", merge, "x_y", false, "x_y", Invalid),
    ] {
        let context = format!("{item} as {shown}, {conflict:?}, moving {moving}");
        let root = tempfile::tempdir()?;
        let (source, disk) = (root.path().join("source"), root.path().join("disk"));
        fs::create_dir_all(&source)?;
        fs::create_dir_all(&disk)?;
        let merging = conflict == merge;
        let file = |item: &str| {
            if merging {
                format!("{item}/same.txt")
            } else {
                item.to_owned()
            }
        };
        if merging {
            fs::create_dir(source.join(item))?;
            fs::create_dir(disk.join(shown))?;
        }
        fs::write(source.join(file(item)), "incoming")?;
        fs::write(disk.join(file(&shown.replace(" (1)", ""))), "old")?;
        let _disk = FsQuirks::install(&disk, Quirks::vfat());

        let events = run_paste(PasteRequest {
            id: OperationRequestId(1615),
            destination: Location::local(&disk),
            items: vec![PasteItem {
                source: Location::local(source.join(item)),
                conflict,
                target_name: Some(ShownName {
                    name: OsString::from(shown),
                    cause: None,
                }),
            }],
            move_sources: moving,
        });

        assert_eq!(fs::read(disk.join(file(landed)))?, b"incoming", "{context}");
        assert_eq!(
            names(&disk)?.len(),
            1 + usize::from(landed != shown || conflict == fail),
            "{context}"
        );
        assert_eq!(source.join(item).exists(), !moving, "{context}");
        // Legacy reported why the prompt's name differed; the prompt chose it, and a job's
        // outcome gives the path the item landed at.
        let _ = (&events, cause);
    }
    Ok(())
}

#[test]
fn a_paste_fails_an_item_whose_shown_name_is_not_one_plain_name() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    for shown in ["../escaped", "sub/x", "a\0b"] {
        let root = tempfile::tempdir()?;
        let (source, disk) = (root.path().join("source"), root.path().join("disk"));
        fs::create_dir_all(&source)?;
        fs::create_dir_all(disk.join("sub"))?;
        fs::write(source.join("item"), "incoming")?;

        let events = RefCell::new(run_paste_events(PasteRequest {
            id: OperationRequestId(1615),
            destination: Location::local(&disk),
            items: vec![PasteItem {
                source: Location::local(source.join("item")),
                conflict: TransferConflict::FailIfExists,
                target_name: Some(ShownName {
                    name: OsString::from(shown),
                    cause: None,
                }),
            }],
            move_sources: false,
        }));
        wait_for_operation(&events, |event| {
            matches!(
                event,
                OperationEvent::Pasted { .. }
                    | OperationEvent::TransferFailed { .. }
                    | OperationEvent::Failed { .. }
            )
        });

        // Jobs refuse the item, not the whole request as legacy's provider did.
        assert!(
            matches!(
                events.borrow().last(),
                Some(OperationEvent::Failed { .. } | OperationEvent::TransferFailed { .. })
            ),
            "{shown:?}"
        );
        assert_eq!(names(root.path())?, ["disk", "source"], "{shown:?}");
        assert_eq!(names(&disk)?, ["sub"], "{shown:?}");
        assert!(names(&disk.join("sub"))?.is_empty(), "{shown:?}");
    }
    Ok(())
}

// AUDIT REPRO (throwaway, not for upstream): the exact items the conflict
// dialog sends when the user picks Replace (interface/actions.rs:323-340).
fn audit_paste_as_dialog_replace(
    destination: &Path,
    sources: &[std::path::PathBuf],
) -> Vec<OperationEvent> {
    let items = sources
        .iter()
        .map(|source| PasteItem {
            source: Location::local(source),
            conflict: if source.is_dir() {
                TransferConflict::Merge
            } else {
                TransferConflict::ReplaceExisting
            },
            target_name: None,
        })
        .collect();
    let events = RefCell::new(run_paste_events(PasteRequest {
        id: OperationRequestId(9001),
        destination: Location::local(destination),
        items,
        move_sources: false,
    }));
    let _deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let out = events.borrow().clone();
    eprintln!("AUDIT events: {:#?}", out);
    out
}

#[test]
fn audit_repro_single_file_replace() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let (src, dst) = (root.path().join("src"), root.path().join("dst"));
    fs::create_dir_all(&src)?;
    fs::create_dir_all(&dst)?;
    fs::write(src.join("a.txt"), b"new")?;
    fs::write(dst.join("a.txt"), b"old")?;
    let events = audit_paste_as_dialog_replace(&dst, &[src.join("a.txt")]);
    eprintln!("AUDIT dst names: {:?}", names(&dst)?);
    assert!(
        matches!(events.last(), Some(OperationEvent::Pasted { .. })),
        "terminal event"
    );
    assert_eq!(fs::read(dst.join("a.txt"))?, b"new");
    Ok(())
}

#[test]
fn audit_repro_two_files_one_clashing_replace() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let (src, dst) = (root.path().join("src"), root.path().join("dst"));
    fs::create_dir_all(&src)?;
    fs::create_dir_all(&dst)?;
    fs::write(src.join("a.txt"), b"new a")?;
    fs::write(src.join("b.txt"), b"new b")?;
    fs::write(dst.join("a.txt"), b"old a")?;
    let events = audit_paste_as_dialog_replace(&dst, &[src.join("a.txt"), src.join("b.txt")]);
    eprintln!("AUDIT dst names: {:?}", names(&dst)?);
    assert!(
        matches!(events.last(), Some(OperationEvent::Pasted { .. })),
        "terminal event"
    );
    assert_eq!(fs::read(dst.join("a.txt"))?, b"new a");
    assert_eq!(fs::read(dst.join("b.txt"))?, b"new b");
    Ok(())
}

#[test]
fn audit_repro_folder_and_file_replace() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let (src, dst) = (root.path().join("src"), root.path().join("dst"));
    fs::create_dir_all(src.join("photos"))?;
    fs::create_dir_all(dst.join("photos"))?;
    fs::write(src.join("photos/one.jpg"), b"new one")?;
    fs::write(dst.join("photos/one.jpg"), b"old one")?;
    fs::write(src.join("notes.txt"), b"new notes")?;
    let events = audit_paste_as_dialog_replace(&dst, &[src.join("photos"), src.join("notes.txt")]);
    eprintln!(
        "AUDIT dst names: {:?} photos: {:?}",
        names(&dst)?,
        names(&dst.join("photos"))?
    );
    assert!(
        matches!(events.last(), Some(OperationEvent::Pasted { .. })),
        "terminal event"
    );
    assert_eq!(fs::read(dst.join("photos/one.jpg"))?, b"new one");
    assert_eq!(fs::read(dst.join("notes.txt"))?, b"new notes");
    Ok(())
}
