// SPDX-License-Identifier: MIT
//! A camera RAW's thumbnail: the largest JPEG preview the camera put in the file, decoded within
//! the pixel limit and turned upright. Most RAWs are TIFF containers (CR2, NEF, ARW, DNG, ORF, PEF,
//! RW2, …) whose previews hang off their IFDs and sub-IFDs; RAF says where its JPEG is in its
//! header; CR3 keeps one in a `PRVW` box. Nothing is demosaiced. Legacy ran dcraw and
//! ImageMagick for this.

use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[cfg(test)]
mod tests;

/// Bytes of the file read: previews sit near its start.
const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IFDS: usize = 64;
const MAX_ENTRIES: u16 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Preview {
    pub offset: u64,
    pub length: u64,
}

pub(crate) fn render(input: &Path, edge: i32) -> Result<Vec<u8>, String> {
    let file = fs::File::open(input).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_READ_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let (mut previews, orientation) = previews(&bytes);
    if previews.is_empty() {
        return Err("No preview inside the RAW file".into());
    }
    // The largest that decodes within the limits.
    previews.sort_by(|a, b| b.length.cmp(&a.length));
    let mut last = String::new();
    for preview in previews {
        let end = (preview.offset + preview.length).min(bytes.len() as u64) as usize;
        match crate::cover::render_oriented(&bytes[preview.offset as usize..end], edge, orientation)
        {
            Ok(png) => return Ok(png),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// The JPEG previews in a RAW's bytes, and the orientation its first IFD gives.
pub(crate) fn previews(bytes: &[u8]) -> (Vec<Preview>, u16) {
    let mut found = Vec::new();
    let mut orientation = 1;
    if bytes.starts_with(b"FUJIFILMCCD-RAW") {
        // RAF: the JPEG's offset and length, big-endian, at 84 and 88.
        if let (Some(offset), Some(length)) = (be32(bytes, 84), be32(bytes, 88)) {
            push(bytes, &mut found, u64::from(offset), u64::from(length));
        }
        return (found, orientation);
    }
    if bytes.get(4..8) == Some(b"ftyp") {
        // CR3 (ISO media): the PRVW box holds a JPEG; read up to its end marker.
        if let Some(at) = find(bytes, b"PRVW")
            && let Some(start) = find(&bytes[at..], &[0xFF, 0xD8, 0xFF]).map(|n| at + n)
        {
            push(
                bytes,
                &mut found,
                start as u64,
                (bytes.len() - start) as u64,
            );
        }
        return (found, orientation);
    }
    let Some(tiff) = Tiff::new(bytes) else {
        return (found, orientation);
    };
    let mut queue = vec![tiff.first];
    let mut seen = Vec::new();
    while let Some(ifd) = queue.pop() {
        if ifd == 0 || seen.contains(&ifd) || seen.len() >= MAX_IFDS {
            continue;
        }
        seen.push(ifd);
        let Some(entries) = tiff.entries(ifd) else {
            continue;
        };
        let (mut jpeg_offset, mut jpeg_length) = (None, None);
        let (mut strip, mut strip_length, mut compression) = (None, None, None);
        for entry in &entries {
            match entry.tag {
                0x0112 if seen.len() == 1 => orientation = tiff.value(entry, 0).unwrap_or(1) as u16,
                0x0201 => jpeg_offset = tiff.value(entry, 0),
                0x0202 => jpeg_length = tiff.value(entry, 0),
                0x0103 => compression = tiff.value(entry, 0),
                0x0111 if entry.count == 1 => strip = tiff.value(entry, 0),
                0x0117 if entry.count == 1 => strip_length = tiff.value(entry, 0),
                // SubIFDs, and the EXIF IFD (where some cameras keep theirs).
                0x014A | 0x8769 => {
                    for index in 0..entry.count.min(16) {
                        if let Some(child) = tiff.value(entry, index) {
                            queue.push(child);
                        }
                    }
                }
                _ => {}
            }
        }
        if let (Some(offset), Some(length)) = (jpeg_offset, jpeg_length) {
            push(bytes, &mut found, offset, length);
        }
        // A strip that is itself a JPEG (old-style 6, or 7 for the full-size preview).
        if let (Some(offset), Some(length), Some(6 | 7)) = (strip, strip_length, compression) {
            push(bytes, &mut found, offset, length);
        }
        if let Some(next) = tiff.next(ifd, entries.len()) {
            queue.push(next);
        }
    }
    (found, orientation)
}

/// Keeps a preview that is within the bytes and starts as a JPEG.
fn push(bytes: &[u8], found: &mut Vec<Preview>, offset: u64, length: u64) {
    let Some(end) = offset.checked_add(length) else {
        return;
    };
    if length < 4
        || end > bytes.len() as u64
        || bytes[offset as usize..].get(..2) != Some(&[0xFF, 0xD8])
    {
        return;
    }
    let preview = Preview { offset, length };
    if !found.contains(&preview) {
        found.push(preview);
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    /// Where the entry's value field is.
    at: usize,
}

struct Tiff<'a> {
    bytes: &'a [u8],
    little: bool,
    first: u64,
}

impl<'a> Tiff<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        let little = match bytes.get(..2)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        let mut tiff = Self {
            bytes,
            little,
            first: 0,
        };
        // 42 for TIFF; Olympus (ORF) and Panasonic (RW2) mark theirs otherwise.
        if !matches!(tiff.u16(2)?, 42 | 0x4F52 | 0x5352 | 0x0055) {
            return None;
        }
        tiff.first = u64::from(tiff.u32(4)?);
        Some(tiff)
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let field: [u8; 2] = self.bytes.get(at..at + 2)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(field)
        } else {
            u16::from_be_bytes(field)
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let field: [u8; 4] = self.bytes.get(at..at + 4)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(field)
        } else {
            u32::from_be_bytes(field)
        })
    }

    fn entries(&self, ifd: u64) -> Option<Vec<Entry>> {
        let at = usize::try_from(ifd).ok()?;
        let count = self.u16(at)?;
        if count == 0 || count > MAX_ENTRIES {
            return None;
        }
        (0..usize::from(count))
            .map(|index| {
                let entry = at + 2 + index * 12;
                Some(Entry {
                    tag: self.u16(entry)?,
                    kind: self.u16(entry + 2)?,
                    count: self.u32(entry + 4)?,
                    at: entry + 8,
                })
            })
            .collect()
    }

    fn next(&self, ifd: u64, entries: usize) -> Option<u64> {
        let next = self.u32(usize::try_from(ifd).ok()? + 2 + entries * 12)?;
        (next != 0).then_some(u64::from(next))
    }

    /// The `index`th value of a SHORT, LONG or IFD entry.
    fn value(&self, entry: &Entry, index: u32) -> Option<u64> {
        let width = match entry.kind {
            3 => 2,
            4 | 13 => 4,
            _ => return None,
        };
        if index >= entry.count {
            return None;
        }
        let total = width * entry.count as usize;
        // Values that fit in four bytes are in the entry; the rest where it points.
        let base = if total <= 4 {
            entry.at
        } else {
            self.u32(entry.at)? as usize
        };
        let at = base + width * index as usize;
        if width == 2 {
            self.u16(at).map(u64::from)
        } else {
            self.u32(at).map(u64::from)
        }
    }
}
