// SPDX-License-Identifier: MIT
//! Spike listing: names are read and sorted once at open; a window stats only its own rows,
//! so a 100k-entry folder costs one directory read, not 100k stats, before the first rows.
//! Flea's listing engine (arena, generations, watch) is the production plan.

use std::{
    collections::HashMap,
    ffi::OsString,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use asc_files_engine::error::{Error, ErrorKind};
use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct Listings {
    next: u64,
    open: HashMap<u64, Listing>,
}

struct Listing {
    folder: PathBuf,
    names: Vec<OsString>,
}

impl Listings {
    pub(crate) fn open(&mut self, folder: &Path) -> Result<(u64, usize), Error> {
        let entries = std::fs::read_dir(folder).map_err(|error| {
            let kind = match error.kind() {
                std::io::ErrorKind::NotFound => ErrorKind::NotFound,
                std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
                std::io::ErrorKind::NotADirectory => ErrorKind::NotDirectory,
                _ => ErrorKind::Failed,
            };
            Error::new(kind, &error.to_string())
        })?;
        let mut names: Vec<OsString> = entries
            .filter_map(|entry| Some(entry.ok()?.file_name()))
            .collect();
        names.sort_by(|a, b| natural_key(a).cmp(&natural_key(b)).then_with(|| a.cmp(b)));
        self.next += 1;
        let count = names.len();
        self.open.insert(
            self.next,
            Listing {
                folder: folder.to_path_buf(),
                names,
            },
        );
        Ok((self.next, count))
    }

    pub(crate) fn window(&self, listing: u64, start: usize, count: usize) -> Option<Vec<Value>> {
        let listing = self.open.get(&listing)?;
        let end = start.saturating_add(count).min(listing.names.len());
        let start = start.min(end);
        Some(
            listing.names[start..end]
                .iter()
                .map(|name| row(&listing.folder, name))
                .collect(),
        )
    }

    pub(crate) fn close(&mut self, listing: u64) {
        self.open.remove(&listing);
    }
}

/// A row stats its entry without following a link; one that vanished since open says so.
fn row(folder: &Path, name: &OsString) -> Value {
    let display = name.to_string_lossy();
    match std::fs::symlink_metadata(folder.join(name)) {
        Ok(meta) => {
            let kind = if meta.is_dir() {
                "folder"
            } else if meta.file_type().is_symlink() {
                "link"
            } else if meta.is_file() {
                "file"
            } else {
                "special"
            };
            json!({"name": display, "kind": kind, "size": meta.len(), "mtime": meta.mtime(), "mode": meta.mode() & 0o7777})
        }
        Err(_) => json!({"name": display, "kind": "gone"}),
    }
}

/// Case-folded, so "b" sorts beside "B"; the full name breaks ties.
fn natural_key(name: &OsString) -> String {
    name.to_string_lossy().to_lowercase()
}
