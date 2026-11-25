// SPDX-License-Identifier: MIT
//! Legacy's archive provider tests. Legacy drove compress and extract through its GLib provider
//! and collected events; the stand-ins below call the engine's synchronous `compress` and
//! `extract` and give back the same events, so each test reads as it did.

use super::{
    ArchiveError, Compressed, Counters, Extracted, compress, copy_with_big_buf,
    decoders::{extract_7z_from_reader, extract_tar},
    extract,
    fixtures::{
        COMPRESSION_STAGE, EXTRACTION_STAGE, FixtureMember, HomeTrashGuard, corrupt_gzip_trailer,
        expected_mode, extract_zip, never_cancelled, set_times_without_following, stages,
        tempdir_on_home_device, test_file_entry, write_7z_entries, write_members,
        write_tar_entries, write_zip_stored,
    },
    listing::INVALID_ARCHIVE,
};
use crate::{
    adapters::local_operations::tests::{
        base_places_files_by_linking,
        fs_quirks::{FsQuirks, Op, Preset, Quirks, Trash},
    },
    model::Location,
    services::{
        ArchiveFormat, CancelledOperation, CompressRequest, ExtractRequest, KeptCause, KeptItem,
        OperationEvent, OperationRequestId, PasswordFailure, RenameCause, RenamedItem,
        ReplacedOriginal, SkippedItem, SpecialKind, TransferConflict, TransferReport,
    },
};
use rustix::io::Errno;
use std::{
    cell::RefCell,
    collections::HashSet,
    error::Error,
    ffi::OsStr,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize},
    },
};

fn cancelled_event(
    request_id: OperationRequestId,
    destination: &Location,
    completed: Vec<Location>,
    failed: Vec<Location>,
    not_attempted: Vec<Location>,
) -> OperationEvent {
    OperationEvent::Cancelled {
        request_id,
        result: CancelledOperation {
            completed,
            failed,
            not_attempted,
            affected_locations: HashSet::from([destination.clone()]),
        },
    }
}

fn failed_event(request_id: OperationRequestId, error: &ArchiveError) -> OperationEvent {
    OperationEvent::Failed {
        request_id,
        message: error.user_message(),
        password_failure: error.password_failure(),
    }
}

fn run_compression(request: CompressRequest) -> Vec<OperationEvent> {
    let sources: Vec<PathBuf> = request
        .entries
        .iter()
        .filter_map(|entry| entry.location.native_path().map(Path::to_path_buf))
        .collect();
    let Some(into) = request.destination.native_path() else {
        return vec![OperationEvent::Failed {
            request_id: request.id,
            message: "Archive destination must be a local path".to_owned(),
            password_failure: None,
        }];
    };
    let result = compress(
        &sources,
        into,
        &request.archive_name,
        request.format,
        request.password.as_deref(),
        request.conflict,
        &Counters::default(),
        &never_cancelled(),
    );
    match result {
        Ok(Compressed {
            archive,
            replaced,
            original_deleted,
            changes,
        }) => {
            let mut events = Vec::new();
            // An original the replace could not remove was kept aside: undo can't bring it back.
            let kept: Vec<KeptItem> = changes
                .kept
                .into_iter()
                .map(|kept| KeptItem {
                    location: Location::local(&kept.path),
                    cause: kept.cause,
                })
                .collect();
            let original_kept = kept
                .iter()
                .any(|item| matches!(item.cause, KeptCause::OriginalNotRemoved(_)));
            if !kept.is_empty() {
                events.push(OperationEvent::ItemsReported {
                    request_id: request.id,
                    report: TransferReport {
                        kept,
                        ..TransferReport::default()
                    },
                });
            }
            events.push(OperationEvent::Compressed {
                request_id: request.id,
                archive_name: archive
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                archive: Location::local(&archive),
                original: if original_deleted || original_kept {
                    Some(ReplacedOriginal::Deleted)
                } else {
                    replaced.map(|_| ReplacedOriginal::Trashed(None))
                },
                placed: None,
            });
            events
        }
        Err(ArchiveError::Cancelled) => vec![cancelled_event(
            request.id,
            &request.destination,
            Vec::new(),
            Vec::new(),
            request
                .entries
                .iter()
                .map(|entry| entry.location.clone())
                .collect(),
        )],
        Err(error) => vec![failed_event(request.id, &error)],
    }
}

fn entry_names(directory: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut names = fs::read_dir(directory)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    names.sort();
    Ok(names)
}

fn run_extraction_with(request: ExtractRequest, cancelled: Arc<AtomicBool>) -> Vec<OperationEvent> {
    let (Some(archive), Some(into)) = (
        request.entry.location.native_path(),
        request.destination.native_path(),
    ) else {
        return vec![OperationEvent::Failed {
            request_id: request.id,
            message: "Archive must be a local file".to_owned(),
            password_failure: None,
        }];
    };
    let mut report = TransferReport::default();
    let result = extract(
        archive,
        into,
        request.created_destination,
        request.password.as_deref(),
        &Counters::default(),
        &cancelled,
        &mut report,
    );
    // What it renamed or left out comes before how it ended, as legacy reported it.
    let mut events = Vec::new();
    if !report.is_empty() {
        events.push(OperationEvent::ItemsReported {
            request_id: request.id,
            report,
        });
    }
    match result {
        Ok(Extracted::Completed { first_name }) => events.push(OperationEvent::Extracted {
            request_id: request.id,
            created: first_name
                .iter()
                .filter_map(|name| request.destination.child(OsStr::new(name)))
                .collect(),
            first_name,
        }),
        Ok(Extracted::Cancelled {
            completed,
            failed,
            not_attempted,
        }) => events.push(cancelled_event(
            request.id,
            &request.destination,
            completed,
            failed,
            not_attempted,
        )),
        Err(ArchiveError::Cancelled) => events.push(cancelled_event(
            request.id,
            &request.destination,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )),
        Err(error) => events.push(failed_event(request.id, &error)),
    }
    events
}

fn run_extraction(request: ExtractRequest) -> Vec<OperationEvent> {
    run_extraction_with(request, never_cancelled())
}

fn run_extraction_cancelled(request: ExtractRequest) -> Vec<OperationEvent> {
    run_extraction_with(request, Arc::new(AtomicBool::new(true)))
}

#[test]
fn compression_provider_rejects_escaping_archive_names() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    let source = root.path().join("source.txt");
    fs::create_dir(&destination)?;
    fs::write(&source, b"source")?;

    let events = run_compression(CompressRequest {
        id: OperationRequestId(1),
        entries: vec![test_file_entry(&source)],
        destination: Location::local(&destination),
        archive_name: "../outside".to_owned(),
        conflict: TransferConflict::ReplaceExisting,
        format: ArchiveFormat::Zip,
        password: None,
    });

    assert!(matches!(events.as_slice(), [OperationEvent::Failed { .. }]));
    assert!(!root.path().join("outside.zip").exists());
    assert!(stages(&destination, COMPRESSION_STAGE)?.is_empty());
    Ok(())
}

#[test]
fn an_archive_name_the_location_would_store_otherwise_is_refused_before_anything_is_written()
-> Result<(), Box<dyn Error>> {
    let rows: [(Preset, &str, Option<String>); 2] = [
        (
            Quirks::vfat,
            "nul",
            Some(
                rust_i18n::t!(
                    "“%{name}” is a reserved name in this location",
                    name = "nul"
                )
                .into_owned(),
            ),
        ),
        (Quirks::ext4, "a:b", None),
    ];
    for (quirks, archive_name, refusal) in rows {
        let root = tempfile::tempdir()?;
        let destination = root.path().join("destination");
        let source = root.path().join("source.txt");
        fs::create_dir(&destination)?;
        fs::write(&source, b"source")?;
        let _scope = FsQuirks::install(&destination, quirks());

        let events = run_compression(CompressRequest {
            id: OperationRequestId(1),
            entries: vec![test_file_entry(&source)],
            destination: Location::local(&destination),
            archive_name: archive_name.to_owned(),
            conflict: TransferConflict::FailIfExists,
            format: ArchiveFormat::Zip,
            password: None,
        });

        match refusal {
            Some(refusal) => {
                assert!(
                    matches!(events.last(), Some(OperationEvent::Failed { message, .. }) if *message == refusal),
                    "{archive_name}: {events:?}"
                );
                assert!(entry_names(&destination)?.is_empty(), "{archive_name}");
            }
            None => {
                assert!(
                    matches!(events.last(), Some(OperationEvent::Compressed { .. })),
                    "{archive_name}: {events:?}"
                );
                assert_eq!(entry_names(&destination)?, [format!("{archive_name}.zip")]);
            }
        }
    }
    Ok(())
}

#[test]
fn compression_conflict_choices_preserve_or_replace_the_destination() -> Result<(), Box<dyn Error>>
{
    let root = tempdir_on_home_device()?;
    let destination = root.path().join("destination");
    let source = root.path().join("source.txt");
    let archive = destination.join("existing.zip");
    let _trash = HomeTrashGuard::new(&archive);
    fs::create_dir(&destination)?;
    fs::write(&source, b"replacement")?;
    fs::write(&archive, b"original")?;
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o640))?;
    let request = |conflict| CompressRequest {
        id: OperationRequestId(1),
        entries: vec![test_file_entry(&source)],
        destination: Location::local(&destination),
        archive_name: "existing".to_owned(),
        conflict,
        format: ArchiveFormat::Zip,
        password: None,
    };

    let refused = run_compression(request(TransferConflict::FailIfExists));
    assert!(
        refused
            .iter()
            .any(|event| matches!(event, OperationEvent::Failed { .. }))
    );
    assert_eq!(fs::read(&archive)?, b"original");
    assert_eq!(fs::metadata(&archive)?.permissions().mode() & 0o777, 0o640);

    for suffix in [1, 2] {
        let kept = run_compression(request(TransferConflict::KeepBoth));
        let expected_name = format!("existing ({suffix}).zip");
        assert!(kept.iter().any(|event| matches!(event,
            OperationEvent::Compressed { archive_name, archive, original: None, .. }
                if archive_name == &expected_name
                    && archive == &Location::local(destination.join(&expected_name))
        )));
        let extracted = destination.join(format!("kept-{suffix}"));
        fs::create_dir(&extracted)?;
        extract_zip(&destination.join(expected_name), &extracted)?;
        assert_eq!(fs::read(extracted.join("source.txt"))?, b"replacement");
        assert_eq!(fs::read(&archive)?, b"original");
        assert_eq!(fs::metadata(&archive)?.permissions().mode() & 0o777, 0o640);
    }

    let replaced = run_compression(request(TransferConflict::ReplaceExisting));
    assert!(replaced.iter().any(|event| matches!(
        event,
        OperationEvent::Compressed {
            original: Some(_),
            ..
        }
    )));
    let extracted = destination.join("extracted");
    fs::create_dir(&extracted)?;
    assert_eq!(
        extract_zip(&archive, &extracted)?,
        Some("source.txt".to_owned())
    );
    assert_eq!(fs::metadata(&archive)?.permissions().mode() & 0o777, 0o640);
    assert!(stages(&destination, COMPRESSION_STAGE)?.is_empty());
    Ok(())
}

#[test]
fn compression_failure_preserves_an_existing_archive() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    let missing = root.path().join("missing.txt");
    let archive = destination.join("existing.zip");
    fs::create_dir(&destination)?;
    fs::write(&archive, b"original")?;

    let events = run_compression(CompressRequest {
        id: OperationRequestId(1),
        entries: vec![test_file_entry(&missing)],
        destination: Location::local(&destination),
        archive_name: "existing".to_owned(),
        conflict: TransferConflict::ReplaceExisting,
        format: ArchiveFormat::Zip,
        password: None,
    });

    assert!(
        events
            .iter()
            .any(|event| matches!(event, OperationEvent::Failed { .. }))
    );
    assert_eq!(fs::read(&archive)?, b"original");
    assert!(stages(&destination, COMPRESSION_STAGE)?.is_empty());
    Ok(())
}

#[test]
fn an_archive_that_replaced_an_original_it_cannot_remove_completes_and_reports_it()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    if base_places_files_by_linking(root.path())? {
        eprintln!("skipped on a base that places files by linking them");
        return Ok(());
    }
    let destination = root.path().join("destination");
    let source = root.path().join("source.txt");
    fs::create_dir(&destination)?;
    fs::write(&source, b"replacement")?;
    fs::write(destination.join("existing.zip"), b"original")?;
    let _scope = FsQuirks::install(
        &destination,
        Quirks {
            trash: Trash::Unsupported,
            fail: vec![(Op::Unlink, "existing.zip", 0, Errno::ACCESS)],
            ..Quirks::ext4()
        },
    );

    let events = run_compression(CompressRequest {
        id: OperationRequestId(4),
        entries: vec![test_file_entry(&source)],
        destination: Location::local(&destination),
        archive_name: "existing".to_owned(),
        conflict: TransferConflict::ReplaceExisting,
        format: ArchiveFormat::Zip,
        password: None,
    });

    assert!(
        matches!(
            events.last(),
            Some(OperationEvent::Compressed {
                archive_name,
                original: Some(ReplacedOriginal::Deleted),
                ..
            }) if archive_name == "existing.zip"
        ),
        "{events:?}"
    );
    let names = entry_names(&destination)?;
    let [originals, published] = names.as_slice() else {
        return Err(format!("the kept original and the archive: {names:?}").into());
    };
    assert!(originals.starts_with(".asc-files-originals-"), "{names:?}");
    assert_eq!(published, "existing.zip");
    let kept_original = destination.join(originals).join("existing.zip");
    assert_eq!(fs::read(&kept_original)?, b"original");
    let extracted = root.path().join("extracted");
    fs::create_dir(&extracted)?;
    assert_eq!(
        extract_zip(&destination.join("existing.zip"), &extracted)?,
        Some("source.txt".to_owned())
    );
    let kept: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OperationEvent::ItemsReported {
                request_id: OperationRequestId(4),
                report,
            } => Some(report.kept.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let [
        KeptItem {
            location,
            cause: KeptCause::OriginalNotRemoved(error),
        },
    ] = kept.as_slice()
    else {
        return Err(format!("the original is reported as kept: {kept:?}").into());
    };
    assert_eq!(location, &Location::local(&kept_original));
    assert!(!error.contains(".asc-files-"), "{error}");
    Ok(())
}

#[test]
fn every_compression_format_commits_a_readable_archive() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    let source = root.path().join("source.txt");
    let mode_reference = root.path().join("mode-reference");
    fs::create_dir(&destination)?;
    fs::write(&source, b"contents")?;
    fs::File::create(&mode_reference)?;
    let expected_mode = fs::metadata(&mode_reference)?.permissions().mode() & 0o777;

    for format in [
        ArchiveFormat::Zip,
        ArchiveFormat::SevenZ,
        ArchiveFormat::TarGz,
        ArchiveFormat::Tar,
    ] {
        let base = format!("archive-{}", format.extension().replace('.', "-"));
        let events = run_compression(CompressRequest {
            id: OperationRequestId(1),
            entries: vec![test_file_entry(&source)],
            destination: Location::local(&destination),
            archive_name: base.clone(),
            conflict: TransferConflict::FailIfExists,
            format,
            password: None,
        });
        assert!(
            events
                .iter()
                .any(|event| matches!(event, OperationEvent::Compressed { .. }))
        );
        let archive = destination.join(format!("{base}.{}", format.extension()));
        let extracted = destination.join(format!("extracted-{base}"));
        fs::create_dir(&extracted)?;
        match format {
            ArchiveFormat::Zip => {
                extract_zip(&archive, &extracted)?;
            }
            ArchiveFormat::SevenZ => {
                extract_7z_from_reader(
                    fs::File::open(&archive)?,
                    &extracted,
                    "archive",
                    sevenz_rust2::Password::empty(),
                    &Arc::new(AtomicUsize::new(0)),
                    &never_cancelled(),
                    &mut TransferReport::default(),
                )?;
            }
            ArchiveFormat::TarGz => {
                extract_tar(
                    &archive,
                    &extracted,
                    "archive",
                    true,
                    &Arc::new(AtomicUsize::new(0)),
                    &never_cancelled(),
                    &mut TransferReport::default(),
                )?;
            }
            ArchiveFormat::Tar => {
                extract_tar(
                    &archive,
                    &extracted,
                    "archive",
                    false,
                    &Arc::new(AtomicUsize::new(0)),
                    &never_cancelled(),
                    &mut TransferReport::default(),
                )?;
            }
            ArchiveFormat::Rar => unreachable!("RAR compression is not supported"),
        }
        assert_eq!(fs::read(extracted.join("source.txt"))?, b"contents");
        assert_eq!(
            fs::metadata(&archive)?.permissions().mode() & 0o777,
            expected_mode
        );
    }
    assert!(stages(&destination, COMPRESSION_STAGE)?.is_empty());
    Ok(())
}

#[test]
fn strata_archives_round_trip_links_modes_and_times() -> Result<(), Box<dyn Error>> {
    const SOURCE_TIME: i64 = 1_000_000_000;
    for format in [
        ArchiveFormat::Zip,
        ArchiveFormat::Tar,
        ArchiveFormat::TarGz,
        ArchiveFormat::SevenZ,
    ] {
        let context = format!("{format:?}");
        let root = tempfile::tempdir()?;
        let source = root.path().join("source");
        fs::create_dir_all(source.join("inner"))?;
        fs::write(source.join("run.sh"), b"#!/bin/sh\necho ok\n")?;
        fs::write(source.join("secret.txt"), b"secret")?;
        fs::write(source.join("inner/note.txt"), b"note")?;
        let with_link = format != ArchiveFormat::SevenZ;
        if with_link {
            std::os::unix::fs::symlink("run.sh", source.join("link"))?;
            set_times_without_following(&source.join("link"), SOURCE_TIME)?;
        }
        let modes = [
            ("run.sh", 0o755),
            ("secret.txt", 0o600),
            ("inner/note.txt", 0o644),
            ("inner", 0o700),
            ("", 0o750),
        ];
        for (name, mode) in modes {
            let path = source.join(name);
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
            set_times_without_following(&path, SOURCE_TIME)?;
        }
        let archives = root.path().join("archives");
        fs::create_dir(&archives)?;
        let events = run_compression(CompressRequest {
            id: OperationRequestId(1),
            entries: vec![test_file_entry(&source)],
            destination: Location::local(&archives),
            archive_name: "source".to_owned(),
            conflict: TransferConflict::FailIfExists,
            format,
            password: None,
        });
        assert!(
            events
                .iter()
                .any(|event| matches!(event, OperationEvent::Compressed { .. })),
            "{context}: {events:?}"
        );
        let extracted = root.path().join("extracted");
        fs::create_dir(&extracted)?;

        let events = run_extraction(ExtractRequest {
            id: OperationRequestId(2),
            entry: test_file_entry(&archives.join(format!("source.{}", format.extension()))),
            destination: Location::local(&extracted),
            created_destination: false,
            password: None,
        });

        assert!(
            matches!(
                events.last(),
                Some(OperationEvent::Extracted { first_name: Some(name), .. }) if name == "source"
            ),
            "{context}: {events:?}"
        );
        let output = extracted.join("source");
        if with_link {
            let link = output.join("link");
            let metadata = fs::symlink_metadata(&link)?;
            assert!(
                metadata.file_type().is_symlink(),
                "{context}: `source/link` extracted as a {} of {} bytes, not as a symlink",
                if metadata.is_dir() {
                    "directory"
                } else {
                    "regular file"
                },
                metadata.len()
            );
            assert_eq!(fs::read_link(&link)?, Path::new("run.sh"), "{context}");
            assert_eq!(metadata.mtime(), SOURCE_TIME, "{context}: link mtime");
        }
        for (name, mode) in modes {
            let metadata = fs::metadata(output.join(name))?;
            assert_eq!(
                format!("{:o}", metadata.permissions().mode() & 0o7777),
                format!("{:o}", expected_mode(mode)),
                "{context}: mode of `source/{name}`"
            );
            assert_eq!(
                metadata.mtime(),
                SOURCE_TIME,
                "{context}: mtime of `source/{name}`"
            );
        }
    }
    Ok(())
}

#[test]
fn compression_reports_unsupported_7z_links_without_committing() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    fs::create_dir(&source)?;
    fs::write(source.join("file.txt"), b"contents")?;
    let link = source.join("link");
    std::os::unix::fs::symlink("file.txt", &link)?;
    for entry in [&source, &link] {
        for conflict in [
            TransferConflict::FailIfExists,
            TransferConflict::ReplaceExisting,
        ] {
            let destination = tempfile::tempdir()?;
            let archive = destination.path().join("archive.7z");
            if conflict == TransferConflict::ReplaceExisting {
                fs::write(&archive, b"original archive")?;
            }
            let events = run_compression(CompressRequest {
                id: OperationRequestId(1),
                entries: vec![test_file_entry(entry)],
                destination: Location::local(destination.path()),
                archive_name: "archive".to_owned(),
                conflict,
                format: ArchiveFormat::SevenZ,
                password: None,
            });
            assert!(events.iter().any(|event| matches!(event, OperationEvent::Failed { message, .. } if message.contains("does not support symbolic links") && message.contains("Use ZIP or TAR instead"))));
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, OperationEvent::Compressed { .. }))
            );
            if conflict == TransferConflict::ReplaceExisting {
                assert_eq!(fs::read(&archive)?, b"original archive");
            } else {
                assert!(!archive.exists());
            }
            assert!(stages(destination.path(), COMPRESSION_STAGE)?.is_empty());
        }
    }
    Ok(())
}

#[test]
fn copy_with_big_buf_stops_when_cancelled() {
    let cancelled = AtomicBool::new(true);
    let mut destination = Vec::new();
    let error = copy_with_big_buf(&b"payload"[..], &mut destination, &cancelled)
        .expect_err("cancelled copy must stop");
    assert!(matches!(error, ArchiveError::Cancelled));
    assert!(destination.is_empty());
}

#[test]
fn copy_with_big_buf_reports_os_write_failures_without_the_errno() {
    struct FullDisk;
    impl std::io::Write for FullDisk {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from_raw_os_error(libc::EFBIG))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let error = copy_with_big_buf(&b"payload"[..], &mut FullDisk, &AtomicBool::new(false))
        .expect_err("failed write must stop the copy");
    assert_eq!(
        error,
        ArchiveError::Failed("The file is too large for this file system".to_owned())
    );
}

#[test]
fn cancelling_extraction_from_started_waits_for_the_worker_and_reports_pending_output()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    fs::create_dir(&destination)?;
    let archive_path = root.path().join("content.zip");
    write_zip_stored(
        &archive_path,
        &[("first.bin", b"early"), ("second.txt", b"late")],
    )?;

    // Cancelled before the worker starts: nothing is written, every member is reported pending.
    let events = run_extraction_cancelled(ExtractRequest {
        id: OperationRequestId(11),
        entry: test_file_entry(&archive_path),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });
    let events = RefCell::new(events);
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| matches!(event, OperationEvent::Cancelled { .. })),
        "expected cancellation after the worker stopped: {:?}",
        events.borrow()
    );
    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| matches!(event, OperationEvent::Extracted { .. }))
    );
    let result = events
        .borrow()
        .iter()
        .find_map(|event| match event {
            OperationEvent::Cancelled { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("terminal cancellation result");
    assert!(
        result
            .affected_locations
            .contains(&Location::local(&destination))
    );
    assert!(result.completed.is_empty());
    assert!(result.failed.is_empty());
    assert_eq!(
        result.not_attempted,
        [
            Location::local(destination.join("first.bin")),
            Location::local(destination.join("second.txt")),
        ]
    );
    assert!(destination.read_dir()?.next().is_none());
    Ok(())
}

#[test]
fn extraction_failures_stop_progress_and_preserve_error_distinctions() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    fs::create_dir(&destination)?;
    let unmade_destination = root.path().join("unmade");
    for (name, expected) in [
        (
            "fake.zip",
            "This file is not a valid archive or is damaged.",
        ),
        ("fake.7z", "This file is not a valid archive or is damaged."),
        (
            "fake.tar",
            "This file is not a valid archive or is damaged.",
        ),
        (
            "fake.tar.gz",
            "This file is not a valid archive or is damaged.",
        ),
        ("missing.zip", "No such file"),
        ("unreadable.zip", "Permission denied"),
        ("destination.zip", "Not a folder"),
        ("unknown.iso", "Unsupported archive format"),
        ("folder.zip", "Not an archive: “folder.zip”"),
        ("passwords.zip", "Not an archive: “passwords.zip”"),
    ] {
        let archive = root.path().join(name);
        if matches!(name, "folder.zip" | "passwords.zip") {
            fs::create_dir(&archive)?;
        } else if name != "missing.zip" {
            fs::write(&archive, b"not an archive")?;
        }
        if name == "unreadable.zip" {
            fs::set_permissions(&archive, fs::Permissions::from_mode(0o000))?;
        }
        if name == "destination.zip" {
            write_zip_stored(&archive, &[("file.txt", b"contents")])?;
        }
        if name == "unreadable.zip" && rustix::process::geteuid().is_root() {
            // Root reads a file without permissions anyway.
            continue;
        }
        let events = RefCell::new(run_extraction(ExtractRequest {
            id: OperationRequestId(434),
            entry: test_file_entry(&archive),
            destination: Location::local(match name {
                "destination.zip" => &archive,
                "folder.zip" => &unmade_destination,
                _ => &destination,
            }),
            created_destination: false,
            password: None,
        }));
        assert!(
            matches!(events.borrow().last(), Some(OperationEvent::Failed { message, password_failure: None, .. }) if message.contains(expected)),
            "{name}: {:?}",
            events.borrow()
        );
        assert!(destination.read_dir()?.next().is_none());
        assert!(!unmade_destination.exists());
        assert!(!root.path().join("outside").exists());
    }
    Ok(())
}

#[test]
fn extraction_provider_sanitizes_parent_paths_without_failure() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    fs::create_dir(&destination)?;
    let archive = root.path().join("unsafe.zip");
    write_zip_stored(
        &archive,
        &[("../escaped.txt", b"escaped"), ("after.txt", b"after")],
    )?;

    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(435),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });

    assert!(events.iter().any(|event| matches!(
        event,
        OperationEvent::Extracted { first_name: Some(name), .. } if name == "unsafe"
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OperationEvent::Failed { .. }))
    );
    assert_eq!(
        fs::read(destination.join("unsafe/escaped.txt"))?,
        b"escaped"
    );
    assert_eq!(fs::read(destination.join("unsafe/after.txt"))?, b"after");
    assert!(!root.path().join("escaped.txt").exists());
    Ok(())
}

#[test]
fn failed_extraction_removes_a_newly_created_empty_destination() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let archive = root.path().join("fake.zip");
    fs::write(&archive, b"not an archive")?;
    let destination = root.path().join("leftover");
    let on_ui_thread = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = |step: &'static str| {
        let seen = on_ui_thread.clone();
        Box::new(move || {
            seen.lock()
                .expect("steps")
                .push((step, crate::util::ui_thread::is_current()));
        }) as Box<dyn FnMut() + Send>
    };
    let _scope = FsQuirks::install(
        root.path(),
        Quirks {
            before: vec![
                (Op::FolderStamp, "fake.zip", 0, seen("archive check")),
                (Op::Unlink, "leftover", 0, seen("cleanup")),
            ],
            ..Quirks::default()
        },
    );
    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(908),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });
    assert!(
        matches!(events.last(), Some(OperationEvent::Failed { .. })),
        "{:?}",
        events
    );
    assert!(
        !destination.exists(),
        "leftover destination was not cleaned up"
    );
    assert_eq!(
        *on_ui_thread.lock().expect("steps"),
        [("archive check", false), ("cleanup", false)],
        "the archive is checked and the folder removed off the UI thread"
    );
    Ok(())
}

#[test]
fn failed_extraction_preserves_a_pre_existing_destination() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let archive = root.path().join("fake.zip");
    fs::write(&archive, b"not an archive")?;
    let destination = root.path().join("existing");
    fs::create_dir(&destination)?;
    fs::write(destination.join("kept.txt"), b"kept")?;
    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(909),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });
    assert!(
        matches!(events.last(), Some(OperationEvent::Failed { .. })),
        "{:?}",
        events
    );
    assert!(destination.exists(), "pre-existing destination was removed");
    assert!(
        destination.join("kept.txt").exists(),
        "user content was lost"
    );
    assert_eq!(destination.read_dir()?.count(), 1);
    Ok(())
}

#[test]
fn failed_extraction_removes_a_caller_created_destination() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let archive = root.path().join("broken.zip");
    fs::write(&archive, b"not an archive")?;
    let destination = root.path().join("broken");
    fs::create_dir(&destination)?;
    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(910),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: true,
        password: None,
    });
    assert!(
        matches!(events.last(), Some(OperationEvent::Failed { .. })),
        "{:?}",
        events
    );
    assert!(
        !destination.exists(),
        "caller-created destination was not cleaned up"
    );
    Ok(())
}

#[test]
fn spilled_members_bundle_under_the_archive_stem() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let archives = [
        root.path().join("bundle.zip"),
        root.path().join("bundle.tar"),
        root.path().join("bundle.tar.gz"),
        root.path().join("bundle.7z"),
    ];
    write_7z_entries(&archives[3], &[("a.txt", b"a"), ("dir/b.txt", b"b")])?;
    write_zip_stored(&archives[0], &[("a.txt", b"a"), ("dir/b.txt", b"b")])?;
    for (archive, gzip) in [(&archives[1], false), (&archives[2], true)] {
        write_tar_entries(
            archive,
            &[
                (tar::EntryType::Regular, "a.txt", b"a"),
                (tar::EntryType::Regular, "dir/b.txt", b"b"),
            ],
            gzip,
        )?;
    }

    for archive in &archives {
        let destination = tempfile::tempdir()?;
        fs::write(destination.path().join("a.txt"), b"EXISTING")?;
        fs::create_dir(destination.path().join("dir"))?;
        fs::write(destination.path().join("dir/keep.txt"), b"keep")?;
        let events = run_extraction(ExtractRequest {
            id: OperationRequestId(1),
            entry: test_file_entry(archive),
            destination: Location::local(destination.path()),
            created_destination: false,
            password: None,
        });

        assert!(
            matches!(
                events.last(),
                Some(OperationEvent::Extracted {
                    first_name: Some(name),
                    ..
                }) if name == "bundle"
            ),
            "{archive:?}: {:?}",
            events
        );
        assert_eq!(
            entry_names(&destination.path().join("bundle"))?,
            ["a.txt", "dir"],
            "{archive:?}"
        );
        assert_eq!(
            fs::read(destination.path().join("bundle/a.txt"))?,
            b"a",
            "{archive:?}"
        );
        assert_eq!(
            fs::read(destination.path().join("bundle/dir/b.txt"))?,
            b"b",
            "{archive:?}"
        );
        assert_eq!(
            fs::read(destination.path().join("a.txt"))?,
            b"EXISTING",
            "{archive:?}"
        );
        assert_eq!(
            entry_names(&destination.path().join("dir"))?,
            ["keep.txt"],
            "{archive:?}"
        );
        assert_eq!(
            entry_names(destination.path())?,
            ["a.txt", "bundle", "dir"],
            "{archive:?}"
        );
    }
    Ok(())
}

#[test]
fn failed_multi_root_extraction_keeps_partial_output_in_the_archive_folder()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let cases = [
        ("abs.zip", "abs", "/etc/evil.txt"),
        ("drive.zip", "drive", "C:/evil.txt"),
        ("abs.tar", "abs", "/etc/evil.txt"),
        ("abs.tar.gz", "abs", "/etc/evil.txt"),
    ];
    for (name, stem, unsafe_member) in cases {
        let archive = root.path().join(name);
        let members: [(&str, &[u8]); 3] = [
            ("ok.txt", b"ok"),
            ("second.txt", b"2"),
            (unsafe_member, b"evil"),
        ];
        if name.ends_with(".zip") {
            write_zip_stored(&archive, &members)?;
        } else {
            let entries =
                members.map(|(member, contents)| (tar::EntryType::Regular, member, contents));
            write_tar_entries(&archive, &entries, name.ends_with(".gz"))?;
        }
        let destination = tempfile::tempdir()?;
        fs::write(destination.path().join("keep.txt"), b"keep")?;

        let events = run_extraction(ExtractRequest {
            id: OperationRequestId(6),
            entry: test_file_entry(&archive),
            destination: Location::local(destination.path()),
            created_destination: false,
            password: None,
        });

        let Some(OperationEvent::Failed { message, .. }) = events.last() else {
            panic!("{name}: expected a failure, got {events:?}");
        };
        let mut expected = [stem, "keep.txt"];
        expected.sort_unstable();
        assert_eq!(
            entry_names(destination.path())?,
            expected,
            "{name}: {message}"
        );
        assert_eq!(
            *message,
            format!(
                "Refusing unsafe archive path: {unsafe_member}. Extracted entries remain in “{stem}”."
            ),
            "{name}"
        );
        assert!(
            stages(destination.path(), EXTRACTION_STAGE)?.is_empty(),
            "{name}"
        );
        assert_eq!(
            fs::read(destination.path().join(stem).join("ok.txt"))?,
            b"ok"
        );
        assert_eq!(
            fs::read(destination.path().join(stem).join("second.txt"))?,
            b"2"
        );
        assert_eq!(fs::read(destination.path().join("keep.txt"))?, b"keep");
    }
    Ok(())
}

#[test]
fn single_root_extraction_lands_verbatim() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    fs::create_dir(&destination)?;

    let archive = root.path().join("note.zip");
    write_zip_stored(&archive, &[("readme.txt", b"hi")])?;
    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(2),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });
    assert!(matches!(
        events.last(),
        Some(OperationEvent::Extracted {
            first_name: Some(name),
            ..
        }) if name == "readme.txt"
    ));
    assert_eq!(fs::read(destination.join("readme.txt"))?, b"hi");
    assert!(!destination.join("note").exists());

    let archive = root.path().join("foldered.zip");
    write_zip_stored(&archive, &[("folder/a.txt", b"a"), ("folder/b.txt", b"b")])?;
    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(3),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });
    assert!(matches!(
        events.last(),
        Some(OperationEvent::Extracted {
            first_name: Some(name),
            ..
        }) if name == "folder"
    ));
    assert_eq!(fs::read(destination.join("folder/a.txt"))?, b"a");
    assert!(!destination.join("foldered").exists());

    let archive = root.path().join("note.zip");
    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(4),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });
    assert!(matches!(
        events.last(),
        Some(OperationEvent::Extracted {
            first_name: Some(name),
            ..
        }) if name == "readme (2).txt"
    ));
    assert_eq!(fs::read(destination.join("readme.txt"))?, b"hi");
    assert_eq!(fs::read(destination.join("readme (2).txt"))?, b"hi");
    assert!(!destination.join("note").exists());
    assert_eq!(destination.read_dir()?.count(), 3);
    Ok(())
}

#[test]
fn extraction_reports_published_locations_for_undo() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    fs::create_dir(&destination)?;
    fs::write(destination.join("readme.txt"), b"original")?;

    let archive = root.path().join("note.zip");
    write_zip_stored(&archive, &[("readme.txt", b"extracted")])?;
    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(7),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });

    let Some(OperationEvent::Extracted { created, .. }) = events.last() else {
        panic!("expected extraction, got {events:?}");
    };
    assert_eq!(
        created.as_slice(),
        &[Location::local(destination.join("readme (2).txt"))],
        "undo must target the published rename, not the pre-existing file"
    );
    assert_eq!(fs::read(destination.join("readme.txt"))?, b"original");
    Ok(())
}

#[test]
fn a_bundle_may_share_its_name_with_an_extracted_root() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    fs::create_dir(&destination)?;
    let archive = root.path().join("photos.zip");
    write_zip_stored(
        &archive,
        &[("photos/inner.txt", b"inner"), ("extra.txt", b"extra")],
    )?;

    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(4),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });

    assert!(matches!(
        events.last(),
        Some(OperationEvent::Extracted {
            first_name: Some(name),
            ..
        }) if name == "photos"
    ));
    assert_eq!(
        fs::read(destination.join("photos/photos/inner.txt"))?,
        b"inner"
    );
    assert_eq!(fs::read(destination.join("photos/extra.txt"))?, b"extra");
    assert_eq!(destination.read_dir()?.count(), 1);
    Ok(())
}

#[test]
fn bundled_extraction_reserves_a_fresh_name_against_existing_entries() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let destination = root.path().join("destination");
    fs::create_dir(&destination)?;
    fs::write(destination.join("bundle"), b"existing file")?;
    std::os::unix::fs::symlink("missing", destination.join("bundle (1)"))?;
    let archive = root.path().join("bundle.zip");
    write_zip_stored(&archive, &[("a.txt", b"a"), ("b.txt", b"b")])?;

    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(5),
        entry: test_file_entry(&archive),
        destination: Location::local(&destination),
        created_destination: false,
        password: None,
    });

    assert!(matches!(
        events.last(),
        Some(OperationEvent::Extracted {
            first_name: Some(name),
            ..
        }) if name == "bundle (2)"
    ));
    assert_eq!(fs::read(destination.join("bundle"))?, b"existing file");
    assert_eq!(fs::read(destination.join("bundle (2)/a.txt"))?, b"a");
    assert_eq!(
        fs::read_link(destination.join("bundle (1)"))?,
        Path::new("missing")
    );
    Ok(())
}

#[test]
fn gzip_trailer_failure_is_reported_through_the_provider() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let archive = root.path().join("content.tar.gz");
    let large = vec![b'x'; 50_000];
    write_tar_entries(
        &archive,
        &[
            (tar::EntryType::Regular, "a.txt", &large),
            (tar::EntryType::Regular, "b.txt", b"b"),
        ],
        true,
    )?;
    corrupt_gzip_trailer(&archive, 8)?;
    let destination = tempfile::tempdir()?;

    let events = run_extraction(ExtractRequest {
        id: OperationRequestId(7),
        entry: test_file_entry(&archive),
        destination: Location::local(destination.path()),
        created_destination: false,
        password: None,
    });

    let Some(OperationEvent::Failed {
        message,
        password_failure: None,
        ..
    }) = events.last()
    else {
        panic!("a damaged gzip trailer was accepted: {events:?}");
    };
    assert_eq!(
        *message,
        format!("{INVALID_ARCHIVE} Extracted entries remain in “content”.")
    );
    assert_eq!(entry_names(destination.path())?, ["content"]);
    assert_eq!(
        fs::metadata(destination.path().join("content/a.txt"))?.len(),
        50_000
    );
    Ok(())
}

#[test]
fn password_failures_reach_the_event_as_their_kind_and_leave_nothing_behind()
-> Result<(), Box<dyn Error>> {
    for format in [ArchiveFormat::Zip, ArchiveFormat::SevenZ] {
        let root = tempfile::tempdir()?;
        let sources = ["first.txt", "second.txt"].map(|name| root.path().join(name));
        for source in &sources {
            fs::write(source, b"contents")?;
        }
        let archive = root.path().join(format!("secret.{}", format.extension()));
        super::write_compression_fixture(&archive, &sources, format, Some("test-password"))?;
        for (password, expected) in [
            (None, PasswordFailure::Required),
            (Some("wrong-password"), PasswordFailure::Incorrect),
        ] {
            let destination = tempfile::tempdir()?;

            let events = run_extraction(ExtractRequest {
                id: OperationRequestId(8),
                entry: test_file_entry(&archive),
                destination: Location::local(destination.path()),
                created_destination: false,
                password: password.map(str::to_owned),
            });

            assert!(
                matches!(
                    events.last(),
                    Some(OperationEvent::Failed { message, password_failure: Some(kind), .. })
                        if *kind == expected && !message.contains("remain")
                ),
                "{format:?} {password:?}: {events:?}"
            );
            assert!(
                entry_names(destination.path())?.is_empty(),
                "{format:?} {password:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn an_extraction_reports_what_it_renamed_and_left_out_before_it_ends() -> Result<(), Box<dyn Error>>
{
    for (format, name, damaged, output) in [
        (ArchiveFormat::TarGz, "venv.tar.gz", false, ""),
        (ArchiveFormat::TarGz, "venv.tar.gz", true, "venv"),
        (ArchiveFormat::Zip, "venv.zip", false, ""),
        (ArchiveFormat::SevenZ, "venv.7z", false, ""),
    ] {
        let root = tempfile::tempdir()?;
        let archive = root.path().join(name);
        write_members(
            &archive,
            format,
            &[
                FixtureMember::Directory {
                    name: "10:00",
                    mode: None,
                    modified: None,
                },
                FixtureMember::File {
                    name: "10:00/a.txt",
                    contents: b"a",
                    mode: None,
                    modified: None,
                },
                FixtureMember::Symlink {
                    name: "10:00/lnk",
                    target: b"a.txt",
                },
            ],
        )?;
        if damaged {
            corrupt_gzip_trailer(&archive, 8)?;
        }
        let stick = root.path().join("stick");
        fs::create_dir(&stick)?;
        let _quirks = FsQuirks::install(&stick, Quirks::vfat());

        let events = run_extraction(ExtractRequest {
            id: OperationRequestId(9),
            entry: test_file_entry(&archive),
            destination: Location::local(&stick),
            created_destination: false,
            password: None,
        });

        let [.., OperationEvent::ItemsReported { report, .. }, ending] = events.as_slice() else {
            panic!("no report came before the end: {events:?}");
        };
        assert_eq!(
            matches!(ending, OperationEvent::Failed { .. }),
            damaged,
            "{name}: {ending:?}"
        );
        let output = stick.join(output);
        assert_eq!(
            report.renamed,
            [RenamedItem {
                requested: Location::local(output.join("10:00")),
                saved_as: Location::local(output.join("10_00")),
                cause: RenameCause::Invalid,
            }],
            "{name}"
        );
        assert_eq!(
            report.skipped,
            [SkippedItem {
                location: Location::local(output.join("10_00/lnk")),
                kind: SpecialKind::Symlink,
            }],
            "{name}"
        );
        assert_eq!(fs::read(output.join("10_00/a.txt"))?, b"a", "{name}");
    }
    Ok(())
}
