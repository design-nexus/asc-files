// SPDX-License-Identifier: MIT
//! Covers for thumbnails, from inside the file: a comic's first page (CBZ), an e-book's declared
//! cover (EPUB), and the picture a 3MF or FreeCAD model carries of itself. Legacy's
//! `sandbox_helper/archive_cover.rs` and the embedded part of `model/embedded.rs`; pictures are
//! decoded by the `image` crate, within pixel limits.

use std::{fs, io::Read, path::Path};

use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoverFormat {
    Cbz,
    Epub,
    ThreeMf,
    FreeCad,
    AppImage,
    Raw,
    Stl,
}

impl CoverFormat {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "cbz" => Some(Self::Cbz),
            "epub" => Some(Self::Epub),
            "3mf" => Some(Self::ThreeMf),
            "fcstd" => Some(Self::FreeCad),
            "appimage" => Some(Self::AppImage),
            "raw" => Some(Self::Raw),
            "stl" => Some(Self::Stl),
            _ => None,
        }
    }
}

const MAX_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_XML_BYTES: u64 = 256 * 1024;
/// Pixels a cover may decode to.
const MAX_PIXELS: u64 = 16 * 1024 * 1024;

fn image_name(name: &str) -> bool {
    matches!(
        name.rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "gif"
    )
}

fn compare_names(left: &str, right: &str) -> std::cmp::Ordering {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let (mut l, mut r) = (0, 0);
    while l < left.len() && r < right.len() {
        if left[l].is_ascii_digit() && right[r].is_ascii_digit() {
            let start_l = l;
            let start_r = r;
            while l < left.len() && left[l].is_ascii_digit() {
                l += 1;
            }
            while r < right.len() && right[r].is_ascii_digit() {
                r += 1;
            }
            let a = left[start_l..l]
                .iter()
                .position(|c| *c != b'0')
                .map_or(l, |n| start_l + n);
            let b = right[start_r..r]
                .iter()
                .position(|c| *c != b'0')
                .map_or(r, |n| start_r + n);
            let order = (l - a)
                .cmp(&(r - b))
                .then_with(|| left[a..l].cmp(&right[b..r]));
            if !order.is_eq() {
                return order;
            }
        } else {
            let order = left[l]
                .to_ascii_lowercase()
                .cmp(&right[r].to_ascii_lowercase());
            if !order.is_eq() {
                return order;
            }
            l += 1;
            r += 1;
        }
    }
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

#[cfg(test)]
pub(crate) fn thumbnail(input: &Path, format: CoverFormat) -> Result<Vec<u8>, String> {
    render(input, format, 256)
}

pub(crate) fn render(input: &Path, format: CoverFormat, edge: i32) -> Result<Vec<u8>, String> {
    // An AppImage is often large; only its icon is read.
    if format == CoverFormat::AppImage {
        return crate::appimage::render(input, edge);
    }
    // A camera RAW's own preview.
    if format == CoverFormat::Raw {
        return crate::raw::render(input, edge);
    }
    // An STL drawn (it carries no picture).
    if format == CoverFormat::Stl {
        return crate::model::render_stl(input, edge);
    }
    if fs::metadata(input).map_err(|e| e.to_string())?.len() > MAX_INPUT_BYTES {
        return Err("Cover archive exceeds the input size limit".into());
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(input).map_err(|e| e.to_string())?)
        .map_err(|_| "Invalid cover archive")?;
    if archive.len() > MAX_ENTRIES {
        return Err("Cover archive entry limit exceeded".into());
    }
    let name = if matches!(format, CoverFormat::ThreeMf | CoverFormat::FreeCad) {
        match model_thumbnail(&mut archive, format) {
            Ok(name) => name,
            // A 3MF without one picture of itself is drawn from its geometry.
            Err(_) if format == CoverFormat::ThreeMf => {
                return crate::model::render_3mf(&mut archive, edge);
            }
            Err(error) => return Err(error),
        }
    } else if format == CoverFormat::Cbz {
        let mut first = None::<String>;
        for i in 0..archive.len() {
            let file = archive
                .by_index_raw(i)
                .map_err(|_| "Invalid cover archive")?;
            let name = file.name().map_err(|_| "Invalid cover archive")?;
            if !file.is_dir()
                && image_name(&name)
                && first
                    .as_deref()
                    .is_none_or(|first| compare_names(&name, first).is_lt())
            {
                first = Some(name.into_owned());
            }
        }
        first.ok_or("Comic archive has no bounded image")?
    } else {
        epub_cover(&mut archive)?
    };
    let bytes = read_member(&mut archive, &name, MAX_IMAGE_BYTES)?;
    render_image(&bytes, edge)
}

fn read_member(
    archive: &mut zip::ZipArchive<fs::File>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let file = archive
        .by_name(name)
        .map_err(|_| "Cover archive member is missing")?;
    if file.size() > limit {
        return Err("Cover archive member exceeds the size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Invalid cover archive member")?;
    if bytes.len() as u64 > limit {
        return Err("Cover archive member exceeds the size limit".into());
    }
    Ok(bytes)
}

fn attribute(tag: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    tag.attributes()
        .flatten()
        .find(|attr| attr.key.local_name().as_ref() == key)
        .and_then(|attr| {
            attr.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, tag.decoder())
                .ok()
                .map(|value| value.into_owned())
        })
}

fn rootfile(xml: &[u8]) -> Result<String, String> {
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader.read_event().map_err(|_| "Invalid EPUB container")? {
            Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == b"rootfile" => {
                return attribute(&tag, b"full-path").ok_or("EPUB has no rootfile".into());
            }
            Event::Eof => return Err("EPUB has no rootfile".into()),
            _ => {}
        }
    }
}

fn cover_href(xml: &[u8]) -> Result<String, String> {
    let mut reader = Reader::from_reader(xml);
    let mut cover_id = None;
    let mut items = Vec::new();
    loop {
        match reader.read_event().map_err(|_| "Invalid EPUB package")? {
            Event::Start(tag) | Event::Empty(tag) => match tag.local_name().as_ref() {
                b"meta" if attribute(&tag, b"name").as_deref() == Some("cover") => {
                    cover_id = attribute(&tag, b"content");
                }
                b"item" => {
                    if let (Some(id), Some(href), Some(media)) = (
                        attribute(&tag, b"id"),
                        attribute(&tag, b"href"),
                        attribute(&tag, b"media-type"),
                    ) {
                        items.push((id, href, media, attribute(&tag, b"properties")));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    let supported = |href: &str, media: &str| {
        image_name(href)
            && matches!(
                media,
                "image/jpeg" | "image/png" | "image/webp" | "image/gif"
            )
    };
    items
        .iter()
        .find(|(_, href, media, properties)| {
            supported(href, media)
                && properties
                    .as_deref()
                    .is_some_and(|value| value.split_whitespace().any(|p| p == "cover-image"))
        })
        .or_else(|| {
            items.iter().find(|(id, href, media, _)| {
                supported(href, media) && cover_id.as_deref() == Some(id.as_str())
            })
        })
        .map(|(_, href, _, _)| href.clone())
        .ok_or("EPUB has no cover image".into())
}

fn epub_cover(archive: &mut zip::ZipArchive<fs::File>) -> Result<String, String> {
    let container = read_member(archive, "META-INF/container.xml", MAX_XML_BYTES)?;
    let package_path = rootfile(&container)?;
    let package_path = normalize_path("", &package_path).ok_or("Invalid EPUB package path")?;
    let xml = read_member(archive, &package_path, MAX_XML_BYTES)?;
    let href = cover_href(&xml)?;
    let base = package_path.rsplit_once('/').map_or("", |(base, _)| base);
    let relative = normalize_path(base, &href).ok_or("Invalid EPUB cover path")?;
    // Some EPUB 2 generators place a root-relative href in a nested package document.
    if archive
        .file_names()
        .any(|name| name.is_ok_and(|name| name == relative))
    {
        return Ok(relative);
    }
    normalize_path("", &href)
        .filter(|name| {
            archive
                .file_names()
                .any(|entry| entry.is_ok_and(|entry| entry == *name))
        })
        .ok_or("EPUB cover is missing".into())
}

fn normalize_path(base: &str, href: &str) -> Option<String> {
    let href = unescape(href.split(['?', '#']).next()?)?;
    let href = href.as_str();
    if href.starts_with('/') || href.contains('\\') {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `%XX` escapes in a URI path, decoded; `None` when they don't make UTF-8.
fn unescape(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The picture a model package carries of itself: FreeCAD's `thumbnails/Thumbnail.png`; for 3MF
/// the one thumbnail part (by its usual name, `Metadata/thumbnail.png`, if there are several).
fn model_thumbnail(
    archive: &mut zip::ZipArchive<fs::File>,
    format: CoverFormat,
) -> Result<String, String> {
    let mut found = Vec::new();
    for name in archive.file_names().flatten() {
        let wanted = if format == CoverFormat::FreeCad {
            name == "thumbnails/Thumbnail.png"
        } else {
            name.to_ascii_lowercase().ends_with("thumbnail.png")
        };
        if wanted {
            found.push(name.into_owned());
        }
    }
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err("The model has no thumbnail".into()),
        many => many
            .iter()
            .find(|name| name.eq_ignore_ascii_case("Metadata/thumbnail.png"))
            .cloned()
            .ok_or("The model has more than one thumbnail".into()),
    }
}

/// `bytes`, a picture, decoded within the pixel limit and scaled to at most `edge` a side, as a PNG.
pub(crate) fn render_image(bytes: &[u8], edge: i32) -> Result<Vec<u8>, String> {
    render_oriented(bytes, edge, 1)
}

/// As `render_image`, turned upright by an EXIF `orientation` (1–8).
pub(crate) fn render_oriented(
    bytes: &[u8],
    edge: i32,
    orientation: u16,
) -> Result<Vec<u8>, String> {
    let reader = || -> Result<_, String> {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| "Invalid cover image")?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_PIXELS * 4);
        reader.limits(limits);
        Ok(reader)
    };
    // Its size first, from the header: nothing past the limit is decoded.
    let (width, height) = reader()?
        .into_dimensions()
        .map_err(|_| "Invalid cover image")?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("Cover image exceeds the pixel limit".into());
    }
    let picture = reader()?
        .decode()
        .map_err(|error| format!("Invalid cover image: {error}"))?;
    let edge = edge.max(1) as u32;
    let picture = if width > edge || height > edge {
        picture.resize(edge, edge, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let picture = match orientation {
        2 => picture.fliph(),
        3 => picture.rotate180(),
        4 => picture.flipv(),
        5 => picture.rotate90().fliph(),
        6 => picture.rotate90(),
        7 => picture.rotate270().fliph(),
        8 => picture.rotate270(),
        _ => picture,
    };
    let mut png = Vec::new();
    picture
        .to_rgba8()
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|_| "Cannot encode cover image")?;
    Ok(png)
}

#[cfg(test)]
mod tests;
