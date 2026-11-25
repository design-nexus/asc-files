// SPDX-License-Identifier: MIT
//! The engine's error: what kind of failure it was, which callers branch on, and the sentence
//! shown for it. Converts to and from `glib::Error` only at the GLib boundary.

use std::fmt;

/// The kinds the engine and its callers tell apart; the names follow GIO's `IOErrorEnum`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ErrorKind {
    Failed,
    NotFound,
    Exists,
    IsDirectory,
    NotDirectory,
    NotEmpty,
    NotRegularFile,
    NotSymbolicLink,
    FilenameTooLong,
    InvalidFilename,
    TooManyLinks,
    NoSpace,
    InvalidArgument,
    PermissionDenied,
    NotSupported,
    NotMounted,
    Cancelled,
    ReadOnly,
    TimedOut,
    Busy,
    WouldBlock,
    TooManyOpenFiles,
    InvalidData,
    BrokenPipe,
    NoSuchDevice,
    /// The destination cannot hold a symbolic link the copy met.
    SymlinkUnheld,
    /// An archive is locked and no password was given.
    PasswordRequired,
    /// The password given does not open the archive.
    PasswordIncorrect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.to_owned(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn matches(&self, kind: ErrorKind) -> bool {
        self.kind == kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// This error, saying `more` other items failed too.
    pub(crate) fn with_more(self, more: usize) -> Self {
        let message = rust_i18n::t!(
            "%{error} (and %{count} more items failed)",
            error = self.message,
            count = more
        );
        Self {
            kind: self.kind,
            message: message.into_owned(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub(crate) fn io_error(error: impl fmt::Display) -> Error {
    Error::new(ErrorKind::Failed, &error.to_string())
}

/// An error from the system, with the kind its errno names (as GIO's `g_io_error_from_errno`
/// did for the legacy app), so a client can tell "not found" from "no space" without reading
/// the sentence.
pub(crate) fn os_io_error(error: impl Into<std::io::Error>) -> Error {
    let error: std::io::Error = error.into();
    let kind = error.raw_os_error().map_or(ErrorKind::Failed, errno_kind);
    Error::new(
        kind,
        &crate::adapters::local_operations::native_fs::os_error_text(error),
    )
}

fn errno_kind(errno: i32) -> ErrorKind {
    use rustix::io::Errno;
    match Errno::from_raw_os_error(errno) {
        Errno::NOENT => ErrorKind::NotFound,
        Errno::EXIST => ErrorKind::Exists,
        Errno::ISDIR => ErrorKind::IsDirectory,
        Errno::NOTDIR => ErrorKind::NotDirectory,
        Errno::NOTEMPTY => ErrorKind::NotEmpty,
        Errno::NAMETOOLONG => ErrorKind::FilenameTooLong,
        Errno::INVAL => ErrorKind::InvalidArgument,
        Errno::MLINK | Errno::LOOP => ErrorKind::TooManyLinks,
        Errno::NOSPC | Errno::DQUOT => ErrorKind::NoSpace,
        Errno::ACCESS | Errno::PERM => ErrorKind::PermissionDenied,
        Errno::NOTSUP | Errno::NOSYS => ErrorKind::NotSupported,
        Errno::CANCELED => ErrorKind::Cancelled,
        Errno::ROFS => ErrorKind::ReadOnly,
        Errno::TIMEDOUT => ErrorKind::TimedOut,
        Errno::BUSY | Errno::TXTBSY => ErrorKind::Busy,
        Errno::AGAIN => ErrorKind::WouldBlock,
        Errno::MFILE | Errno::NFILE => ErrorKind::TooManyOpenFiles,
        Errno::PIPE => ErrorKind::BrokenPipe,
        Errno::NODEV | Errno::NXIO => ErrorKind::NoSuchDevice,
        _ => ErrorKind::Failed,
    }
}

pub(crate) fn translated_io_error(message: &str) -> Error {
    io_error(crate::i18n::tr(message))
}

pub(crate) fn cancelled() -> Error {
    Error::new(ErrorKind::Cancelled, "Operation cancelled")
}

/// The localized sentence for an error: a fixed one for the kinds people meet, else its own.
pub(crate) fn message(error: &Error) -> String {
    let message = match error.kind {
        ErrorKind::NotFound => "No such file or folder",
        ErrorKind::PermissionDenied => "Permission denied",
        ErrorKind::NotDirectory => "Not a folder",
        ErrorKind::NoSpace => "There is not enough space on the device",
        ErrorKind::ReadOnly => "The file system is read-only",
        ErrorKind::TimedOut => "The operation timed out",
        ErrorKind::Exists => "An item with that name already exists",
        ErrorKind::NotEmpty => "The folder is not empty",
        ErrorKind::IsDirectory => "The item is a folder",
        ErrorKind::FilenameTooLong => "The name is too long",
        ErrorKind::InvalidFilename => crate::services::INVALID_NAME_REASON,
        ErrorKind::Busy => "The item is in use",
        ErrorKind::TooManyLinks => "There are too many levels of symbolic links",
        ErrorKind::NotMounted => "The location is not mounted",
        _ => return error.message.clone(),
    };
    crate::i18n::tr(message)
}

/// [`message`] for the `%{error}` slot after a colon.
pub(crate) fn detail(error: &Error) -> String {
    crate::services::error_detail(message(error))
}

/// The copy's own failure, or that and the cleanup that could not finish after it.
pub(crate) fn after_cleanup(copy_error: Error, cleanup: Result<(), Error>) -> Error {
    match cleanup {
        Ok(()) => copy_error,
        Err(cleanup_error) => io_error(rust_i18n::t!(
            "%{error}; the incomplete copy could not be removed: %{cleanup_error}",
            error = message(&copy_error),
            cleanup_error = detail(&cleanup_error)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_can_say_how_many_more_failed() {
        let error = Error::new(ErrorKind::PermissionDenied, "a.jpg could not be read").with_more(2);
        assert_eq!(
            error.message(),
            "a.jpg could not be read (and 2 more items failed)"
        );
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }
}
