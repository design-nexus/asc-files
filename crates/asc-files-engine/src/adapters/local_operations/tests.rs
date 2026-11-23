// SPDX-License-Identifier: MIT
//! Spike: the engine's test seams: the fault-injecting filesystem and the copy-activity counter.

pub(crate) mod fs_quirks;

/// File copies running at once, and the most seen, across copy workers.
static COPY_ACTIVITY: std::sync::Mutex<(usize, usize)> = std::sync::Mutex::new((0, 0));

fn copy_activity() -> std::sync::MutexGuard<'static, (usize, usize)> {
    COPY_ACTIVITY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub(super) struct CopyActivity;

impl CopyActivity {
    pub(super) fn start() -> Self {
        let mut activity = copy_activity();
        activity.0 += 1;
        activity.1 = activity.1.max(activity.0);
        Self
    }
}

impl Drop for CopyActivity {
    fn drop(&mut self) {
        copy_activity().0 -= 1;
    }
}
