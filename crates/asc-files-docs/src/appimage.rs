// SPDX-License-Identifier: MIT
//! An AppImage's icon, for its thumbnail: the SquashFS image after the ELF runtime is read in
//! place (gzip, zstd or lz4; never run, never extracted to disk), and its `.DirIcon`, else the icon
//! its `.desktop` entry names, is drawn. Legacy's `sandbox_helper/appimage.rs`, which ran
//! `unsquashfs` instead.

use std::{
    fs,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

use backhand::{FilesystemReader, InnerNode};

#[cfg(test)]
mod tests;

const SQUASHFS_MAGIC: &[u8; 4] = b"hsqs";
const SHT_NOBITS: u32 = 8;
const ICON_EXTENSIONS: [&str; 2] = ["png", "svg"];
const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DESKTOP_BYTES: u64 = 64 * 1024;
/// Entries looked through in the image's top folder.
const MAX_ENTRIES: usize = 100_000;

pub(crate) fn render(input: &Path, edge: i32) -> Result<Vec<u8>, String> {
    let mut file = fs::File::open(input).map_err(|error| error.to_string())?;
    let offset = embedded_image_offset(&mut file)
        .ok_or_else(|| "No SquashFS image follows the ELF runtime".to_owned())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let image = FilesystemReader::from_reader_with_offset(BufReader::new(file), offset)
        .map_err(|error| format!("Unreadable SquashFS image: {error}"))?;
    let entries = root_entries(&image);
    let mut candidates = dir_icon_candidates(&entries);
    if let Some(desktop) = entries
        .iter()
        .find(|entry| entry.is_file && entry.name.ends_with(".desktop"))
        && let Ok(contents) = read(&image, &desktop.name, MAX_DESKTOP_BYTES)
        && let Some(icon) = desktop_icon(&contents)
    {
        candidates.extend(icon_name_candidates(&entries, &icon));
    }
    for candidate in candidates {
        if let Ok(bytes) = read(&image, &candidate, MAX_ICON_BYTES)
            && let Ok(png) = render_icon(&bytes, edge)
        {
            return Ok(png);
        }
    }
    Err("No usable icon inside the AppImage".to_owned())
}

/// A member's bytes, by its path from the image's root.
fn read(image: &FilesystemReader<'_>, path: &str, limit: u64) -> Result<Vec<u8>, String> {
    let wanted = Path::new("/").join(path.trim_start_matches('/'));
    let node = image
        .files()
        .find(|node| node.fullpath == wanted)
        .ok_or("No such member")?;
    let InnerNode::File(file) = &node.inner else {
        return Err("Not a file".into());
    };
    let mut bytes = Vec::new();
    image
        .file(file)
        .reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Member exceeds the size limit".into());
    }
    Ok(bytes)
}

fn render_icon(bytes: &[u8], edge: i32) -> Result<Vec<u8>, String> {
    if bytes.starts_with(b"\x89PNG") {
        return crate::cover::render_image(bytes, edge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Not a PNG or SVG icon")?;
    if !text.contains("<svg") {
        return Err("Not a PNG or SVG icon".into());
    }
    let tree = resvg::usvg::Tree::from_str(text, &resvg::usvg::Options::default())
        .map_err(|error| error.to_string())?;
    let size = tree.size();
    // Icons are drawn at the size asked, larger or smaller than they say they are.
    let scale = edge.clamp(1, 1024) as f32 / size.width().max(size.height());
    let width = (size.width() * scale).round().max(1.0) as u32;
    let height = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or("Cannot allocate image")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|error| error.to_string())
}

pub(crate) struct RootEntry {
    pub(crate) name: String,
    pub(crate) is_file: bool,
    pub(crate) symlink_target: Option<String>,
}

/// What the image's top folder holds.
fn root_entries(image: &FilesystemReader<'_>) -> Vec<RootEntry> {
    image
        .files()
        .take(MAX_ENTRIES)
        .filter_map(|node| {
            let relative = node.fullpath.strip_prefix("/").ok()?;
            let mut parts = relative.components();
            let name = parts.next()?.as_os_str().to_str()?.to_owned();
            if parts.next().is_some() {
                return None;
            }
            let (is_file, symlink_target) = match &node.inner {
                InnerNode::File(_) => (true, None),
                InnerNode::Symlink(link) => (false, link.link.to_str().map(str::to_owned)),
                _ => (false, None),
            };
            Some(RootEntry {
                name,
                is_file,
                symlink_target,
            })
        })
        .collect()
}

pub(crate) fn dir_icon_candidates(entries: &[RootEntry]) -> Vec<String> {
    let Some(dir_icon) = entries.iter().find(|entry| entry.name == ".DirIcon") else {
        return Vec::new();
    };
    if dir_icon.is_file {
        return vec![dir_icon.name.clone()];
    }
    let Some(target) = &dir_icon.symlink_target else {
        return Vec::new();
    };
    if !target.starts_with('/') {
        return vec![target.trim_start_matches("./").to_owned()];
    }
    // AppImage builders leave absolute build-host targets; the basename still names the icon
    // beside the launcher.
    let Some(base) = target.rsplit('/').next() else {
        return Vec::new();
    };
    entries
        .iter()
        .find(|entry| entry.is_file && entry.name.eq_ignore_ascii_case(base))
        .map(|entry| vec![entry.name.clone()])
        .unwrap_or_default()
}

pub(crate) fn icon_name_candidates(entries: &[RootEntry], icon: &str) -> Vec<String> {
    if icon.contains('/') {
        return vec![icon.trim_start_matches('/').to_owned()];
    }
    entries
        .iter()
        .filter(|entry| {
            entry.is_file
                && entry
                    .name
                    .rsplit_once('.')
                    .is_some_and(|(stem, extension)| {
                        (stem.eq_ignore_ascii_case(icon) || entry.name.eq_ignore_ascii_case(icon))
                            && ICON_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
                    })
        })
        .map(|entry| entry.name.clone())
        .collect()
}

pub(crate) fn desktop_icon(contents: &[u8]) -> Option<String> {
    std::str::from_utf8(contents)
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("Icon=")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

/// Where the SquashFS image starts: after the last section the ELF runtime keeps in the file.
pub(crate) fn embedded_image_offset(reader: &mut (impl Read + Seek)) -> Option<u64> {
    let mut header = [0u8; 64];
    reader.read_exact(&mut header).ok()?;
    if header[..4] != *b"\x7fELF" {
        return None;
    }
    let class64 = match header[4] {
        1 => false,
        2 => true,
        _ => return None,
    };
    let big_endian = match header[5] {
        1 => false,
        2 => true,
        _ => return None,
    };
    let minimum = if class64 { 64 } else { 40 };
    let (shoff, shentsize, shnum) = if class64 {
        (
            uint(&header, 0x28, 8, big_endian),
            uint(&header, 0x3a, 2, big_endian),
            uint(&header, 0x3c, 2, big_endian),
        )
    } else {
        (
            uint(&header, 0x20, 4, big_endian),
            uint(&header, 0x2e, 2, big_endian),
            uint(&header, 0x30, 2, big_endian),
        )
    };
    if shoff == 0 || (shentsize as usize) < minimum {
        return None;
    }
    let mut section = vec![0u8; shentsize as usize];
    reader.seek(SeekFrom::Start(shoff)).ok()?;
    let mut count = shnum;
    if count == 0 {
        // Extended numbering keeps the real count in section 0's sh_size.
        reader.read_exact(&mut section).ok()?;
        count = section_header(&section, class64, big_endian).size;
        reader.seek(SeekFrom::Start(shoff)).ok()?;
    }
    if count == 0 || count > 4096 {
        return None;
    }
    let mut end = shoff.saturating_add(shentsize.saturating_mul(count));
    for _ in 0..count {
        reader.read_exact(&mut section).ok()?;
        let section = section_header(&section, class64, big_endian);
        if section.kind != SHT_NOBITS {
            end = end.max(section.offset.saturating_add(section.size));
        }
    }
    reader.seek(SeekFrom::Start(end)).ok()?;
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).ok()?;
    (magic == *SQUASHFS_MAGIC).then_some(end)
}

struct SectionHeader {
    kind: u32,
    offset: u64,
    size: u64,
}

fn section_header(bytes: &[u8], class64: bool, big_endian: bool) -> SectionHeader {
    if class64 {
        SectionHeader {
            kind: uint(bytes, 4, 4, big_endian) as u32,
            offset: uint(bytes, 24, 8, big_endian),
            size: uint(bytes, 32, 8, big_endian),
        }
    } else {
        SectionHeader {
            kind: uint(bytes, 4, 4, big_endian) as u32,
            offset: uint(bytes, 16, 4, big_endian),
            size: uint(bytes, 20, 4, big_endian),
        }
    }
}

fn uint(bytes: &[u8], offset: usize, width: usize, big_endian: bool) -> u64 {
    let Some(field) = bytes.get(offset..offset + width) else {
        return 0;
    };
    if big_endian {
        field
            .iter()
            .fold(0, |value, byte| value << 8 | u64::from(*byte))
    } else {
        field
            .iter()
            .rev()
            .fold(0, |value, byte| value << 8 | u64::from(*byte))
    }
}
