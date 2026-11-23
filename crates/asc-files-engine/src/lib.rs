// SPDX-License-Identifier: MIT
//! Spike: the legacy Nyx Files copy/move engine, built with no GLib dependency.

rust_i18n::i18n!("locales", fallback = "en");

pub mod adapters;
pub mod error;
pub mod i18n;
pub mod jobs;
pub mod model;
pub mod services;
pub(crate) mod util;

/// The public operations API.
pub use adapters::local_operations::copy_engine::ops;
#[cfg(test)]
pub(crate) mod test_support;
