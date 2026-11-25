// SPDX-License-Identifier: MIT
//! An archive's listing as a tree of folders, for its preview; legacy's (`services/preview.rs`).

use super::{ArchiveFileEntry, ArchiveFormat, split_archive_name};
use std::ffi::OsStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveDirectory {
    pub name: String,
    pub children: Vec<ArchiveNode>,
}

impl Drop for ArchiveDirectory {
    // Recursive drop glue can overflow the stack on attacker-controlled nesting.
    fn drop(&mut self) {
        if self.children.is_empty() {
            return;
        }
        let mut stack = vec![std::mem::take(&mut self.children)];
        while let Some(mut children) = stack.pop() {
            for child in children.drain(..) {
                if let ArchiveNode::Directory(mut directory) = child {
                    stack.push(std::mem::take(&mut directory.children));
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveNode {
    Directory(ArchiveDirectory),
    File { name: String, size: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivePreviewTree {
    pub root: ArchiveDirectory,
    pub file_count: usize,
}

/// Callers must enforce the listing name budget before constructing this unbounded tree.
pub fn archive_preview_tree(entries: Vec<ArchiveFileEntry>) -> ArchivePreviewTree {
    #[derive(Default)]
    struct Builder {
        directory: bool,
        name: String,
        size: u64,
        children: Vec<Builder>,
        // Index only directories: duplicate files must remain distinct members.
        dirs: std::collections::HashMap<String, usize>,
    }

    fn insert(seed: &mut Builder, segments: Vec<&str>, directory: bool, size: u64) {
        let Some(last) = segments.as_slice().split_last() else {
            return;
        };
        let mut current = seed;
        for parent in last.1 {
            let next = match current.dirs.get(*parent).copied() {
                Some(index) => &mut current.children[index],
                None => {
                    current
                        .dirs
                        .insert((*parent).to_owned(), current.children.len());
                    current.children.push(Builder {
                        directory: true,
                        name: (*parent).to_owned(),
                        ..Builder::default()
                    });
                    let last = current.children.len() - 1;
                    &mut current.children[last]
                }
            };
            current = next;
        }
        if directory {
            if !current.dirs.contains_key(*last.0) {
                current
                    .dirs
                    .insert((*last.0).to_owned(), current.children.len());
                current.children.push(Builder {
                    directory: true,
                    name: (*last.0).to_owned(),
                    ..Builder::default()
                });
            }
        } else {
            current.children.push(Builder {
                directory: false,
                name: (*last.0).to_owned(),
                size,
                ..Builder::default()
            });
        }
    }

    fn sort_and_convert(children: Vec<Builder>) -> (Vec<ArchiveNode>, usize) {
        // Use heap frames so hostile nesting cannot exhaust the call stack.
        struct Frame {
            name: String,
            children: Vec<Builder>,
            next: usize,
            nodes: Vec<ArchiveNode>,
            file_count: usize,
        }

        fn sort_level(children: &mut [Builder]) {
            children.sort_by_cached_key(|child| {
                (
                    std::cmp::Reverse(child.directory),
                    child.name.to_ascii_lowercase(),
                    child.name.clone(),
                )
            });
        }

        let mut root_children = children;
        sort_level(&mut root_children);
        let mut stack = vec![Frame {
            name: String::new(),
            children: root_children,
            next: 0,
            nodes: Vec::new(),
            file_count: 0,
        }];
        while !stack.is_empty() {
            let advanced = {
                let Some(frame) = stack.last_mut() else {
                    break;
                };
                if frame.next >= frame.children.len() {
                    None
                } else {
                    let child = std::mem::take(&mut frame.children[frame.next]);
                    frame.next += 1;
                    Some(child)
                }
            };
            let Some(child) = advanced else {
                let frame = stack
                    .pop()
                    .expect("levels complete before the stack empties");
                match stack.last_mut() {
                    Some(parent) => {
                        parent.file_count += frame.file_count;
                        parent.nodes.push(ArchiveNode::Directory(ArchiveDirectory {
                            name: frame.name,
                            children: frame.nodes,
                        }));
                    }
                    None => return (frame.nodes, frame.file_count),
                }
                continue;
            };
            if child.directory {
                let mut grandchildren = child.children;
                sort_level(&mut grandchildren);
                stack.push(Frame {
                    name: child.name,
                    children: grandchildren,
                    next: 0,
                    nodes: Vec::new(),
                    file_count: 0,
                });
            } else {
                let Some(frame) = stack.last_mut() else {
                    unreachable!("a file always has a level awaiting it");
                };
                frame.file_count += 1;
                frame.nodes.push(ArchiveNode::File {
                    name: child.name,
                    size: child.size,
                });
            }
        }
        unreachable!("archive levels complete before the stack empties");
    }

    let mut root = Builder {
        directory: true,
        ..Builder::default()
    };
    for entry in entries {
        let segments = split_archive_name(&entry.name);
        insert(&mut root, segments, entry.directory, entry.size);
    }
    let (children, file_count) = sort_and_convert(root.children);
    ArchivePreviewTree {
        root: ArchiveDirectory {
            name: String::new(),
            children,
        },
        file_count,
    }
}

/// The archive format a preview can list, by the file's name (RAR's listing needs the RAR helper).
pub fn archive_preview_format(name: &OsStr) -> Option<ArchiveFormat> {
    match ArchiveFormat::from_extension(&name.to_string_lossy()) {
        Some(
            format @ (ArchiveFormat::Zip
            | ArchiveFormat::SevenZ
            | ArchiveFormat::TarGz
            | ArchiveFormat::Tar),
        ) => Some(format),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
