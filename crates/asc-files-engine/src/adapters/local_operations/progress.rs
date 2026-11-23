// SPDX-License-Identifier: MIT
//! Spike: engine items extracted from the legacy adapters/local_operations/progress.rs.

use super::copy_engine::SharedProgress;
use crate::{
    model::Location,
    services::{OperationEvent, OperationRequestId, TransferReport},
};

use super::translated_io_error;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Reported {
    pub(super) bytes: u64,
    pub(super) files: usize,
}

impl Reported {
    pub(super) fn since(self, earlier: Self) -> Self {
        Self {
            bytes: self.bytes.saturating_sub(earlier.bytes),
            files: self.files.saturating_sub(earlier.files),
        }
    }
}
