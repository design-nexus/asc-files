// SPDX-License-Identifier: MIT

use super::fs_quirks::{Op, Racer, Support, hold};
use super::*;
use crate::services::{KeptCause, KeptItem};

#[test]
fn a_copy_reports_the_destination_it_created_and_no_moved_item() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("photo.jpg");
    let destination = root.path().join("album");
    fs::write(&source, b"original-content")?;
    fs::create_dir(&destination)?;

    let events = run_paste(PasteRequest {
        id: OperationRequestId(70),
        destination: Location::local(&destination),
        items: vec![PasteItem {
            source: Location::local(&source),
            conflict: TransferConflict::FailIfExists,
            target_name: None,
        }],
        move_sources: false,
    });

    let created = events
        .iter()
        .filter_map(|event| match event {
            OperationEvent::TransferProgress {
                created_location, ..
            } => created_location.clone(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        created,
        vec![Location::local(destination.join("photo.jpg"))]
    );
    assert!(item_moves(&events).is_empty());
    Ok(())
}

#[test]
fn duplicating_a_file_preserves_contents_and_reports_the_generated_name()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let destination = root.path().to_path_buf();
    let source = destination.join("photo.jpg");
    fs::write(&source, b"original-content")?;

    let created = run_paste_collecting_created(PasteRequest {
        id: OperationRequestId(71),
        destination: Location::local(&destination),
        items: vec![PasteItem {
            source: Location::local(&source),
            conflict: TransferConflict::FailIfExists,
            target_name: None,
        }],
        move_sources: false,
    })?;

    assert_eq!(
        created.into_iter().flatten().collect::<Vec<_>>(),
        vec![Location::local(destination.join("photo (1).jpg"))]
    );
    assert_eq!(fs::read(&source)?, b"original-content");
    assert_eq!(
        fs::read(destination.join("photo (1).jpg"))?,
        b"original-content"
    );
    Ok(())
}

#[test]
fn a_copy_that_replaces_an_existing_item_reports_its_destination() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("photo.jpg");
    let destination = root.path().join("album");
    fs::write(&source, b"new-content")?;
    fs::create_dir(&destination)?;
    fs::write(destination.join("photo.jpg"), b"old-content")?;

    let created = run_paste_collecting_created(PasteRequest {
        id: OperationRequestId(72),
        destination: Location::local(&destination),
        items: vec![PasteItem {
            source: Location::local(&source),
            conflict: TransferConflict::ReplaceExisting,
            target_name: None,
        }],
        move_sources: false,
    })?;

    assert_eq!(
        created.into_iter().flatten().collect::<Vec<_>>(),
        vec![Location::local(destination.join("photo.jpg"))]
    );
    Ok(())
}

#[test]
fn a_move_reports_where_each_item_landed_instead_of_a_created_destination()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let photo = root.path().join("photo.jpg");
    let notes = root.path().join("notes");
    let destination = root.path().join("album");
    fs::write(&photo, b"original-content")?;
    fs::create_dir(&notes)?;
    fs::create_dir(&destination)?;

    let events = run_paste(PasteRequest {
        id: OperationRequestId(73),
        destination: Location::local(&destination),
        items: [&photo, &notes]
            .map(|source| PasteItem {
                source: Location::local(source),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            })
            .into(),
        move_sources: true,
    });

    assert!(!events.iter().any(|event| matches!(
        event,
        OperationEvent::TransferProgress {
            created_location: Some(_),
            ..
        }
    )));
    assert_eq!(
        item_moves(&events),
        [
            (
                Location::local(&photo),
                Location::local(destination.join("photo.jpg"))
            ),
            (
                Location::local(&notes),
                Location::local(destination.join("notes"))
            ),
        ]
    );
    Ok(())
}

#[test]
fn an_item_replaced_without_trash_is_left_out_of_the_paste_undo() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    for move_sources in [false, true] {
        let root = tempfile::tempdir()?;
        let sources = root.path().join("sources");
        let destination = root.path().join("destination");
        fs::create_dir(&sources)?;
        fs::create_dir(&destination)?;
        fs::write(sources.join("replace.txt"), b"incoming")?;
        fs::write(sources.join("new.txt"), b"new")?;
        fs::write(destination.join("replace.txt"), b"only copy")?;
        let _quirks = FsQuirks::install(
            root.path(),
            Quirks {
                trash: super::fs_quirks::Trash::Unsupported,
                fs_type: Some(0x6969),
                ..Quirks::default()
            },
        );

        let events = run_paste(PasteRequest {
            id: OperationRequestId(74),
            destination: Location::local(&destination),
            items: vec![
                PasteItem {
                    source: Location::local(sources.join("replace.txt")),
                    conflict: TransferConflict::ReplaceExisting,
                    target_name: None,
                },
                PasteItem {
                    source: Location::local(sources.join("new.txt")),
                    conflict: TransferConflict::FailIfExists,
                    target_name: None,
                },
            ],
            move_sources,
        });

        let created = events
            .iter()
            .filter_map(|event| match event {
                OperationEvent::TransferProgress {
                    created_location, ..
                } => created_location.clone(),
                _ => None,
            })
            .collect::<Vec<_>>();
        let expected_created = if move_sources {
            Vec::new()
        } else {
            vec![Location::local(destination.join("new.txt"))]
        };
        assert_eq!(created, expected_created);
        assert!(!events.iter().any(|event| matches!(
            event,
            OperationEvent::Merged { .. } | OperationEvent::ItemTrashed { .. }
        )));
        let expected_moves = if move_sources {
            ["new.txt", "replace.txt"]
                .map(|name| {
                    (
                        Location::local(sources.join(name)),
                        Location::local(destination.join(name)),
                    )
                })
                .to_vec()
        } else {
            Vec::new()
        };
        // Legacy moved items without a conflict first; jobs keep the order asked for.
        let mut moves = item_moves(&events);
        moves.sort_by(|a, b| a.0.compare(&b.0));
        assert_eq!(moves, expected_moves);
        assert_eq!(fs::read(destination.join("replace.txt"))?, b"incoming");
    }
    Ok(())
}

#[test]
fn a_move_that_cannot_remove_the_original_it_replaced_keeps_it_and_its_undo()
-> Result<(), Box<dyn Error>> {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Original {
        Undeletable,
        StaysOpen,
        ClosesDuringTheWait,
        OpenInAProgram,
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Transfer {
        MoveWithinTheMount,
        MoveFromAnotherMount,
        Copy,
    }
    let _serial = serial()?;
    let rows: [(Original, Preset, Transfer); 7] = [
        (
            Original::Undeletable,
            Quirks::ext4,
            Transfer::MoveWithinTheMount,
        ),
        (
            Original::StaysOpen,
            Quirks::nfs3,
            Transfer::MoveWithinTheMount,
        ),
        (
            Original::ClosesDuringTheWait,
            Quirks::nfs3,
            Transfer::MoveWithinTheMount,
        ),
        (
            Original::OpenInAProgram,
            Quirks::cifs_posix,
            Transfer::MoveWithinTheMount,
        ),
        (
            Original::Undeletable,
            Quirks::ext4,
            Transfer::MoveFromAnotherMount,
        ),
        (
            Original::StaysOpen,
            Quirks::nfs3,
            Transfer::MoveFromAnotherMount,
        ),
        (Original::Undeletable, Quirks::ext4, Transfer::Copy),
    ];
    for (original, preset, transfer) in rows {
        let row = format!("{original:?}, {transfer:?}");
        let _clock = ClockGuard::set(SystemTime::now());
        let root = tempfile::tempdir()?;
        if original == Original::Undeletable && base_places_files_by_linking(root.path())? {
            eprintln!("skipped on a base that places files by linking them: {row}");
            continue;
        }
        let sources = root.path().join("sources");
        let destination = root.path().join("destination");
        fs::create_dir(&sources)?;
        fs::create_dir(&destination)?;
        fs::write(sources.join("replace.txt"), b"incoming")?;
        fs::write(destination.join("replace.txt"), b"only copy")?;
        let holds = Arc::new(std::sync::Mutex::new(Vec::new()));
        let releasing = holds.clone();
        let _quirks = FsQuirks::install(
            root.path(),
            Quirks {
                trash: super::fs_quirks::Trash::Unsupported,
                fail: match original {
                    Original::Undeletable => {
                        vec![(Op::Unlink, "replace.txt", 0, rustix::io::Errno::ACCESS)]
                    }
                    _ => Vec::new(),
                },
                after: match original {
                    Original::ClosesDuringTheWait => vec![(
                        Op::Unlink,
                        ".asc-files-originals-*",
                        0,
                        Box::new(move || releasing.lock().expect("the holds").clear()) as Racer,
                    )],
                    _ => Vec::new(),
                },
                holder: (original == Original::OpenInAProgram)
                    .then(|| crate::services::Holder::Program("Text Editor".to_owned())),
                ..preset()
            },
        );
        let _sources = (transfer == Transfer::MoveFromAnotherMount)
            .then(|| FsQuirks::install(&sources, Quirks::default()));
        if original != Original::Undeletable {
            let held = hold(&destination.join("replace.txt"));
            holds.lock().map_err(|_| "the holds")?.push(held);
        }

        let events = run_paste(PasteRequest {
            id: OperationRequestId(75),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(sources.join("replace.txt")),
                conflict: TransferConflict::ReplaceExisting,
                target_name: None,
            }],
            move_sources: transfer != Transfer::Copy,
        });

        let left = names(&destination)?;
        let inside = match left.as_slice() {
            [originals, _] => names(&destination.join(originals))?,
            _ => Vec::new(),
        };
        holds.lock().map_err(|_| "the holds")?.clear();
        assert_eq!(
            fs::read(destination.join("replace.txt"))?,
            b"incoming",
            "{row}"
        );
        let moved = (
            Location::local(sources.join("replace.txt")),
            Location::local(destination.join("replace.txt")),
        );
        if transfer == Transfer::Copy {
            assert_eq!(item_moves(&events), [], "{row}");
            assert_eq!(fs::read(sources.join("replace.txt"))?, b"incoming", "{row}");
        } else {
            assert_eq!(item_moves(&events), [moved], "{row}");
            assert!(!sources.join("replace.txt").exists(), "{row}");
        }
        assert!(
            matches!(events.last(), Some(OperationEvent::Pasted { .. })),
            "{row}: {events:?}"
        );
        let kept: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                OperationEvent::ItemsReported { report, .. } => Some(report.kept.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        if original == Original::ClosesDuringTheWait {
            assert_eq!(kept, [], "{row}");
            assert_eq!(left, ["replace.txt"], "{row}");
            continue;
        }
        let [originals, _] = left.as_slice() else {
            return Err(format!("{row}: the original stays aside: {left:?}").into());
        };
        let [
            KeptItem {
                location,
                cause: KeptCause::OriginalNotRemoved(error),
            },
        ] = kept.as_slice()
        else {
            return Err(format!("{row}: the original is reported as kept: {kept:?}").into());
        };
        let kept_original = destination.join(originals).join("replace.txt");
        assert_eq!(location, &Location::local(&kept_original), "{row}");
        assert!(!error.contains(".asc-files-"), "{row}: {error}");
        match original {
            Original::Undeletable => {
                assert_eq!(fs::read(&kept_original)?, b"only copy", "{row}");
            }
            Original::StaysOpen => {
                let [leftover] = inside.as_slice() else {
                    return Err(format!("{row}: only the leftover stays: {inside:?}").into());
                };
                assert!(leftover.starts_with(".nfs"), "{row}: {leftover}");
                let reason = crate::i18n::tr(
                    "A file in it is still open in another program. Close it and try again.",
                );
                assert_eq!(error, &reason, "{row}");
            }
            Original::OpenInAProgram => {
                assert_eq!(inside, Vec::<String>::new(), "{row}");
                let reason = rust_i18n::t!(
                    "A file in it is still open in %{program}. Close it there and try again.",
                    program = "Text Editor"
                );
                assert_eq!(error, &reason, "{row}");
            }
            Original::ClosesDuringTheWait => {}
        }
    }
    Ok(())
}
