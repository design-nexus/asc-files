// SPDX-License-Identifier: MIT
//! The engine's modules; the GLib orchestration that drove them stays in nyx-files-core.

pub mod copy_engine;
pub(crate) mod deferred_close;
pub(crate) mod deletion;
pub(crate) mod environment;
pub(crate) mod fs_ops;
pub(crate) mod holders;
pub(crate) mod leftovers;
pub(crate) mod mount_profile;
pub(crate) mod names;
pub(crate) mod native_fs;
pub mod placement;
pub(crate) mod progress;
pub(crate) mod replacement;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod work_queue;

pub(crate) use crate::error::{
    after_cleanup as copy_failure_after_cleanup, cancelled as cancelled_local_operation, io_error,
    translated_io_error,
};
use native_fs::os_error_text;
use placement::PlaceError;

const INVALID_NAME_REASON: &str = crate::services::INVALID_NAME_REASON;
const UNCONFIRMED_REASON: &str =
    "The result could not be confirmed. Check both locations before trying again.";
const UNSUPPORTED_REASON: &str =
    "This location can\u{2019}t guarantee that another item won\u{2019}t be overwritten";
const MAX_LOCAL_IO_WORKERS: usize = 8;

fn blocking_join_message(error: Box<dyn std::any::Any + Send>) -> String {
    error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            error
                .downcast_ref::<&str>()
                .map(|message| (*message).to_owned())
        })
        .unwrap_or_else(|| "filesystem sync panicked".to_owned())
}

pub(super) fn placement_reason(error: PlaceError) -> String {
    match error {
        PlaceError::Exists => crate::i18n::tr("An item with that name already exists"),
        PlaceError::InvalidName => crate::i18n::tr(INVALID_NAME_REASON),
        PlaceError::IntoItself => crate::i18n::tr("Can\u{2019}t put a folder inside itself"),
        PlaceError::Changed => {
            crate::i18n::tr("The target changed while the operation was in progress")
        }
        PlaceError::Unconfirmed => crate::i18n::tr(UNCONFIRMED_REASON),
        PlaceError::CrossDevice => os_error_text(rustix::io::Errno::XDEV),
        PlaceError::Unsupported => crate::i18n::tr(UNSUPPORTED_REASON),
        PlaceError::Io(error) => os_error_text(error),
    }
}

pub(super) fn placement_error_kind(error: PlaceError) -> crate::error::ErrorKind {
    match error {
        PlaceError::Exists => crate::error::ErrorKind::Exists,
        PlaceError::InvalidName => crate::error::ErrorKind::InvalidFilename,
        _ => crate::error::ErrorKind::Failed,
    }
}
