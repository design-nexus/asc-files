// SPDX-License-Identifier: MIT

use rustix::io::Errno;

use super::fs_quirks::{Op, Racer, Trash};
use super::*;

#[test]
fn failed_final_flush_reports_that_writes_may_still_be_pending() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let destination = root.path().join("usb");
    fs::create_dir(&destination)?;
    let source = root.path().join("photo.bin");
    fs::write(&source, b"pixels")?;
    let _guard = RemovableFlushGuard::install(root.path(), Some(io::ErrorKind::Other));
    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(106),
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
    pump_until_transfer(&events);
    assert!(matches!(terminal_transfer(&events.borrow()),
        Some(OperationEvent::TransferFailed { message, .. })
        if message.contains("writes may still be pending") && message.contains("wait for safe eject")));
    Ok(())
}

#[test]
fn the_barrier_before_deleting_a_moved_source_follows_the_destination_family()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    // The family, what udev records the device as holding (an error is a
    // lookup that fails on an NTFS device), the kernel, whether the device
    // is removable, whether the paste moves, whether each written file is
    // synced before it is closed, and the barrier's calls.
    type Device = Option<Result<&'static str, Errno>>;
    const NTFS: Device = Some(Ok("ntfs"));
    const EXFAT: Device = Some(Ok("exfat"));
    const HFS: Device = Some(Ok("hfsplus"));
    const FAILS: Device = Some(Err(Errno::ACCESS));
    const FS: &[Op] = &[Op::SyncFs];
    const DIR: &[Op] = &[Op::SyncDir];
    const BOTH: &[Op] = &[Op::SyncDir, Op::SyncFs, Op::SyncDir];
    const FS_THEN_DIR: &[Op] = &[Op::SyncFs, Op::SyncDir];
    type Row = (
        &'static str,
        Device,
        Option<(u32, u32, u32)>,
        bool,
        bool,
        bool,
        &'static [Op],
    );
    let rows: [Row; 13] = [
        ("ext4", None, None, false, true, false, FS),
        ("vfat", None, None, false, true, false, BOTH),
        ("ntfs3", None, None, false, true, false, BOTH),
        ("fuseblk", None, None, false, true, true, DIR),
        ("fuseblk", NTFS, None, false, true, false, BOTH),
        ("fuseblk", EXFAT, None, false, true, false, BOTH),
        ("fuseblk", HFS, None, false, true, true, DIR),
        ("fuseblk", FAILS, None, false, true, true, DIR),
        ("cifs", None, None, false, true, true, &[]),
        ("9p", None, None, false, true, true, FS),
        ("ext4", None, None, true, true, false, FS_THEN_DIR),
        ("cifs", None, None, false, false, false, &[]),
        ("ext4", None, Some((5, 4, 0)), false, true, true, FS),
    ];
    for (id, (fs_type, device, kernel, removable, moving, each_file, barrier)) in (300..).zip(rows)
    {
        let row = format!(
            "{fs_type} holding {device:?} on {kernel:?} removable: {removable} moving: {moving}"
        );
        let _kernel = KernelGuard::set(kernel);
        let root = tempfile::tempdir()?;
        let sources = root.path().join("sources");
        let folder = sources.join("folder");
        fs::create_dir_all(&folder)?;
        fs::write(folder.join("a.txt"), b"a")?;
        fs::write(folder.join("b.txt"), b"bb")?;
        let destination = root.path().join("destination");
        fs::create_dir(&destination)?;
        let _removable = removable.then(|| RemovableFlushGuard::install(&destination, None));
        let destination_quirks = FsQuirks::install(
            &destination,
            Quirks {
                mount_type: Some((fs_type, "rw")),
                device_format: device.map(|format| format.unwrap_or("ntfs")),
                fail: match device {
                    Some(Err(error)) => vec![(Op::DeviceFormat, "*", 0, error)],
                    _ => Vec::new(),
                },
                ..Quirks::default()
            },
        );
        let at_first_unlink = Arc::new(std::sync::Mutex::new(None));
        let (record, read) = (at_first_unlink.clone(), destination_quirks.reader());
        let _sources = FsQuirks::install(
            &sources,
            Quirks {
                before: vec![(
                    Op::Unlink,
                    "*",
                    0,
                    Box::new(move || {
                        *record.lock().unwrap_or_else(|error| error.into_inner()) = Some(read());
                    }),
                )],
                ..Quirks::default()
            },
        );

        let events = Rc::new(RefCell::new(Vec::new()));
        let emitted = events.clone();
        let _operation = LocalOperationProvider.paste(
            PasteRequest {
                id: OperationRequestId(id),
                destination: Location::local(&destination),
                items: vec![PasteItem {
                    source: Location::local(&folder),
                    conflict: TransferConflict::FailIfExists,
                    target_name: None,
                }],
                move_sources: moving,
            },
            Rc::new(move |event| emitted.borrow_mut().push(event)),
        );
        pump_until_transfer(&events);

        assert!(
            matches!(
                terminal_transfer(&events.borrow()),
                Some(OperationEvent::Pasted { .. })
            ),
            "{row}: {:?}",
            events.borrow()
        );
        assert_eq!(fs::read(destination.join("folder/b.txt"))?, b"bb", "{row}");
        assert_eq!(folder.exists(), !moving, "{row}");
        let flushing = events
            .borrow()
            .iter()
            .any(|event| matches!(event, OperationEvent::FlushingToDevice { .. }));
        assert_eq!(
            flushing,
            !barrier.is_empty(),
            "{row}: the device flush shows"
        );
        let calls = if moving {
            at_first_unlink
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .ok_or("the source was deleted")?
        } else {
            destination_quirks.calls()
        };
        let position = |op: Op, name: &str| {
            calls
                .iter()
                .position(|call| call.op == op && call.names.iter().any(|named| named == name))
        };
        for name in ["a.txt", "b.txt"] {
            let synced = position(Op::SyncData, name);
            assert_eq!(synced.is_some(), each_file, "{row}: {name}");
            if let Some(synced) = synced {
                assert!(
                    position(Op::Close, name).is_some_and(|closed| synced < closed),
                    "{row}: {name} is synced before it is closed"
                );
            }
        }
        let reached: Vec<Op> = calls
            .iter()
            .map(|call| call.op)
            .filter(|op| matches!(op, Op::SyncFs | Op::SyncDir))
            .collect();
        assert_eq!(reached, barrier, "{row}");
    }
    Ok(())
}

#[test]
fn a_merging_move_flushes_each_filesystem_it_wrote_to_before_deleting() -> Result<(), Box<dyn Error>>
{
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let sources = root.path().join("sources");
    let destination = root.path().join("destination");
    fs::create_dir_all(sources.join("media/sub"))?;
    fs::create_dir_all(destination.join("media/sub"))?;
    fs::write(sources.join("media/sub/new.bin"), b"new")?;
    let outer = FsQuirks::install(&destination, Quirks::ext4());
    let nested = FsQuirks::install(&destination.join("media/sub"), Quirks::ext4());
    let at_first_unlink = Arc::new(std::sync::Mutex::new(None));
    let (record, read) = (at_first_unlink.clone(), nested.reader());
    let _sources = FsQuirks::install(
        &sources,
        Quirks {
            before: vec![(
                Op::Unlink,
                "*",
                0,
                Box::new(move || {
                    *record.lock().unwrap_or_else(|error| error.into_inner()) = Some(read());
                }),
            )],
            ..Quirks::default()
        },
    );

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(360),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(sources.join("media")),
                conflict: TransferConflict::Merge,
                target_name: None,
            }],
            move_sources: true,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    pump_until_transfer(&events);

    assert!(
        matches!(
            terminal_transfer(&events.borrow()),
            Some(OperationEvent::Pasted { .. })
        ),
        "{:?}",
        events.borrow()
    );
    assert_eq!(fs::read(destination.join("media/sub/new.bin"))?, b"new");
    assert!(!sources.join("media").exists());
    let calls: Vec<Op> = at_first_unlink
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .ok_or("a source was deleted")?
        .iter()
        .map(|call| call.op)
        .collect();
    let opened = calls.iter().position(|op| *op == Op::OpenDir);
    let written = calls
        .iter()
        .position(|op| matches!(op, Op::Write | Op::CopyRange | Op::Clone));
    assert!(
        opened.is_some_and(|opened| written.is_some_and(|written| opened < written)),
        "the nested filesystem's barrier is opened before the copy writes there: {calls:?}"
    );
    assert!(calls.contains(&Op::SyncFs), "{calls:?}");
    assert!(outer.calls().iter().any(|call| call.op == Op::SyncFs));
    Ok(())
}

#[test]
fn a_replace_on_smb_syncs_its_copy_only_where_the_original_is_deleted() -> Result<(), Box<dyn Error>>
{
    use super::fs_quirks::Trash;
    let _serial = serial()?;
    for (id, trash, synced) in [(380, Trash::Supported, false), (381, Trash::Refused, true)] {
        let row = format!("{trash:?}");
        let root = tempfile::tempdir()?;
        let sources = root.path().join("sources");
        let destination = root.path().join("destination");
        fs::create_dir_all(sources.join("folder"))?;
        fs::write(sources.join("folder/a.txt"), b"a")?;
        fs::write(sources.join("folder/b.txt"), b"b")?;
        fs::create_dir_all(destination.join("folder"))?;
        fs::write(destination.join("folder/old.txt"), b"old")?;
        let _sources = FsQuirks::install(&sources, Quirks::default());
        let scope = FsQuirks::install(
            &destination,
            Quirks {
                mount_type: Some(("cifs", "rw")),
                fs_type: Some(0xFF53_4D42),
                trash,
                ..Quirks::default()
            },
        );

        let events = run_paste(PasteRequest {
            id: OperationRequestId(id),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(sources.join("folder")),
                conflict: TransferConflict::ReplaceExisting,
                target_name: None,
            }],
            move_sources: false,
        });

        assert!(
            matches!(
                terminal_transfer(&events),
                Some(OperationEvent::Pasted { .. })
            ),
            "{row}: {events:?}"
        );
        assert_eq!(names(&destination.join("folder"))?, ["a.txt", "b.txt"]);
        let calls = scope.calls();
        let synced_at: Vec<usize> = calls
            .iter()
            .enumerate()
            .filter(|(_, call)| call.op == Op::SyncData)
            .map(|(index, _)| index)
            .collect();
        assert_eq!(synced_at.len(), if synced { 2 } else { 0 }, "{row}");
        let displaced = calls.iter().position(|call| {
            call.op == Op::Rename && call.names.first().is_some_and(|name| name == "folder")
        });
        assert!(
            synced_at
                .iter()
                .all(|synced| displaced.is_some_and(|displaced| *synced < displaced)),
            "{row}: the copy is synced before the original is displaced"
        );
    }
    Ok(())
}

#[test]
fn moved_files_are_synced_on_smb_whatever_the_conflict() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let conflicts = [
        TransferConflict::FailIfExists,
        TransferConflict::ReplaceExisting,
        TransferConflict::Merge,
    ];
    for (id, conflict) in (370..).zip(conflicts) {
        let root = tempfile::tempdir()?;
        let sources = root.path().join("sources");
        let destination = root.path().join("destination");
        fs::create_dir_all(&sources)?;
        fs::create_dir_all(&destination)?;
        let item = if conflict == TransferConflict::Merge {
            fs::create_dir(sources.join("folder"))?;
            fs::write(sources.join("folder/incoming.txt"), b"incoming")?;
            fs::create_dir(destination.join("folder"))?;
            sources.join("folder")
        } else {
            fs::write(sources.join("incoming.txt"), b"incoming")?;
            if conflict == TransferConflict::ReplaceExisting {
                fs::write(destination.join("incoming.txt"), b"old")?;
            }
            sources.join("incoming.txt")
        };
        let destination_quirks = FsQuirks::install(
            &destination,
            Quirks {
                mount_type: Some(("cifs", "rw")),
                trash: super::fs_quirks::Trash::Unsupported,
                ..Quirks::default()
            },
        );
        let at_first_unlink = Arc::new(std::sync::Mutex::new(None));
        let (record, read) = (at_first_unlink.clone(), destination_quirks.reader());
        let _sources = FsQuirks::install(
            &sources,
            Quirks {
                before: vec![(
                    Op::Unlink,
                    "*",
                    0,
                    Box::new(move || {
                        *record.lock().unwrap_or_else(|error| error.into_inner()) = Some(read());
                    }),
                )],
                ..Quirks::default()
            },
        );

        let events = Rc::new(RefCell::new(Vec::new()));
        let emitted = events.clone();
        let _operation = LocalOperationProvider.paste(
            PasteRequest {
                id: OperationRequestId(id),
                destination: Location::local(&destination),
                items: vec![PasteItem {
                    source: Location::local(&item),
                    conflict,
                    target_name: None,
                }],
                move_sources: true,
            },
            Rc::new(move |event| emitted.borrow_mut().push(event)),
        );
        pump_until_transfer(&events);

        assert!(
            matches!(
                terminal_transfer(&events.borrow()),
                Some(OperationEvent::Pasted { .. })
            ),
            "{conflict:?}: {:?}",
            events.borrow()
        );
        assert!(!item.exists(), "{conflict:?}");
        let calls = at_first_unlink
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .ok_or("a source was deleted")?;
        assert!(
            calls.iter().any(|call| call.op == Op::SyncData),
            "{conflict:?}: the copy is synced before its source is deleted: {:?}",
            calls.iter().map(|call| call.op).collect::<Vec<_>>()
        );
    }
    Ok(())
}

#[test]
fn an_original_that_cannot_be_deleted_stays_aside_and_its_replacing_move_completes()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let sources = root.path().join("sources");
    let destination = root.path().join("destination");
    fs::create_dir(&sources)?;
    fs::create_dir(&destination)?;
    let source = sources.join("notes.txt");
    fs::write(&source, b"incoming")?;
    fs::write(destination.join("notes.txt"), b"old")?;
    let (watched, foreign) = (destination.clone(), root.path().join("foreign"));
    let racer: super::fs_quirks::Racer = Box::new(move || {
        let originals = fs::read_dir(&watched)
            .expect("list the destination")
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".asc-files-")
            })
            .expect("the original is aside");
        fs::write(&foreign, b"foreign").expect("write the foreign file");
        fs::rename(&foreign, originals.path().join("notes.txt"))
            .expect("put it at the original's name");
    });
    let _sources = FsQuirks::install(&sources, Quirks::default());
    let _destination = FsQuirks::install(
        &destination,
        Quirks {
            trash: super::fs_quirks::Trash::Unsupported,
            before: vec![(Op::SyncFs, "*", 0, racer)],
            ..Quirks::ext4()
        },
    );
    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.paste(
        PasteRequest {
            id: OperationRequestId(470),
            destination: Location::local(&destination),
            items: vec![PasteItem {
                source: Location::local(&source),
                conflict: TransferConflict::ReplaceExisting,
                target_name: None,
            }],
            move_sources: true,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    pump_until_transfer(&events);

    let events = events.take();
    assert!(
        matches!(
            terminal_transfer(&events),
            Some(OperationEvent::Pasted { .. })
        ),
        "{events:?}"
    );
    assert_eq!(fs::read(destination.join("notes.txt"))?, b"incoming");
    assert!(!source.exists());
    let [originals, _] = names(&destination)?
        .try_into()
        .map_err(|_| "the originals folder and the replacement")?;
    let kept = destination.join(originals).join("notes.txt");
    assert_eq!(fs::read(&kept)?, b"foreign");
    let reported: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OperationEvent::ItemsReported { report, .. } => Some(report.kept.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(
        matches!(
            reported.as_slice(),
            [crate::services::KeptItem {
                location,
                cause: crate::services::KeptCause::OriginalNotRemoved(_),
            }] if location == &Location::local(&kept)
        ),
        "{reported:?}"
    );
    Ok(())
}
