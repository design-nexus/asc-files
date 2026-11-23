// SPDX-License-Identifier: MIT
//! The Files engine: copy, move, replace, Trash, jobs and listings on native paths, with no
//! GLib, Qt or UI dependency.

rust_i18n::i18n!("locales", fallback = "en");

pub mod adapters;
pub mod drives;
pub mod error;
pub mod i18n;
pub mod jobs;
pub mod listing;
pub mod merge;
pub mod model;
pub(crate) mod serde_path;
pub mod services;
pub mod trash;
pub mod undo;
pub(crate) mod util;

/// The public operations API.
pub use adapters::local_operations::copy_engine::ops;
#[cfg(test)]
pub(crate) mod test_support;
