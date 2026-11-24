// SPDX-License-Identifier: MIT

use super::fs_quirks::{Op, Racer};
use super::*;

#[test]
fn copying_a_file_emits_bytes_before_item_completion() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.bin");
    let destination = root.path().join("destination");
    let contents = vec![0x5a; 1024 * 1024];
    fs::write(&source, &contents)?;
    fs::create_dir(&destination)?;

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(25),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(&source),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            }],
            move_sources: false,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    while !events.borrow().iter().any(|event| {
        matches!(
            event,
            OperationEvent::Pasted { .. }
                | OperationEvent::Cancelled { .. }
                | OperationEvent::TransferFailed { .. }
                | OperationEvent::Failed { .. }
        )
    }) {
        panic!("the paste ended without a terminal event");
    }

    assert!(matches!(
        events.borrow().last(),
        Some(OperationEvent::Pasted { .. })
    ));
    assert!(events.borrow().iter().any(|event| matches!(
        event,
        OperationEvent::TransferProgress {
            completed_items: 0,
            completed_files: 1,
            total_files: Some(1),
            transferred_bytes,
            total_bytes: Some(total_bytes),
            ..
        } if *transferred_bytes > 0 && *total_bytes == contents.len() as u64
    )));
    assert_eq!(fs::read(destination.join("source.bin"))?, contents);
    Ok(())
}

#[test]
fn nested_copy_reports_files_before_the_selected_folder_finishes() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source)?;
    fs::create_dir(&destination)?;
    for index in 0..32 {
        fs::write(
            source.join(format!("file-{index:02}.txt")),
            index.to_string(),
        )?;
    }
    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(26),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(&source),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            }],
            move_sources: false,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    while !events.borrow().iter().any(|event| {
        matches!(
            event,
            OperationEvent::Pasted { .. } | OperationEvent::TransferFailed { .. }
        )
    }) {
        panic!("the paste ended without a terminal event");
    }
    assert!(matches!(
        events.borrow().last(),
        Some(OperationEvent::Pasted { .. })
    ));
    assert!(events.borrow().iter().any(|event| matches!(
        event,
        OperationEvent::TransferProgress {
            completed_items: 0,
            completed_files,
            total_files: Some(32),
            ..
        } if *completed_files > 0
    )));
    for index in 0..32 {
        assert_eq!(
            fs::read_to_string(destination.join(format!("source/file-{index:02}.txt")))?,
            index.to_string()
        );
    }
    Ok(())
}

#[test]
fn copying_many_selected_files_completes_the_bounded_bulk_path() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source)?;
    fs::create_dir(&destination)?;
    let mut items = Vec::new();
    let mut expected = Vec::new();
    for index in 0..12 {
        let path = source.join(format!("file-{index:02}.txt"));
        fs::write(&path, index.to_string())?;
        expected.push(Location::local(&path));
        items.push(PasteItem {
            source: Location::local(path),
            conflict: TransferConflict::FailIfExists,
            target_name: None,
        });
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(27),
            destination: Location::local(&destination),
            items,
            move_sources: false,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    while !events.borrow().iter().any(|event| {
        matches!(
            event,
            OperationEvent::Pasted { .. } | OperationEvent::TransferFailed { .. }
        )
    }) {
        panic!("the paste ended without a terminal event");
    }

    assert!(matches!(
        events.borrow().last(),
        Some(OperationEvent::Pasted { locations, .. }) if locations == &expected
    ));
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(
                event,
                OperationEvent::TransferProgress {
                    created_location: Some(_),
                    ..
                }
            ))
            .count(),
        expected.len()
    );
    for index in 0..12 {
        assert_eq!(
            fs::read_to_string(destination.join(format!("file-{index:02}.txt")))?,
            index.to_string()
        );
    }
    Ok(())
}

#[test]
fn linked_names_count_as_copied_before_their_folder_is_published() -> Result<(), Box<dyn Error>> {
    use std::{
        os::unix::fs::MetadataExt,
        sync::atomic::{AtomicBool, Ordering},
    };

    use super::fs_quirks::{Op, Racer};

    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let tree = root.path().join("source/tree");
    let destination = root.path().join("destination");
    fs::create_dir_all(tree.join("sub"))?;
    fs::create_dir(&destination)?;
    let size = 1024 * 1024;
    fs::write(tree.join("a"), vec![7; size])?;
    fs::hard_link(tree.join("a"), tree.join("sub/b"))?;
    let counted = Arc::new(AtomicBool::new(false));
    let waiting = counted.clone();
    // Holds the publish until both names were reported as copied.
    let racer: Racer = Box::new(move || {
        let start = Instant::now();
        while !waiting.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(5));
        }
    });
    let _destination = FsQuirks::install(
        &destination,
        Quirks {
            before: vec![(Op::Rename, ".asc-files-copy-*", 0, racer)],
            ..Quirks::default()
        },
    );

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(31),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(&tree),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            }],
            move_sources: false,
        },
        Rc::new(move |event| {
            if let OperationEvent::TransferProgress {
                completed_items: 0,
                transferred_bytes,
                ..
            } = event
                && transferred_bytes == 2 * size as u64
            {
                counted.store(true, Ordering::Release);
            }
            emitted.borrow_mut().push(event);
        }),
    );
    drive_until_transfer_settles(&events);

    let events = events.borrow();
    assert!(
        matches!(events.last(), Some(OperationEvent::Pasted { .. })),
        "{events:?}"
    );
    let copied = destination.join("tree");
    let (a, b) = (
        fs::metadata(copied.join("a"))?,
        fs::metadata(copied.join("sub/b"))?,
    );
    assert_eq!((a.ino(), a.nlink()), (b.ino(), 2));
    assert!(
        events.iter().any(|event| matches!(
            event,
            OperationEvent::TransferProgress {
                completed_items: 0,
                transferred_bytes,
                ..
            } if *transferred_bytes == 2 * size as u64
        )),
        "{events:?}"
    );
    Ok(())
}

#[test]
fn copied_links_count_their_bytes_as_they_are_made() -> Result<(), Box<dyn Error>> {
    use std::sync::Mutex;

    use super::fs_quirks::{Op, Racer};

    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let tree = root.path().join("source/tree");
    let destination = root.path().join("destination");
    fs::create_dir_all(&tree)?;
    fs::create_dir(&destination)?;
    std::os::unix::fs::symlink("first target", tree.join("a-link"))?;
    std::os::unix::fs::symlink("second", tree.join("b-link"))?;
    fs::write(tree.join("file.bin"), vec![7; 64 * 1024])?;
    let links = Arc::new(Mutex::new(None::<u64>));
    let seen = links.clone();
    // Holds the file until both links were reported, with the bytes so far.
    let racer: Racer = Box::new(move || {
        let start = Instant::now();
        while seen.lock().expect("links").is_none() && start.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(5));
        }
    });
    let _destination = FsQuirks::install(
        &destination,
        Quirks {
            before: vec![(Op::Write, "*", 0, racer)],
            ..Quirks::default()
        },
    );

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let recorded = links.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(33),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(&tree),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            }],
            move_sources: false,
        },
        Rc::new(move |event| {
            if let OperationEvent::TransferProgress {
                completed_files: 2,
                transferred_bytes,
                ..
            } = event
            {
                recorded
                    .lock()
                    .expect("links")
                    .get_or_insert(transferred_bytes);
            }
            emitted.borrow_mut().push(event);
        }),
    );
    drive_until_transfer_settles(&events);

    assert!(
        matches!(events.borrow().last(), Some(OperationEvent::Pasted { .. })),
        "{:?}",
        events.borrow()
    );
    let lengths = ("first target".len() + "second".len()) as u64;
    assert_eq!(*links.lock().expect("links"), Some(lengths));
    Ok(())
}

#[test]
fn a_folder_copied_out_of_its_stage_counts_each_file_once() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let tree = root.path().join("source/tree");
    let destination = root.path().join("destination");
    fs::create_dir_all(tree.join("sub"))?;
    fs::create_dir(&destination)?;
    for index in 0..4 {
        fs::write(tree.join(index.to_string()), b"x")?;
        fs::write(tree.join(format!("sub/{index}")), b"y")?;
    }
    std::os::unix::fs::symlink("0", tree.join("link"))?;
    // The stage cannot be placed, so it is copied to its final name.
    let _destination = FsQuirks::install(&destination, Quirks::without_placement());

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(32),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(&tree),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            }],
            move_sources: false,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    drive_until_transfer_settles(&events);

    let events = events.borrow();
    assert!(
        matches!(events.last(), Some(OperationEvent::Pasted { .. })),
        "{events:?}"
    );
    assert_eq!(fs::read(destination.join("tree/sub/3"))?, b"y");
    let files = events
        .iter()
        .filter_map(|event| match event {
            OperationEvent::TransferProgress {
                completed_files,
                total_files,
                ..
            } => Some((*completed_files, *total_files)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        files
            .iter()
            .all(|files| *files == (files.0, Some(9)) && files.0 <= 9),
        "{files:?}"
    );
    Ok(())
}

#[test]
fn bulk_copy_preserves_symlinks_and_one_file_directories() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source)?;
    fs::create_dir(&destination)?;

    let mut items = Vec::new();
    let mut expected = Vec::new();
    for index in 0..9 {
        let path = source.join(format!("file-{index:02}.txt"));
        fs::write(&path, index.to_string())?;
        expected.push(Location::local(&path));
        items.push(PasteItem {
            source: Location::local(path),
            conflict: TransferConflict::FailIfExists,
            target_name: None,
        });
    }
    let folder = source.join("folder");
    fs::create_dir(&folder)?;
    fs::write(folder.join("nested.txt"), b"nested")?;
    expected.push(Location::local(&folder));
    items.push(PasteItem {
        source: Location::local(&folder),
        conflict: TransferConflict::FailIfExists,
        target_name: None,
    });
    let link = source.join("link");
    std::os::unix::fs::symlink("file-00.txt", &link)?;
    expected.push(Location::local(&link));
    items.push(PasteItem {
        source: Location::local(&link),
        conflict: TransferConflict::FailIfExists,
        target_name: None,
    });

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(28),
            destination: Location::local(&destination),
            items,
            move_sources: false,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    while !events.borrow().iter().any(|event| {
        matches!(
            event,
            OperationEvent::Pasted { .. } | OperationEvent::TransferFailed { .. }
        )
    }) {
        panic!("the paste ended without a terminal event");
    }

    assert!(matches!(
        events.borrow().last(),
        Some(OperationEvent::Pasted { locations, .. }) if locations == &expected
    ));
    assert_eq!(fs::read(destination.join("folder/nested.txt"))?, b"nested");
    assert_eq!(
        fs::read_link(destination.join("link"))?,
        PathBuf::from("file-00.txt")
    );
    Ok(())
}

#[test]
fn fat32_size_check_rejects_oversized_nested_file_before_copy() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    const FAT32_MAX_FILE_SIZE: u64 = 4 * 1024 * 1024 * 1024 - 1;
    let root = tempfile::tempdir()?;
    let sources = root.path().join("sources");
    let stick = root.path().join("stick");
    let nested = sources.join("folder");
    fs::create_dir_all(&nested)?;
    fs::create_dir(&stick)?;
    // Sparse, so the test writes almost nothing.
    fs::File::create(nested.join("ubuntu.iso"))?.set_len(FAT32_MAX_FILE_SIZE + 1)?;
    let _stick = FsQuirks::install(&stick, Quirks::vfat());

    let events = run_paste_events(PasteRequest {
        id: OperationRequestId(31),
        destination: Location::local(&stick),
        items: vec![PasteItem {
            source: Location::local(&nested),
            conflict: TransferConflict::FailIfExists,
            target_name: None,
        }],
        move_sources: false,
    });

    let Some(OperationEvent::TransferFailed { message, .. }) = events.last() else {
        return Err(format!("the paste is refused: {events:?}").into());
    };
    assert!(
        message.contains(" is too large for a FAT32 drive "),
        "{message}"
    );
    assert!(message.contains("ubuntu.iso"), "{message}");
    assert!(message.contains("exFAT"), "{message}");
    assert!(
        names(&stick)?.is_empty(),
        "nothing is written before the refusal"
    );

    // Without a limit, the folder is measured.
    let measured = crate::ops::measure(&nested, None, &crate::ops::Cancel::new())?;
    assert_eq!(
        measured,
        Some(crate::ops::Measured {
            bytes: FAT32_MAX_FILE_SIZE + 1,
            files: 1
        })
    );
    Ok(())
}
