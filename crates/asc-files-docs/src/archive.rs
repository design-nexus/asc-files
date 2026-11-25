// SPDX-License-Identifier: MIT
//! An archive's listing for the preview, by the engine's reader (legacy's), as a flat list of
//! nodes each naming its parent: a deep tree stays flat on the way to the app.

use std::{ffi::OsStr, path::Path, sync::atomic::AtomicBool};

use asc_files_engine::{
    adapters::local_operations::archive::{
        ARCHIVE_TOO_LARGE_MESSAGE, ArchiveListingStatus, list_archive_entries_direct,
    },
    services::{ArchiveDirectory, ArchiveNode, archive_preview_format, archive_preview_tree},
};
use serde_json::{Value, json};

/// `{status, message?, files, nodes: [{name, parent, folder, size}]}`; node 0 is the archive
/// itself, and every other node comes after its parent. `format` is the archive's extension
/// (`zip`, `7z`, `tar`, `tar.gz`): in the sandbox the file goes by another name.
pub(crate) fn listing(format: &str, path: &Path) -> Value {
    let Some(format) = archive_preview_format(OsStr::new(&format!("archive.{format}"))) else {
        return json!({"status": "unsupported", "files": 0, "nodes": []});
    };
    let listed = list_archive_entries_direct(path, format, None, &AtomicBool::new(false));
    let listing = match listed {
        Ok(listing) => listing,
        Err(message) if message == ARCHIVE_TOO_LARGE_MESSAGE => {
            return json!({"status": "tooLarge", "files": 0, "nodes": []});
        }
        Err(message) => {
            return json!({"status": "error", "message": message, "files": 0, "nodes": []});
        }
    };
    let status = match listing.status {
        ArchiveListingStatus::Open => "open",
        ArchiveListingStatus::NeedsPassword => "needsPassword",
        ArchiveListingStatus::WrongPassword => "wrongPassword",
        ArchiveListingStatus::Unsupported => "unsupported",
    };
    let tree = archive_preview_tree(listing.entries);
    let mut nodes = vec![json!({"name": "", "parent": -1, "folder": true, "size": 0})];
    // Walked with a list of its own, so hostile nesting cannot exhaust the stack.
    let mut pending: Vec<(&ArchiveDirectory, usize)> = vec![(&tree.root, 0)];
    while let Some((directory, parent)) = pending.pop() {
        for child in &directory.children {
            let index = nodes.len();
            match child {
                ArchiveNode::Directory(folder) => {
                    nodes.push(
                        json!({"name": folder.name, "parent": parent, "folder": true, "size": 0}),
                    );
                    pending.push((folder, index));
                }
                ArchiveNode::File { name, size } => {
                    nodes.push(
                        json!({"name": name, "parent": parent, "folder": false, "size": size}),
                    );
                }
            }
        }
    }
    json!({"status": status, "files": tree.file_count, "nodes": nodes})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn an_archive_is_listed_as_nodes_after_their_parents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, body) in [
            ("docs/readme.txt", "hello"),
            ("docs/deep/x.bin", "xx"),
            ("top.txt", "t"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();

        let listed = listing("zip", &path);
        assert_eq!(listed["status"], "open");
        assert_eq!(listed["files"], 3);
        let nodes = listed["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 6, "{listed}");
        for (index, node) in nodes.iter().enumerate().skip(1) {
            assert!(node["parent"].as_i64().unwrap() < index as i64, "{node}");
        }
        let readme = nodes
            .iter()
            .find(|node| node["name"] == "readme.txt")
            .unwrap();
        assert_eq!(readme["size"], 5);
        assert_eq!(
            nodes[readme["parent"].as_u64().unwrap() as usize]["name"],
            "docs"
        );

        std::fs::write(dir.path().join("broken.zip"), b"not a zip").unwrap();
        assert_eq!(
            listing("zip", &dir.path().join("broken.zip"))["status"],
            "error"
        );
        assert_eq!(listing("rar", &path)["status"], "unsupported");
    }
}
