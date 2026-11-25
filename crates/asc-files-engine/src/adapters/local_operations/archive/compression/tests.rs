// SPDX-License-Identifier: MIT
//! Legacy's compression tests. The encoding tests are as they were; publication is the engine's
//! own placement now (see `super::super::compress`), so its tests are written against `compress`
//! for what the user sees: a private stage, numbering, replacing with and without a Trash, and
//! nothing published after a cancel or a failed close.

mod cancellation;
mod policy;

use super::super::destination::process_umask;
use super::super::fixtures::{
    COMPRESSION_STAGE, HomeTrashGuard, compression_stage_mode, names, never_cancelled,
    set_times_without_following, stages, tempdir_on_home_device, write_compression_fixture,
};
use super::super::{ArchiveError, Compressed, Counters, compress};
use super::inspect_archive_sources;
use crate::{
    adapters::local_operations::tests::fs_quirks::{FsQuirks, Op, Quirks, Trash},
    services::{ArchiveFormat, TransferConflict},
};
use policy::assert_seven_z_methods;
use rustix::io::Errno;
use std::{
    collections::BTreeMap,
    error::Error,
    ffi::OsString,
    fmt::Debug,
    fs,
    io::{Read, Write},
    os::unix::{ffi::OsStringExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Local time, as legacy read it from glib's `DateTime`.
struct LocalTime(libc::tm);

impl LocalTime {
    fn from_unix_local(seconds: i64) -> Result<Self, String> {
        super::super::local_time(seconds)
            .map(Self)
            .ok_or_else(|| format!("no local time for {seconds}"))
    }
    fn year(&self) -> i32 {
        self.0.tm_year + 1900
    }
    fn month(&self) -> i32 {
        self.0.tm_mon + 1
    }
    fn day_of_month(&self) -> i32 {
        self.0.tm_mday
    }
    fn hour(&self) -> i32 {
        self.0.tm_hour
    }
    fn minute(&self) -> i32 {
        self.0.tm_min
    }
    fn second(&self) -> i32 {
        self.0.tm_sec
    }
}

/// `compress` of `source` into `into` as `archive.zip`.
fn compress_into(
    source: &Path,
    into: &Path,
    conflict: TransferConflict,
    cancelled: &Arc<AtomicBool>,
) -> Result<Compressed, ArchiveError> {
    compress(
        &[source.to_path_buf()],
        into,
        "archive",
        ArchiveFormat::Zip,
        None,
        conflict,
        &Counters::default(),
        cancelled,
    )
}

fn is_zip(path: &Path) -> bool {
    fs::File::open(path)
        .ok()
        .and_then(|file| zip::ZipArchive::new(file).ok())
        .is_some()
}

#[test]
fn compression_staging_stays_private_until_publish() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.txt");
    fs::write(&source, b"source")?;
    let into = root.path().join("into");
    fs::create_dir(&into)?;
    let written = Arc::new(Mutex::new(None));
    let seen = Arc::new(Mutex::new(None));
    let (folder, record) = (into.clone(), seen.clone());
    let (written_in, written_record) = (into.clone(), written.clone());
    let _scope = FsQuirks::install(
        &into,
        Quirks {
            before: vec![
                (
                    Op::SyncData,
                    ".asc-files-copy-*",
                    0,
                    Box::new(move || {
                        *written_record.lock().unwrap() = Some(
                            compression_stage_mode(&written_in).map_err(|error| error.to_string()),
                        )
                    }),
                ),
                (
                    Op::Close,
                    ".asc-files-copy-*",
                    0,
                    Box::new(move || {
                        *record.lock().unwrap() =
                            Some(compression_stage_mode(&folder).map_err(|error| error.to_string()))
                    }),
                ),
            ],
            ..Quirks::default()
        },
    );
    let compressed = compress_into(
        &source,
        &into,
        TransferConflict::FailIfExists,
        &never_cancelled(),
    )?;
    assert_eq!(
        written
            .lock()
            .unwrap()
            .take()
            .ok_or("the stage was never synced")??,
        0o600
    );
    // The stage is owner-only while it is written and given its final mode just before it closes.
    let mode = seen
        .lock()
        .unwrap()
        .take()
        .ok_or("the stage was never closed")??;
    assert_eq!(mode, 0o666 & !process_umask());
    assert_eq!(compressed.archive, into.join("archive.zip"));
    assert_eq!(
        fs::metadata(&compressed.archive)?.permissions().mode() & 0o777,
        0o666 & !process_umask()
    );
    assert!(is_zip(&compressed.archive));
    assert!(stages(&into, COMPRESSION_STAGE)?.is_empty());
    Ok(())
}

#[test]
fn a_taken_name_is_refused_kept_or_numbered_as_asked() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.txt");
    fs::write(&source, b"source")?;
    // Keep Both takes the name itself while it is free.
    let free = tempfile::tempdir()?;
    let kept = compress_into(
        &source,
        free.path(),
        TransferConflict::KeepBoth,
        &never_cancelled(),
    )?;
    assert_eq!(kept.archive, free.path().join("archive.zip"));
    let archive = root.path().join("archive.zip");
    fs::write(&archive, b"original")?;
    assert!(
        compress_into(
            &source,
            root.path(),
            TransferConflict::FailIfExists,
            &never_cancelled()
        )
        .is_err()
    );
    assert_eq!(fs::read(&archive)?, b"original");
    assert!(stages(root.path(), COMPRESSION_STAGE)?.is_empty());
    let numbered = compress_into(
        &source,
        root.path(),
        TransferConflict::KeepBoth,
        &never_cancelled(),
    )?;
    assert_ne!(numbered.archive, archive);
    assert!(
        numbered
            .archive
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("archive") && name.ends_with(".zip"))
    );
    assert!(is_zip(&numbered.archive));
    assert_eq!(fs::read(&archive)?, b"original");
    assert!(
        compress_into(
            &source,
            root.path(),
            TransferConflict::Merge,
            &never_cancelled()
        )
        .is_err()
    );
    assert!(stages(root.path(), COMPRESSION_STAGE)?.is_empty());
    Ok(())
}

#[test]
fn a_replaced_archive_goes_to_the_trash_and_keeps_its_mode() -> Result<(), Box<dyn Error>> {
    let root = tempdir_on_home_device()?;
    let source = root.path().join("source.txt");
    fs::write(&source, b"source")?;
    let archive = root.path().join("archive.zip");
    fs::write(&archive, b"original")?;
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o640))?;
    let _trash = HomeTrashGuard::new(&archive);
    let compressed = compress_into(
        &source,
        root.path(),
        TransferConflict::ReplaceExisting,
        &never_cancelled(),
    )?;
    assert_eq!(compressed.archive, archive);
    assert!(is_zip(&archive));
    assert_eq!(fs::metadata(&archive)?.permissions().mode() & 0o777, 0o640);
    let trashed = compressed
        .replaced
        .ok_or("the original went to the Trash")?;
    assert_eq!(fs::read(&trashed.file)?, b"original");
    assert!(!compressed.original_deleted);
    Ok(())
}

#[test]
fn an_archive_replaced_without_trash_deletes_its_original() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.txt");
    fs::write(&source, b"source")?;
    let archive = root.path().join("archive.zip");
    fs::write(&archive, b"original")?;
    let _scope = FsQuirks::install(
        root.path(),
        Quirks {
            trash: Trash::Unsupported,
            ..Quirks::default()
        },
    );
    let compressed = compress_into(
        &source,
        root.path(),
        TransferConflict::ReplaceExisting,
        &never_cancelled(),
    )?;
    assert!(is_zip(&archive));
    assert!(compressed.replaced.is_none());
    assert!(compressed.original_deleted);
    assert_eq!(names(root.path())?, ["archive.zip", "source.txt"]);
    Ok(())
}

#[test]
fn compression_replacement_preserves_directory_destinations() -> Result<(), Box<dyn Error>> {
    for populated in [false, true] {
        let root = tempfile::tempdir()?;
        let source = root.path().join("source.txt");
        fs::write(&source, b"source")?;
        let archive = root.path().join("archive.zip");
        fs::create_dir(&archive)?;
        if populated {
            fs::write(archive.join("contents"), b"original")?;
        }
        let result = compress_into(
            &source,
            root.path(),
            TransferConflict::ReplaceExisting,
            &never_cancelled(),
        );
        assert!(matches!(result, Err(ArchiveError::Failed(_))), "{result:?}");
        assert!(archive.is_dir());
        if populated {
            assert_eq!(fs::read(archive.join("contents"))?, b"original");
        }
        assert!(stages(root.path(), COMPRESSION_STAGE)?.is_empty());
    }
    Ok(())
}

/// Legacy refused before encoding; the engine's placement refuses at publication, which
/// leaves the same: the original, and no stage.
#[test]
fn replacement_fails_when_no_strategy_is_safe() -> Result<(), Box<dyn Error>> {
    const NFS_SUPER_MAGIC: rustix::fs::FsWord = 0x6969;
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.txt");
    fs::write(&source, b"source")?;
    let archive = root.path().join("archive.zip");
    fs::write(&archive, b"original")?;
    let _quirks = FsQuirks::install(
        root.path(),
        Quirks {
            trash: Trash::Unsupported,
            fs_type: Some(NFS_SUPER_MAGIC),
            ..Quirks::without_placement()
        },
    );
    let result = compress_into(
        &source,
        root.path(),
        TransferConflict::ReplaceExisting,
        &never_cancelled(),
    );
    assert!(matches!(result, Err(ArchiveError::Failed(_))), "{result:?}");
    assert_eq!(fs::read(&archive)?, b"original");
    assert_eq!(names(root.path())?, ["archive.zip", "source.txt"]);
    Ok(())
}

#[test]
fn staged_archives_are_not_published_after_cancellation_or_close_failure()
-> Result<(), Box<dyn Error>> {
    for replacing in [false, true] {
        for boundary in ["encoding", "close", "close error"] {
            let root = tempfile::tempdir()?;
            let source = root.path().join("source.txt");
            fs::write(&source, b"source")?;
            let archive = root.path().join("archive.zip");
            if replacing {
                fs::write(&archive, b"original")?;
            }
            let cancelled = Arc::new(AtomicBool::new(false));
            let on_hook = cancelled.clone();
            let mut quirks = Quirks {
                trash: Trash::Unsupported,
                ..Quirks::default()
            };
            match boundary {
                "encoding" => quirks.before.push((
                    Op::SyncData,
                    ".asc-files-copy-*",
                    0,
                    Box::new(move || on_hook.store(true, Ordering::Release)),
                )),
                "close" => quirks.after.push((
                    Op::Close,
                    ".asc-files-copy-*",
                    0,
                    Box::new(move || on_hook.store(true, Ordering::Release)),
                )),
                _ => quirks
                    .fail
                    .push((Op::Close, ".asc-files-copy-*", 0, Errno::IO)),
            }
            let _scope = FsQuirks::install(root.path(), quirks);
            let conflict = if replacing {
                TransferConflict::ReplaceExisting
            } else {
                TransferConflict::FailIfExists
            };
            let result = compress_into(&source, root.path(), conflict, &cancelled);
            if boundary == "close error" {
                assert!(
                    matches!(result, Err(ArchiveError::Failed(_))),
                    "{boundary}, replacing {replacing}: {result:?}"
                );
            } else {
                assert!(
                    matches!(result, Err(ArchiveError::Cancelled)),
                    "{boundary}, replacing {replacing}: {result:?}"
                );
            }
            if replacing {
                assert_eq!(fs::read(&archive)?, b"original");
            } else {
                assert!(!archive.exists());
            }
            assert!(
                stages(root.path(), COMPRESSION_STAGE)?.is_empty(),
                "{boundary}, replacing {replacing}"
            );
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum CompressedEntry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

fn read_compressed_entries(
    path: &Path,
    format: ArchiveFormat,
    password: Option<&str>,
) -> Result<BTreeMap<PathBuf, CompressedEntry>, Box<dyn Error>> {
    let file = fs::File::open(path)?;
    let mut result = BTreeMap::new();
    match format {
        ArchiveFormat::Zip => {
            let mut archive = zip::ZipArchive::new(file)?;
            for index in 0..archive.len() {
                let options =
                    zip::read::ZipReadOptions::new().password(password.map(str::as_bytes));
                let mut entry = archive.by_index_with_options(index, options)?;
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                let value = if entry.is_dir() {
                    CompressedEntry::Directory
                } else if entry.is_symlink() {
                    CompressedEntry::Symlink(PathBuf::from(OsString::from_vec(bytes)))
                } else {
                    CompressedEntry::File(bytes)
                };
                assert!(
                    result
                        .insert(PathBuf::from(&*entry.name().expect("member name")), value)
                        .is_none()
                );
            }
        }
        ArchiveFormat::Tar | ArchiveFormat::TarGz => {
            let reader: Box<dyn Read> = if format == ArchiveFormat::TarGz {
                Box::new(flate2::read::GzDecoder::new(file))
            } else {
                Box::new(file)
            };
            for entry in tar::Archive::new(reader).entries()? {
                let mut entry = entry?;
                let value = if entry.header().entry_type().is_dir() {
                    CompressedEntry::Directory
                } else if entry.header().entry_type().is_symlink() {
                    CompressedEntry::Symlink(
                        entry
                            .link_name()?
                            .ok_or("Missing link target")?
                            .into_owned(),
                    )
                } else {
                    assert!(entry.header().entry_type().is_file());
                    let mut bytes = Vec::new();
                    entry.read_to_end(&mut bytes)?;
                    CompressedEntry::File(bytes)
                };
                assert!(result.insert(entry.path()?.into_owned(), value).is_none());
            }
        }
        ArchiveFormat::SevenZ => {
            let mut archive = sevenz_rust2::ArchiveReader::new(
                file,
                password
                    .map(sevenz_rust2::Password::from)
                    .unwrap_or_default(),
            )?;
            archive.for_each_entries(|entry, reader| {
                let value = if entry.is_directory() {
                    CompressedEntry::Directory
                } else {
                    let mut bytes = Vec::new();
                    reader.read_to_end(&mut bytes)?;
                    CompressedEntry::File(bytes)
                };
                assert!(result.insert(PathBuf::from(entry.name()), value).is_none());
                Ok(true)
            })?;
        }
        ArchiveFormat::Rar => return Err("RAR compression is not supported".into()),
    }
    Ok(result)
}

#[test]
fn compression_preserves_links_in_zip_and_tar() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    fs::create_dir_all(source.join("nested"))?;
    fs::create_dir(source.join("empty"))?;
    fs::write(source.join("file.txt"), b"file contents")?;
    fs::write(source.join("nested/child.txt"), b"child contents")?;
    let mut expected = BTreeMap::from([
        (PathBuf::from("source"), CompressedEntry::Directory),
        (PathBuf::from("source/nested"), CompressedEntry::Directory),
        (PathBuf::from("source/empty"), CompressedEntry::Directory),
        (
            PathBuf::from("source/file.txt"),
            CompressedEntry::File(b"file contents".to_vec()),
        ),
        (
            PathBuf::from("source/nested/child.txt"),
            CompressedEntry::File(b"child contents".to_vec()),
        ),
    ]);
    for (name, target) in [
        ("file-link", "file.txt"),
        ("directory-link", "nested"),
        ("broken-link", "missing.txt"),
        ("current-directory-link", "."),
    ] {
        std::os::unix::fs::symlink(target, source.join(name))?;
        expected.insert(
            PathBuf::from("source").join(name),
            CompressedEntry::Symlink(PathBuf::from(target)),
        );
    }
    let selected_link = root.path().join("selected-link");
    std::os::unix::fs::symlink("source/nested", &selected_link)?;
    expected.insert(
        PathBuf::from("selected-link"),
        CompressedEntry::Symlink(PathBuf::from("source/nested")),
    );
    let entries = [source, selected_link];
    assert_eq!(
        inspect_archive_sources(&entries, &never_cancelled())?.files,
        7
    );
    for (format, password) in [
        (ArchiveFormat::Zip, None),
        (ArchiveFormat::Zip, Some("test-password")),
        (ArchiveFormat::Tar, None),
        (ArchiveFormat::TarGz, None),
    ] {
        let archive = root.path().join("archive");
        assert_eq!(
            write_compression_fixture(&archive, &entries, format, password)?,
            7
        );
        assert_eq!(
            read_compressed_entries(&archive, format, password)?,
            expected,
            "{format:?}"
        );
    }
    Ok(())
}

#[test]
fn seven_z_compression_preserves_mixed_methods_and_empty_directories() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    fs::create_dir_all(source.join("empty"))?;
    fs::write(source.join("one.txt"), b"one")?;
    fs::write(source.join("two.png"), b"two")?;
    let expected = BTreeMap::from([
        (PathBuf::from("source"), CompressedEntry::Directory),
        (PathBuf::from("source/empty"), CompressedEntry::Directory),
        (
            PathBuf::from("source/one.txt"),
            CompressedEntry::File(b"one".to_vec()),
        ),
        (
            PathBuf::from("source/two.png"),
            CompressedEntry::File(b"two".to_vec()),
        ),
    ]);
    for password in [None, Some("test-password")] {
        let archive = root.path().join("archive");
        assert_eq!(
            write_compression_fixture(
                &archive,
                std::slice::from_ref(&source),
                ArchiveFormat::SevenZ,
                password
            )?,
            2
        );
        assert_eq!(
            read_compressed_entries(&archive, ArchiveFormat::SevenZ, password)?,
            expected,
        );
        assert_seven_z_methods(&archive, password, "source/one.txt", false)?;
        assert_seven_z_methods(&archive, password, "source/two.png", true)?;
        if password.is_some() {
            for wrong_password in [None, Some("wrong-password")] {
                assert!(
                    read_compressed_entries(&archive, ArchiveFormat::SevenZ, wrong_password)
                        .is_err()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn compression_handles_non_utf8_link_targets_without_loss() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let link = root.path().join("link");
    let target = PathBuf::from(OsString::from_vec(b"target-\xff".to_vec()));
    std::os::unix::fs::symlink(&target, &link)?;
    for format in [ArchiveFormat::Tar, ArchiveFormat::TarGz] {
        let archive = root.path().join("archive");
        write_compression_fixture(&archive, std::slice::from_ref(&link), format, None)?;
        assert_eq!(
            read_compressed_entries(&archive, format, None)?,
            BTreeMap::from([(
                PathBuf::from("link"),
                CompressedEntry::Symlink(target.clone())
            )])
        );
    }
    let error = write_compression_fixture(
        &root.path().join("archive.zip"),
        &[link],
        ArchiveFormat::Zip,
        None,
    )
    .expect_err("ZIP must reject a link target it cannot encode");
    assert!(error.contains("non-UTF-8 link target"));
    Ok(())
}

#[test]
fn compression_accepts_a_symlink_in_the_parent_path() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let actual = root.path().join("actual");
    let alias = root.path().join("alias");
    fs::create_dir_all(actual.join("tree"))?;
    fs::write(actual.join("tree/file.txt"), b"contents")?;
    std::os::unix::fs::symlink("actual", &alias)?;
    let entries = [alias.join("tree")];
    let expected = BTreeMap::from([
        (PathBuf::from("tree"), CompressedEntry::Directory),
        (
            PathBuf::from("tree/file.txt"),
            CompressedEntry::File(b"contents".to_vec()),
        ),
    ]);

    for format in [
        ArchiveFormat::Zip,
        ArchiveFormat::Tar,
        ArchiveFormat::TarGz,
        ArchiveFormat::SevenZ,
    ] {
        let archive = root.path().join("archive");
        assert_eq!(
            inspect_archive_sources(&entries, &never_cancelled())?.files,
            1
        );
        assert_eq!(
            write_compression_fixture(&archive, &entries, format, None)?,
            1
        );
        assert_eq!(
            read_compressed_entries(&archive, format, None)?,
            expected,
            "{format:?}"
        );
    }
    Ok(())
}

#[test]
fn zip_and_seven_z_refuse_non_utf8_names_instead_of_mangling_them() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root
        .path()
        .join(OsString::from_vec(b"name-\xff.txt".to_vec()));
    fs::write(&source, b"contents")?;
    for format in [ArchiveFormat::Zip, ArchiveFormat::SevenZ] {
        let archive = root.path().join("archive.out");
        let error =
            write_compression_fixture(&archive, std::slice::from_ref(&source), format, None)
                .expect_err("a non-UTF-8 name cannot be stored losslessly");
        assert!(error.contains("non-UTF-8 name"), "{format:?}: {error}");
    }
    let archive = root.path().join("archive.tar");
    write_compression_fixture(
        &archive,
        std::slice::from_ref(&source),
        ArchiveFormat::Tar,
        None,
    )?;
    Ok(())
}

#[test]
fn encrypted_seven_z_archives_are_still_compressed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("zeros.bin");
    fs::write(&source, vec![0u8; 4 << 20])?;
    let plain = root.path().join("plain.7z");
    let encrypted = root.path().join("encrypted.7z");
    write_compression_fixture(
        &plain,
        std::slice::from_ref(&source),
        ArchiveFormat::SevenZ,
        None,
    )?;
    write_compression_fixture(
        &encrypted,
        std::slice::from_ref(&source),
        ArchiveFormat::SevenZ,
        Some("secret"),
    )?;
    let plain_len = fs::metadata(&plain)?.len();
    let encrypted_len = fs::metadata(&encrypted)?.len();
    assert!(
        encrypted_len < 1 << 20,
        "encrypted archive should compress ({encrypted_len} bytes, plain {plain_len} bytes)"
    );
    Ok(())
}

const SOURCE_TIME: i64 = 1_000_000_000;

#[derive(PartialEq, Eq)]
struct CompressedMetadata {
    mode: Option<u32>,
    modified: Option<i64>,
}

impl std::fmt::Debug for CompressedMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.mode {
            Some(mode) => write!(formatter, "mode {mode:o}, ")?,
            None => formatter.write_str("no mode, ")?,
        }
        match self.modified {
            Some(seconds) => write!(formatter, "modified {seconds}"),
            None => formatter.write_str("no time"),
        }
    }
}

fn zip_dos_seconds(time: zip::DateTime) -> Result<i64, Box<dyn Error>> {
    Ok(super::super::from_local_time(
        i32::from(time.year()),
        i32::from(time.month()),
        i32::from(time.day()),
        i32::from(time.hour()),
        i32::from(time.minute()),
        i32::from(time.second()),
    )
    .ok_or("no such local time")?)
}

fn zip_extended_time(entry: &zip::read::ZipFile<'_, fs::File>) -> Option<i64> {
    entry.extra_data_fields().find_map(|field| match field {
        zip::ExtraField::ExtendedTimestamp(stamp) => stamp.mod_time().map(i64::from),
        _ => None,
    })
}

fn read_compressed_metadata(
    path: &Path,
    format: ArchiveFormat,
    password: Option<&str>,
) -> Result<BTreeMap<PathBuf, CompressedMetadata>, Box<dyn Error>> {
    let file = fs::File::open(path)?;
    let mut result = BTreeMap::new();
    match format {
        ArchiveFormat::Zip => {
            let mut archive = zip::ZipArchive::new(file)?;
            for index in 0..archive.len() {
                let options =
                    zip::read::ZipReadOptions::new().password(password.map(str::as_bytes));
                let entry = archive.by_index_with_options(index, options)?;
                let modified = match zip_extended_time(&entry) {
                    Some(seconds) => Some(seconds),
                    None => entry.last_modified().map(zip_dos_seconds).transpose()?,
                };
                let metadata = CompressedMetadata {
                    mode: entry.unix_mode().map(|mode| mode & 0o7777),
                    modified,
                };
                let name = PathBuf::from(entry.name().expect("member name").trim_end_matches('/'));
                assert!(result.insert(name, metadata).is_none());
            }
        }
        ArchiveFormat::Tar | ArchiveFormat::TarGz => {
            let reader: Box<dyn Read> = if format == ArchiveFormat::TarGz {
                Box::new(flate2::read::GzDecoder::new(file))
            } else {
                Box::new(file)
            };
            for entry in tar::Archive::new(reader).entries()? {
                let entry = entry?;
                let metadata = CompressedMetadata {
                    mode: Some(entry.header().mode()? & 0o7777),
                    modified: Some(i64::try_from(entry.header().mtime()?)?),
                };
                assert!(
                    result
                        .insert(entry.path()?.into_owned(), metadata)
                        .is_none()
                );
            }
        }
        ArchiveFormat::SevenZ => {
            let archive = sevenz_rust2::ArchiveReader::new(
                file,
                password
                    .map(sevenz_rust2::Password::from)
                    .unwrap_or_default(),
            )?;
            for entry in &archive.archive().files {
                let mode = (entry.has_windows_attributes && entry.windows_attributes & 0x8000 != 0)
                    .then_some((entry.windows_attributes >> 16) & 0o7777);
                let modified = if entry.has_last_modified_date {
                    let time = std::time::SystemTime::from(entry.last_modified_date);
                    Some(i64::try_from(
                        time.duration_since(std::time::UNIX_EPOCH)?.as_secs(),
                    )?)
                } else {
                    None
                };
                let metadata = CompressedMetadata { mode, modified };
                assert!(
                    result
                        .insert(PathBuf::from(entry.name()), metadata)
                        .is_none()
                );
            }
        }
        ArchiveFormat::Rar => return Err("RAR compression is not supported".into()),
    }
    Ok(result)
}

#[test]
fn compression_records_source_times_and_modes_in_every_format() -> Result<(), Box<dyn Error>> {
    for (format, password) in [
        (ArchiveFormat::Zip, None),
        (ArchiveFormat::Zip, Some("test-password")),
        (ArchiveFormat::Tar, None),
        (ArchiveFormat::TarGz, None),
        (ArchiveFormat::SevenZ, None),
    ] {
        let context = format!("{format:?} password={}", password.is_some());
        let root = tempfile::tempdir()?;
        let source = root.path().join("dir");
        fs::create_dir(&source)?;
        fs::write(source.join("run.sh"), b"#!/bin/sh\n")?;
        fs::write(source.join("secret.txt"), b"secret")?;
        fs::set_permissions(source.join("run.sh"), fs::Permissions::from_mode(0o755))?;
        fs::set_permissions(source.join("secret.txt"), fs::Permissions::from_mode(0o600))?;
        // 7z refuses links (`compression_reports_unsupported_7z_links_without_committing`).
        let with_link = format != ArchiveFormat::SevenZ;
        if with_link {
            std::os::unix::fs::symlink("run.sh", source.join("link"))?;
            set_times_without_following(&source.join("link"), SOURCE_TIME)?;
        }
        set_times_without_following(&source.join("run.sh"), SOURCE_TIME)?;
        set_times_without_following(&source.join("secret.txt"), SOURCE_TIME)?;
        fs::set_permissions(&source, fs::Permissions::from_mode(0o750))?;
        set_times_without_following(&source, SOURCE_TIME)?;
        let mut expected = BTreeMap::from([
            (PathBuf::from("dir"), (0o750, SOURCE_TIME)),
            (PathBuf::from("dir/run.sh"), (0o755, SOURCE_TIME)),
            (PathBuf::from("dir/secret.txt"), (0o600, SOURCE_TIME)),
        ]);
        if with_link {
            expected.insert(PathBuf::from("dir/link"), (0o777, SOURCE_TIME));
        }
        let expected = expected
            .into_iter()
            .map(|(name, (mode, modified))| {
                (
                    name,
                    CompressedMetadata {
                        mode: Some(mode),
                        modified: Some(modified),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let archive = root.path().join("archive");

        write_compression_fixture(&archive, &[source], format, password)?;

        assert_eq!(
            read_compressed_metadata(&archive, format, password)?,
            expected,
            "{context}"
        );
        if format == ArchiveFormat::Zip {
            let mut zip = zip::ZipArchive::new(fs::File::open(&archive)?)?;
            let local = LocalTime::from_unix_local(SOURCE_TIME)?;
            for index in 0..zip.len() {
                let entry = zip.by_index_raw(index)?;
                let dos = entry.last_modified().ok_or("ZIP entry has no DOS time")?;
                assert_eq!(
                    (
                        i32::from(dos.year()),
                        i32::from(dos.month()),
                        i32::from(dos.day()),
                        i32::from(dos.hour()),
                        i32::from(dos.minute()),
                        i32::from(dos.second()),
                    ),
                    (
                        local.year(),
                        local.month(),
                        local.day_of_month(),
                        local.hour(),
                        local.minute(),
                        local.second(),
                    ),
                    "{context}: DOS time of `{}`",
                    entry.name().expect("member name")
                );
                assert_eq!(
                    zip_extended_time(&entry),
                    Some(SOURCE_TIME),
                    "{context}: UT field of `{}`",
                    entry.name().expect("member name")
                );
            }
        }
    }
    Ok(())
}

#[test]
fn zip_times_outside_the_dos_range_are_clamped_without_failing() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    fs::create_dir(&source)?;
    let epoch = source.join("epoch.txt");
    let beyond_i32 = source.join("beyond-i32.txt");
    let beyond_dos = source.join("beyond-dos.txt");
    let beyond_i32_time = i64::from(u32::MAX) + 10;
    let beyond_dos_time = 4_360_000_000;
    for (path, seconds) in [
        (&epoch, 0),
        (&beyond_i32, beyond_i32_time),
        (&beyond_dos, beyond_dos_time),
    ] {
        fs::write(path, b"time")?;
        set_times_without_following(path, seconds)?;
    }
    let archive = root.path().join("archive.zip");

    write_compression_fixture(
        &archive,
        &[epoch, beyond_i32, beyond_dos],
        ArchiveFormat::Zip,
        None,
    )?;

    let mut zip = zip::ZipArchive::new(fs::File::open(&archive)?)?;
    let dos_parts = |time: zip::DateTime| {
        (
            time.year(),
            time.month(),
            time.day(),
            time.hour(),
            time.minute(),
            time.second(),
        )
    };
    let entry = zip.by_name("epoch.txt")?;
    assert_eq!(
        entry.last_modified().map(dos_parts),
        Some(dos_parts(zip::DateTime::DEFAULT))
    );
    assert_eq!(zip_extended_time(&entry), Some(0));
    drop(entry);
    let entry = zip.by_name("beyond-i32.txt")?;
    let local = LocalTime::from_unix_local(beyond_i32_time)?;
    assert_eq!(
        entry.last_modified().map(dos_parts),
        Some((
            u16::try_from(local.year())?,
            u8::try_from(local.month())?,
            u8::try_from(local.day_of_month())?,
            u8::try_from(local.hour())?,
            u8::try_from(local.minute())?,
            u8::try_from(local.second() & !1)?,
        ))
    );
    assert_eq!(zip_extended_time(&entry), None);
    drop(entry);
    let entry = zip.by_name("beyond-dos.txt")?;
    assert_eq!(
        entry.last_modified().map(dos_parts),
        Some((2107, 12, 31, 23, 59, 58))
    );
    assert_eq!(zip_extended_time(&entry), None);
    Ok(())
}
