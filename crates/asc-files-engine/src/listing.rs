// SPDX-License-Identifier: MIT
//! Folder listings for a view: a folder is read and sorted once, and its rows are served in
//! windows, so a 100k-entry folder costs one directory read before the first rows rather than
//! 100k stats. Every read stamps a new generation; a window says which generation it is from,
//! so a client drops rows from a listing that has since changed.
//!
//! [`Watch`] follows the folder for outside changes. The design is Flea's (`src/backend/watch.rs`,
//! `watchpoll.rs`, `sort.rs`; MIT, see `docs/PROVENANCE.md`): inotify with only the events that
//! change what a listing shows, a burst coalesced into one notice, a deleted or unmounted folder
//! reported with the nearest folder that still exists, and network folders, whose remote changes
//! inotify never sees, polled by modification time under a deadline so a dead share costs one
//! parked thread and never a hang.

use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    adapters::volume::MountTable,
    error::{Error, os_io_error},
    services::{is_hidden_name, native_hidden_names},
};

/// What a listing is sorted by.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Modified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Order {
    pub key: SortKey,
    pub descending: bool,
    /// Folders, and links to folders, come first in either direction.
    pub folders_first: bool,
}

impl Default for Order {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            descending: false,
            folders_first: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// Names starting with a dot, or named in the folder's `.hidden` file. The engine's own
    /// hidden names are among them and are listed like any other: after a crash, a set-aside
    /// original may be the only copy of the user's file.
    pub show_hidden: bool,
    pub order: Order,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Folder,
    File,
    Link,
    /// A pipe, socket or device.
    Special,
}

/// What a row's stat said. Times are seconds and nanoseconds since the epoch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stat {
    pub size: u64,
    pub modified: (i64, u32),
    pub mode: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub name: OsString,
    pub kind: Kind,
    /// A link whose target is a folder, which opens as one.
    pub link_to_folder: bool,
    pub hidden: bool,
    /// None when the entry went between the read and this window.
    pub stat: Option<Stat>,
}

#[derive(Clone, Debug)]
struct Entry {
    name: OsString,
    kind: Kind,
    link_to_folder: bool,
    hidden: bool,
    /// Read for every entry only when the order needs it; a window stats its own rows afresh.
    stat: Option<Stat>,
}

impl Entry {
    fn opens_as_folder(&self) -> bool {
        self.kind == Kind::Folder || self.link_to_folder
    }
}

/// One folder, read and sorted.
#[derive(Debug)]
pub struct Listing {
    folder: PathBuf,
    options: Options,
    generation: u64,
    entries: Vec<Entry>,
    /// What interrupted operations left here (originals set aside, probes), hidden or not.
    leftovers: Vec<std::ffi::OsString>,
    /// Text typed over the folder: only the names it matches are listed.
    filter: String,
}

impl Listing {
    pub fn read(folder: &Path, options: Options) -> Result<Self, Error> {
        let mut listing = Self {
            folder: folder.to_path_buf(),
            options,
            generation: 0,
            entries: Vec::new(),
            leftovers: Vec::new(),
            filter: String::new(),
        };
        listing.refresh()?;
        Ok(listing)
    }

    /// Reads the folder again; the listing keeps what it had if the read fails.
    pub fn refresh(&mut self) -> Result<(), Error> {
        (self.entries, self.leftovers) = read_entries(&self.folder, &self.options)?;
        let mut filter = crate::search::Filter::new(&self.filter);
        if !filter.is_empty() {
            self.entries
                .retain(|entry| filter.matches(&entry.name.to_string_lossy()));
        }
        self.generation += 1;
        Ok(())
    }

    /// Re-reads under other options: hiding changes which entries are listed.
    pub fn set_options(&mut self, options: Options) -> Result<(), Error> {
        let previous = std::mem::replace(&mut self.options, options);
        self.refresh().inspect_err(|_| self.options = previous)
    }

    /// Lists only the names `text` matches (fuzzily; `!word` leaves out), reading the folder
    /// again; empty lists them all.
    pub fn set_filter(&mut self, text: &str) -> Result<(), Error> {
        let previous = std::mem::replace(&mut self.filter, text.to_owned());
        self.refresh().inspect_err(|_| self.filter = previous)
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The entries interrupted operations left in the folder, found as it was read, whether or
    /// not hidden entries are listed: a client offers to put originals back.
    pub fn leftovers(&self) -> &[std::ffi::OsString] {
        &self.leftovers
    }

    pub fn options(&self) -> Options {
        self.options
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The rows `start..start + count`, clamped to the listing, each statted now.
    pub fn window(&self, start: usize, count: usize) -> Vec<Row> {
        let end = start.saturating_add(count).min(self.entries.len());
        self.entries[start.min(end)..end]
            .iter()
            .map(|entry| Row {
                name: entry.name.clone(),
                kind: entry.kind,
                link_to_folder: entry.link_to_folder,
                hidden: entry.hidden,
                stat: lstat(&self.folder.join(&entry.name)),
            })
            .collect()
    }

    /// Where `name` sits in the listing, so a view can keep or restore its selection.
    pub fn position(&self, name: &OsStr) -> Option<usize> {
        self.entries.iter().position(|entry| entry.name == name)
    }
}

fn lstat(path: &Path) -> Option<Stat> {
    std::fs::symlink_metadata(path).ok().map(|meta| Stat {
        size: meta.len(),
        modified: (meta.mtime(), u32::try_from(meta.mtime_nsec()).unwrap_or(0)),
        mode: meta.mode() & 0o7777,
    })
}

fn read_entries(
    folder: &Path,
    options: &Options,
) -> Result<(Vec<Entry>, Vec<std::ffi::OsString>), Error> {
    let read = std::fs::read_dir(folder).map_err(os_io_error)?;
    let listed_hidden = native_hidden_names(folder);
    let wants_stats = options.order.key != SortKey::Name;
    let mut entries = Vec::new();
    let mut leftovers = Vec::new();
    for entry in read {
        // An entry that went while the folder was read is simply not listed.
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        if crate::recovery::is_leftover(&name) {
            leftovers.push(name.clone());
        }
        let hidden = is_hidden_name(&name, &listed_hidden);
        if hidden && !options.show_hidden {
            continue;
        }
        // d_type is free; only filesystems that leave it unknown cost a stat here.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let kind = if file_type.is_dir() {
            Kind::Folder
        } else if file_type.is_symlink() {
            Kind::Link
        } else if file_type.is_file() {
            Kind::File
        } else if file_type.is_fifo()
            || file_type.is_socket()
            || file_type.is_block_device()
            || file_type.is_char_device()
        {
            Kind::Special
        } else {
            Kind::File
        };
        let path = entry.path();
        let link_to_folder = kind == Kind::Link && path.is_dir();
        let stat = wants_stats.then(|| lstat(&path)).flatten();
        entries.push(Entry {
            name,
            kind,
            link_to_folder,
            hidden,
            stat,
        });
    }
    entries.sort_by(|a, b| compare(a, b, options.order));
    Ok((entries, leftovers))
}

fn compare(a: &Entry, b: &Entry, order: Order) -> Ordering {
    if order.folders_first {
        match b.opens_as_folder().cmp(&a.opens_as_folder()) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    let both_folders = a.opens_as_folder() && b.opens_as_folder();
    let by_key = match order.key {
        // A folder's own size says nothing about its contents, so folders keep name order.
        SortKey::Size if !both_folders => {
            let size = |entry: &Entry| entry.stat.map_or(0, |stat| stat.size);
            size(a).cmp(&size(b))
        }
        SortKey::Modified => {
            let modified = |entry: &Entry| entry.stat.map_or((0, 0), |stat| stat.modified);
            modified(a).cmp(&modified(b))
        }
        _ => Ordering::Equal,
    };
    let ordered =
        by_key.then_with(|| name_order(a.name.as_encoded_bytes(), b.name.as_encoded_bytes()));
    if order.descending {
        ordered.reverse()
    } else {
        ordered
    }
}

/// Finder's order rather than byte order: a run of digits compares by value (`file_2` before
/// `file_10`, leading zeros ignored) and letters compare without case. The raw bytes break a tie,
/// so `README` and `readme` always list in the same order. Bytes above ASCII compare as bytes,
/// which for UTF-8 is code point order. Ported from Flea's `sort.rs`.
pub fn name_order(a: &[u8], b: &[u8]) -> Ordering {
    finder_cmp(a, b).then_with(|| a.cmp(b))
}

fn finder_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (a_end, a_significant) = digit_run(a, i);
            let (b_end, b_significant) = digit_run(b, j);
            // More significant digits is a larger number; a 40-digit run fits no integer type.
            let (a_digits, b_digits) = (&a[a_significant..a_end], &b[b_significant..b_end]);
            match a_digits
                .len()
                .cmp(&b_digits.len())
                .then_with(|| a_digits.cmp(b_digits))
            {
                Ordering::Equal => {}
                other => return other,
            }
            (i, j) = (a_end, b_end);
            continue;
        }
        match a[i].to_ascii_lowercase().cmp(&b[j].to_ascii_lowercase()) {
            Ordering::Equal => {}
            other => return other,
        }
        i += 1;
        j += 1;
    }
    (a.len() - i).cmp(&(b.len() - j))
}

/// The digit run starting at `at`, as (one past its end, its first significant digit). A run of
/// zeros keeps one, so it compares as a single zero.
fn digit_run(bytes: &[u8], at: usize) -> (usize, usize) {
    let mut end = at;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    let mut significant = at;
    while significant + 1 < end && bytes[significant] == b'0' {
        significant += 1;
    }
    (end, significant)
}

/// An outside change to a watched folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    /// What the folder lists may differ now; read it again.
    Changed,
    /// The folder was deleted or moved away. `nearest` is the closest folder above it that
    /// existed when this was told, where a view can go; if that one goes too, its own listing
    /// says so in turn.
    Gone { nearest: PathBuf },
    /// The drive the folder is on was unmounted.
    Unmounted { nearest: PathBuf },
}

/// How often a network folder is checked, how long one check may take before the share is
/// treated as stalled, and how long a stalled share is left alone. Flea's values.
const POLL_EVERY: Duration = Duration::from_secs(5);
const POLL_DEADLINE: Duration = Duration::from_secs(10);
const STALLED_FOR: Duration = Duration::from_secs(60);
/// A burst of writes is told as one change.
const COALESCE: Duration = Duration::from_millis(100);
/// How often the watch thread checks whether it should stop.
const STOP_CHECK: Duration = Duration::from_millis(200);

/// Follows one folder until dropped. Changes arrive on the watch's own thread.
pub struct Watch {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
    follows_changes: bool,
}

impl Watch {
    pub fn start(folder: &Path, notify: impl Fn(Change) + Send + Sync + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let notify: Arc<dyn Fn(Change) + Send + Sync> = Arc::new(notify);
        let mut threads = Vec::new();
        let inotify = watch_inotify(folder);
        let follows_changes = inotify.is_some();
        if let Some((fd, wd)) = inotify {
            let (stop, notify, folder) = (stop.clone(), notify.clone(), folder.to_path_buf());
            threads.push(
                thread::Builder::new()
                    .name("asc-files-watch".into())
                    .spawn(move || follow(&fd, wd, &folder, &stop, &*notify))
                    .expect("a watch thread"),
            );
        }
        if is_network_folder(folder) {
            let (stop, folder) = (stop.clone(), folder.to_path_buf());
            threads.push(
                thread::Builder::new()
                    .name("asc-files-poll".into())
                    .spawn(move || poll(&folder, &stop, &*notify))
                    .expect("a poll thread"),
            );
        }
        Self {
            stop,
            threads,
            follows_changes,
        }
    }

    /// False when inotify refused the folder (no inotify, or out of watches): the listing still
    /// works but goes stale until it is read again.
    pub fn follows_changes(&self) -> bool {
        self.follows_changes
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, AtomicOrdering::Release);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn watch_mask() -> rustix::fs::inotify::WatchFlags {
    use rustix::fs::inotify::WatchFlags as W;
    // Not MODIFY: a file still being written is no change to a listing until it is closed.
    W::ATTRIB
        | W::CLOSE_WRITE
        | W::MOVED_FROM
        | W::MOVED_TO
        | W::CREATE
        | W::DELETE
        | W::DELETE_SELF
        | W::MOVE_SELF
        | W::ONLYDIR
}

fn watch_inotify(folder: &Path) -> Option<(rustix::fd::OwnedFd, i32)> {
    use rustix::fs::inotify;
    let fd = inotify::init(inotify::CreateFlags::CLOEXEC | inotify::CreateFlags::NONBLOCK).ok()?;
    let wd = inotify::add_watch(&fd, folder, watch_mask()).ok()?;
    Some((fd, wd))
}

/// What one batch of events says, the strongest first: unmounted, gone, changed.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
enum Seen {
    #[default]
    Nothing,
    Changed,
    Gone,
    Unmounted,
}

fn drain(fd: &rustix::fd::OwnedFd, wd: i32) -> Seen {
    use rustix::fs::inotify::{ReadFlags as R, Reader};
    let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 8192];
    let mut reader = Reader::new(fd, &mut buffer);
    let mut seen = Seen::Nothing;
    while let Ok(event) = reader.next() {
        let flags = event.events();
        let this = if flags.contains(R::QUEUE_OVERFLOW) {
            // Events were lost; only a fresh read can say what the folder holds.
            Seen::Changed
        } else if event.wd() != wd {
            Seen::Nothing
        } else if flags.contains(R::UNMOUNT) {
            Seen::Unmounted
        } else if flags.intersects(R::DELETE_SELF | R::MOVE_SELF | R::IGNORED) {
            Seen::Gone
        } else {
            Seen::Changed
        };
        seen = seen.max(this);
    }
    seen
}

fn wait_readable(fd: &rustix::fd::OwnedFd, timeout: Duration) -> bool {
    use rustix::event::{PollFd, PollFlags, Timespec};
    let timeout = Timespec {
        tv_sec: i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(timeout.subsec_nanos()),
    };
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    rustix::event::poll(&mut fds, Some(&timeout)).is_ok_and(|ready| ready > 0)
}

fn follow(
    fd: &rustix::fd::OwnedFd,
    wd: i32,
    folder: &Path,
    stop: &AtomicBool,
    notify: &(dyn Fn(Change) + Send + Sync),
) {
    while !stop.load(AtomicOrdering::Acquire) {
        if !wait_readable(fd, STOP_CHECK) {
            continue;
        }
        let mut seen = drain(fd, wd);
        if seen == Seen::Changed {
            // The rest of the burst joins this notice.
            thread::sleep(COALESCE);
            seen = seen.max(drain(fd, wd));
        }
        match seen {
            Seen::Nothing => {}
            Seen::Changed => notify(Change::Changed),
            Seen::Gone => {
                // A tree being removed loses its folders one by one; let it settle first.
                thread::sleep(COALESCE);
                notify(Change::Gone {
                    nearest: nearest_existing(folder),
                });
                return;
            }
            Seen::Unmounted => {
                notify(Change::Unmounted {
                    nearest: nearest_existing(folder),
                });
                return;
            }
        }
    }
}

/// The closest folder at or above `path` that exists, so a view never lands on nothing.
pub fn nearest_existing(path: &Path) -> PathBuf {
    path.ancestors()
        .skip(1)
        .find(|ancestor| ancestor.is_dir())
        .unwrap_or(Path::new("/"))
        .to_path_buf()
}

fn is_network_folder(folder: &Path) -> bool {
    let Ok(stat) = rustix::fs::statx(
        rustix::fs::CWD,
        folder,
        rustix::fs::AtFlags::empty(),
        rustix::fs::StatxFlags::MNT_ID,
    ) else {
        return false;
    };
    MountTable::current()
        .mount_of(stat.stx_mnt_id)
        .is_some_and(|(mount, _)| mount.outside_writers())
}

fn poll(folder: &Path, stop: &AtomicBool, notify: &(dyn Fn(Change) + Send + Sync)) {
    let mut last = None;
    let mut stalled_since: Option<Instant> = None;
    // The check still out on a stalled share; a new one starts only once it has returned.
    let mut outstanding: Option<mpsc::Receiver<Option<(i64, i64)>>> = None;
    let mut next = Instant::now();
    while !stop.load(AtomicOrdering::Acquire) {
        if Instant::now() < next {
            thread::sleep(STOP_CHECK);
            continue;
        }
        next = Instant::now() + POLL_EVERY;
        if let Some(since) = stalled_since {
            let returned = outstanding
                .as_ref()
                .is_some_and(|check| check.try_recv() != Err(mpsc::TryRecvError::Empty));
            if !returned || since.elapsed() < STALLED_FOR {
                continue;
            }
            stalled_since = None;
            outstanding = None;
        }
        let (send, check) = mpsc::channel();
        let path = folder.to_path_buf();
        let started = thread::Builder::new()
            .name("asc-files-poll-stat".into())
            .spawn(move || {
                let modified = std::fs::metadata(&path)
                    .ok()
                    .map(|meta| (meta.mtime(), meta.mtime_nsec()));
                let _ = send.send(modified);
            });
        if started.is_err() {
            continue;
        }
        match check.recv_timeout(POLL_DEADLINE) {
            Ok(Some(modified)) => {
                if last.is_some_and(|last| last != modified) {
                    notify(Change::Changed);
                }
                last = Some(modified);
            }
            // Gone: the inotify side, or the next read, says so.
            Ok(None) => {}
            Err(_) => {
                stalled_since = Some(Instant::now());
                outstanding = Some(check);
            }
        }
    }
}

/// Folders read at once, at most [`READERS_PER_DRIVE`] per drive, so a stalled network share
/// holds a bounded number of threads however often a view retries it. Flea's rule
/// (`listwork_scanner.rs`).
pub const READERS_PER_DRIVE: usize = 2;

/// Counts the reads in progress per drive.
#[derive(Default)]
pub struct Readers {
    reading: std::sync::Mutex<Vec<u64>>,
    /// Woken when a read ends, for those waiting for a place.
    freed: std::sync::Condvar,
}

/// A read in progress; it frees its place when dropped.
pub struct Reading {
    readers: Arc<Readers>,
    device: u64,
}

impl Readers {
    /// A place to read `folder`, or None when its drive already has as many stalled-or-running
    /// reads as it may.
    pub fn claim(self: &Arc<Self>, folder: &Path) -> Option<Reading> {
        self.claim_within(folder, std::time::Duration::ZERO)
    }

    /// A place to read `folder`, waiting up to `wait` for one to free: reads that end quickly
    /// (many folders opened at once, as Columns view does) only queue, and only reads that have
    /// not ended by then (a stalled share) refuse it.
    pub fn claim_within(
        self: &Arc<Self>,
        folder: &Path,
        wait: std::time::Duration,
    ) -> Option<Reading> {
        // stat of the folder itself can stall too; the drive is known from its parent's mount,
        // which for a mount point is the mount itself, so fall back to the folder's own path.
        let device = device_of(folder)?;
        let deadline = std::time::Instant::now() + wait;
        let mut reading = self
            .reading
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while reading.iter().filter(|held| **held == device).count() >= READERS_PER_DRIVE {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            reading = self
                .freed
                .wait_timeout(reading, left)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        reading.push(device);
        Some(Reading {
            readers: self.clone(),
            device,
        })
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        let mut reading = self
            .readers
            .reading
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(at) = reading.iter().position(|held| *held == self.device) {
            reading.swap_remove(at);
        }
        self.readers.freed.notify_all();
    }
}

/// The mount id of `folder` from the mount table alone, which never touches the drive, or its
/// device number where the table does not say.
fn device_of(folder: &Path) -> Option<u64> {
    let table = MountTable::current();
    if let Some((id, _)) = table
        .points()
        .filter(|(_, point)| folder.starts_with(point))
        .max_by_key(|(_, point)| point.as_os_str().len())
    {
        return Some(id);
    }
    std::fs::metadata(folder).ok().map(|meta| meta.dev())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_by_number_value_and_without_case() {
        let mut names = vec![
            "file_10", "File_2", "file_02", "apple", "Apple", "b", "file_1",
        ];
        names.sort_by(|a, b| name_order(a.as_bytes(), b.as_bytes()));
        assert_eq!(
            names,
            [
                "Apple", "apple", "b", "file_1", "File_2", "file_02", "file_10"
            ]
        );
    }

    #[test]
    fn a_digit_run_longer_than_any_integer_still_compares_by_value() {
        let big = format!("v{}", "9".repeat(40));
        let bigger = format!("v1{}", "0".repeat(40));
        assert_eq!(
            name_order(big.as_bytes(), bigger.as_bytes()),
            Ordering::Less
        );
    }

    #[test]
    fn the_nearest_existing_folder_is_above_the_gone_one() {
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("a/b/c");
        std::fs::create_dir_all(root.path().join("a")).unwrap();
        assert_eq!(nearest_existing(&gone), root.path().join("a"));
    }
}
