// SPDX-License-Identifier: MIT
//! Archives: listing, extracting and compressing ZIP, 7z, TAR and TAR.GZ, ported from legacy's
//! `adapters/local_operations/archive` (B9).
//!
//! Extraction confines writes to a pinned destination; compression writes into a private staging
//! folder in the destination and publishes through the engine's own placement. Both run on the
//! caller's thread (a job's worker) and report progress through shared counters.

mod compression;
mod decoders;
mod destination;
mod extraction;
mod listing;
mod rar_stream;
pub(crate) mod rar_wire;

pub use listing::{
    ARCHIVE_PREVIEW_FAILED_MESSAGE, ARCHIVE_TOO_LARGE_MESSAGE, ARCHIVE_UNSUPPORTED_MESSAGE,
    ArchiveListing, ArchiveListingStatus, INVALID_ARCHIVE, MAX_ARCHIVE_PASSWORD_BYTES,
    archive_payload_valid, decode_archive_listing, encode_archive_result,
    list_archive_entries_direct,
};
pub use rar_stream::helper as rar_helper;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
pub(crate) use fixtures::write_compression_fixture;
#[cfg(test)]
mod tests;

use super::{
    copy_engine::ops::{self, Cancel, Progress},
    fs_ops::{Entry, fs},
    native_fs::open_local_parent_directory,
    typed_names,
};
use crate::{
    model::{Location, hidden_names::HiddenName},
    services::{
        ArchiveFormat, NameMode, PasswordFailure, TransferConflict, TransferReport,
        validate_basename,
    },
    trash::Trashed,
};
use compression::{compress_7z, compress_tar, compress_zip, inspect_archive_sources};
use decoders::{extract_7z_from_reader, extract_tar, extract_zip_from_archive};
use extraction::ArchiveOutcome;
use rustix::{
    fs::{AtFlags, FileType, Mode, OFlags},
    io::Errno,
};
use std::{
    ffi::OsStr,
    os::fd::AsFd,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// How far an archive operation is: members done, of how many (0 while they are counted).
#[derive(Debug, Default)]
pub struct Counters {
    pub done: Arc<AtomicUsize>,
    pub total: AtomicUsize,
}

/// A finished archive.
#[derive(Debug)]
pub struct Compressed {
    pub archive: PathBuf,
    /// Where the item the archive replaced went, when it went to a Trash.
    pub replaced: Option<Trashed>,
    /// The item it replaced was deleted (its drive has no Trash), so undo can't bring it back.
    pub original_deleted: bool,
    /// What publishing it did otherwise than asked (an original it could not remove, kept).
    pub changes: ops::Changes,
}

/// Compresses `sources` into `into`/`name`.`format`'s extension.
///
/// The archive is written to a private (0600) hidden file in `into`, then given its name the way
/// the engine places a moved item: failing where the name is taken, numbered, or replacing what
/// holds it (to the Trash, or deleted once the archive is in place where there is none). Its mode
/// is then the replaced file's, or `0o666` less the umask. A name the location would store
/// otherwise is refused before anything is written.
#[allow(clippy::too_many_arguments)]
pub fn compress(
    sources: &[PathBuf],
    into: &Path,
    name: &str,
    format: ArchiveFormat,
    password: Option<&str>,
    conflict: TransferConflict,
    counters: &Counters,
    cancelled: &Arc<AtomicBool>,
) -> Result<Compressed, ArchiveError> {
    validate_basename(name).map_err(|message| ArchiveError::Failed(crate::i18n::tr(message)))?;
    if sources.is_empty() {
        return Err(ArchiveError::Failed(crate::i18n::tr("Nothing to compress")));
    }
    if format == ArchiveFormat::Rar {
        return Err(ArchiveError::Failed(crate::i18n::tr(
            "RAR compression is not supported",
        )));
    }
    let file_name = format!("{name}.{}", format.extension());
    if let Some(refusal) = typed_names::refused_in(into, &file_name, false) {
        return Err(ArchiveError::Failed(refusal));
    }
    // An archive replaces a file of its name, never a folder.
    if conflict == TransferConflict::ReplaceExisting
        && std::fs::symlink_metadata(into.join(&file_name)).is_ok_and(|metadata| metadata.is_dir())
    {
        return Err(ArchiveError::Failed(
            rust_i18n::t!("A folder named “%{name}” is already here", name = file_name)
                .into_owned(),
        ));
    }
    let folder = open_local_parent_directory(into).map_err(ArchiveError::Failed)?;
    let (stage, fd) = super::placement::reserve_hidden_name(HiddenName::Copy, |candidate| {
        fs().create_exclusive(Entry::new(&folder, candidate), Mode::from_raw_mode(0o600))
            .map_err(super::placement::creation_error)
    })
    .map_err(|error| ArchiveError::Failed(super::placement_reason(error)))?;
    // Whatever goes wrong before the archive has its name, the staging file goes.
    let unstage = |error: ArchiveError| {
        let _ = fs().unlink(Entry::new(&folder, &stage), AtFlags::empty());
        error
    };
    let errno = |error: Errno| archive_io_failed(&error.into());
    let written = (|| {
        let sources_seen = inspect_archive_sources(sources, cancelled)?;
        counters.total.store(sources_seen.files, Ordering::Relaxed);
        let done = &counters.done;
        // The encoder writes through its own handle; the stage's own is closed last, checked.
        let file = std::fs::File::from(fs().dup(fd.as_fd()).map_err(errno)?);
        let encoded = match format {
            ArchiveFormat::Zip => compress_zip(file, sources, password, done, cancelled),
            ArchiveFormat::SevenZ => compress_7z(file, sources, password, done, cancelled),
            ArchiveFormat::TarGz => compress_tar(
                file,
                sources,
                Some(sources_seen.gzip_level()),
                done,
                cancelled,
            ),
            ArchiveFormat::Tar => compress_tar(file, sources, None, done, cancelled),
            ArchiveFormat::Rar => unreachable!("refused above"),
        };
        encoded?;
        check_archive_cancelled(cancelled)?;
        // On the drive before it takes the name, so a crash never leaves a short archive there.
        fs().sync_data(fd.as_fd()).map_err(errno)?;
        let replaced_mode = (conflict == TransferConflict::ReplaceExisting)
            .then(|| std::fs::symlink_metadata(into.join(&file_name)).ok())
            .flatten()
            .filter(std::fs::Metadata::is_file)
            .map(|metadata| {
                std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o7777
            });
        fs().chmod_fd(
            fd.as_fd(),
            replaced_mode.unwrap_or(0o666 & !destination::process_umask()),
        )
        .map_err(errno)?;
        // A drive may report a failed write only when the file is closed (NFS, SMB).
        fs().close(fd).map_err(errno)?;
        check_archive_cancelled(cancelled)
    })();
    written.map_err(unstage)?;
    let stage = into.join(stage);
    let (cancel, progress) = (Cancel::sharing(cancelled.clone()), Progress::new());
    let target = (into, OsStr::new(&file_name));
    let published = match conflict {
        TransferConflict::FailIfExists => {
            ops::move_as(&stage, target, NameMode::Keep, &cancel, &progress)
                .map(|moved| (moved.name, None, false, moved.changes))
        }
        TransferConflict::KeepBoth => {
            ops::move_keeping_both_as(&stage, target, NameMode::Keep, &cancel, &progress)
                .map(|moved| (moved.name, None, false, moved.changes))
        }
        TransferConflict::ReplaceExisting => {
            ops::move_replacing_as(&stage, target, NameMode::Keep, &cancel, &progress).map(
                |replaced| {
                    (
                        replaced.name,
                        replaced.original,
                        replaced.original_deleted,
                        replaced.changes,
                    )
                },
            )
        }
        TransferConflict::Merge => Err(crate::error::translated_io_error(
            "An archive can't be merged",
        )),
    };
    match published {
        Ok((name, replaced, original_deleted, changes)) => Ok(Compressed {
            archive: into.join(name),
            replaced,
            original_deleted,
            changes,
        }),
        Err(error) => {
            let _ = std::fs::remove_file(&stage);
            Err(archive_failed(error.message()))
        }
    }
}

/// What an extraction did.
#[derive(Debug)]
pub enum Extracted {
    /// Every member is in place; `first_name` is the one top-level item, when there is one.
    Completed { first_name: Option<String> },
    /// Cancelled part way: what was written, what was left half done, what was never started.
    Cancelled {
        completed: Vec<Location>,
        failed: Vec<Location>,
        not_attempted: Vec<Location>,
    },
}

/// Extracts the archive `archive` into the folder `into`, made if it is missing (and removed
/// again if the extraction fails while it is still empty). `created_destination`: the caller made
/// `into` for this extraction, so it may be removed the same way. What it renamed or left out is
/// noted in `report`, whatever the outcome.
pub fn extract(
    archive: &Path,
    into: &Path,
    created_destination: bool,
    password: Option<&str>,
    counters: &Counters,
    cancelled: &Arc<AtomicBool>,
    report: &mut TransferReport,
) -> Result<Extracted, ArchiveError> {
    let display_name = archive
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    // A listing may be stale: a FIFO in the archive's place would block a decoder that opened it.
    if super::fs_ops::folder_stamp(archive, false)
        .is_ok_and(|stamp| stamp.kind != FileType::RegularFile)
    {
        return Err(ArchiveError::Failed(
            rust_i18n::t!("Not an archive: “%{name}”", name = display_name).into_owned(),
        ));
    }
    let created = created_destination
        || super::fs_ops::folder_stamp(into, false).is_err_and(|error| error == Errno::NOENT);
    if created && let Err(error) = std::fs::create_dir_all(into) {
        return Err(ArchiveError::Failed(
            rust_i18n::t!(
                "Could not create folder: %{error}",
                error = crate::services::io_error_detail(&error)
            )
            .into_owned(),
        ));
    }
    let mut created = CreatedFolder(created.then_some(into));
    let done = &counters.done;
    let extracted = (|| match ArchiveFormat::from_extension(&display_name) {
        Some(ArchiveFormat::Zip) => {
            let file = std::fs::File::open(archive)
                .map_err(|error| crate::services::io_error_message(&error))?;
            let mut zip = zip::ZipArchive::new(file).map_err(decoders::zip_error)?;
            counters.total.store(zip.len(), Ordering::Relaxed);
            extract_zip_from_archive(
                &mut zip,
                into,
                &display_name,
                password,
                done,
                cancelled,
                report,
            )
        }
        Some(ArchiveFormat::SevenZ) => {
            let password = password
                .map(sevenz_rust2::Password::from)
                .unwrap_or_default();
            let file = std::fs::File::open(archive)
                .map_err(|error| crate::services::io_error_message(&error))?;
            extract_7z_from_reader(file, into, &display_name, password, done, cancelled, report)
        }
        Some(ArchiveFormat::TarGz) => {
            extract_tar(archive, into, &display_name, true, done, cancelled, report)
        }
        Some(ArchiveFormat::Tar) => {
            extract_tar(archive, into, &display_name, false, done, cancelled, report)
        }
        Some(ArchiveFormat::Rar) => decoders::extract_rar(
            archive,
            into,
            &display_name,
            password,
            done,
            cancelled,
            report,
        ),
        None => Err(archive_failed(rust_i18n::t!(
            "Unsupported archive format: %{name}",
            name = display_name
        ))),
    })();
    match extracted? {
        ArchiveOutcome::Completed(first_name) => {
            created.0 = None;
            Ok(Extracted::Completed { first_name })
        }
        ArchiveOutcome::Cancelled {
            completed,
            failed,
            not_attempted,
        } => Ok(Extracted::Cancelled {
            completed,
            failed,
            not_attempted,
        }),
    }
}

/// A folder made for an extraction, removed again (if still empty) unless the extraction
/// completes.
struct CreatedFolder<'a>(Option<&'a Path>);

impl Drop for CreatedFolder<'_> {
    fn drop(&mut self) {
        let Some((parent, name)) = self
            .0
            .and_then(|folder| Some((folder.parent()?, folder.file_name()?)))
        else {
            return;
        };
        if let Ok(parent) = fs().open_path(
            parent.as_os_str(),
            OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            let _ = fs().unlink(Entry::new(&parent, name), AtFlags::REMOVEDIR);
        }
    }
}

pub(crate) const PASSWORD_REQUIRED: &str = "A password is required to extract this archive.";
/// 7z/RAR and failed decryption checks cannot always distinguish damage from a bad password.
pub(crate) const MAYBE_BAD_PASSWORD: &str = "The password may be incorrect.";

/// Byte size of the reusable read/write buffer used by [`copy_with_big_buf`].
const COPY_BUF: usize = 1 << 20;
const ARCHIVE_CANCELLED: &str = "Operation cancelled";

/// Failure or cooperative cancellation of a compress or extract step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveError {
    /// The operation was cancelled before it finished.
    Cancelled,
    /// Encoding, decoding, or filesystem work failed with this message.
    Failed(String),
    PasswordRequired(String),
    IncorrectPassword(String),
}

impl ArchiveError {
    /// Failures are localized where they are built; password and
    /// cancellation messages are shared constants translated here.
    pub fn user_message(&self) -> String {
        match self {
            Self::Cancelled => crate::i18n::tr(ARCHIVE_CANCELLED),
            Self::PasswordRequired(message) | Self::IncorrectPassword(message) => {
                crate::i18n::tr(message)
            }
            Self::Failed(message) => message.clone(),
        }
    }

    pub fn password_failure(&self) -> Option<PasswordFailure> {
        match self {
            Self::PasswordRequired(_) => Some(PasswordFailure::Required),
            Self::IncorrectPassword(_) => Some(PasswordFailure::Incorrect),
            Self::Cancelled | Self::Failed(_) => None,
        }
    }
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str(ARCHIVE_CANCELLED),
            Self::Failed(message)
            | Self::PasswordRequired(message)
            | Self::IncorrectPassword(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<ArchiveError> for crate::error::Error {
    fn from(error: ArchiveError) -> Self {
        use crate::error::ErrorKind;
        let kind = match &error {
            ArchiveError::Cancelled => ErrorKind::Cancelled,
            ArchiveError::PasswordRequired(_) => ErrorKind::PasswordRequired,
            ArchiveError::IncorrectPassword(_) => ErrorKind::PasswordIncorrect,
            ArchiveError::Failed(_) => ErrorKind::Failed,
        };
        Self::new(kind, &error.user_message())
    }
}

impl From<String> for ArchiveError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for ArchiveError {
    fn from(message: &str) -> Self {
        Self::Failed(message.to_owned())
    }
}

fn archive_failed(error: impl std::fmt::Display) -> ArchiveError {
    ArchiveError::Failed(error.to_string())
}

fn archive_read_failed(error: std::io::Error) -> ArchiveError {
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ArchiveError>())
    {
        Some(inner) => inner.clone(),
        None => archive_io_failed(&error),
    }
}

fn archive_io_failed(error: &std::io::Error) -> ArchiveError {
    archive_failed(crate::services::io_error_message(error))
}

/// Returns [`ArchiveError::Cancelled`] when the `cancelled` flag is set.
///
/// # Errors
///
/// - [`Cancelled`] if `cancelled` is set
///
/// [`Cancelled`]: ArchiveError::Cancelled
fn check_archive_cancelled(cancelled: &AtomicBool) -> Result<(), ArchiveError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(ArchiveError::Cancelled)
    } else {
        Ok(())
    }
}

/// Copies `reader` to `writer`, checking `cancelled` between 1 MiB chunks.
///
/// Returns the number of bytes written.
///
/// # Performance
///
/// Uses a [`COPY_BUF`]-sized heap buffer so large members are not copied
/// through the default 8 KiB [`std::io::copy`] path.
///
/// # Errors
///
/// - [`Cancelled`] if `cancelled` is set between reads
/// - [`Failed`] if a read or write fails
///
/// [`Cancelled`]: ArchiveError::Cancelled
/// [`Failed`]: ArchiveError::Failed
fn copy_with_big_buf(
    mut reader: impl std::io::Read,
    writer: &mut (impl std::io::Write + ?Sized),
    cancelled: &AtomicBool,
) -> Result<u64, ArchiveError> {
    let mut buf = vec![0u8; COPY_BUF];
    let mut total = 0;
    loop {
        check_archive_cancelled(cancelled)?;
        let n = reader.read(&mut buf).map_err(archive_read_failed)?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .map_err(|error| archive_io_failed(&error))?;
        total += n as u64;
    }
    Ok(total)
}

/// `seconds` since the epoch as local time; what glib's `DateTime::from_unix_local` gave legacy.
fn local_time(seconds: i64) -> Option<libc::tm> {
    let seconds = libc::time_t::try_from(seconds).ok()?;
    // SAFETY: `tm` is plain data that `localtime_r` fills; both pointers are valid for the call.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        (!libc::localtime_r(&seconds, &mut tm).is_null()).then_some(tm)
    }
}

/// A local date and time as seconds since the epoch, or `None` when it names no real moment
/// (31 February, say); what glib's `DateTime::from_local` gave legacy.
fn from_local_time(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
) -> Option<i64> {
    // SAFETY: `tm` is plain data; `mktime` reads and normalises it in place.
    let (seconds, tm) = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_year = year - 1900;
        tm.tm_mon = month - 1;
        tm.tm_mday = day;
        tm.tm_hour = hour;
        tm.tm_min = minute;
        tm.tm_sec = second;
        tm.tm_isdst = -1;
        (libc::mktime(&mut tm), tm)
    };
    // mktime moves an impossible date on (31 February to 3 March); such a date is refused.
    let same = tm.tm_year == year - 1900 && tm.tm_mon == month - 1 && tm.tm_mday == day;
    (seconds != -1 && same).then_some(i64::from(seconds))
}
