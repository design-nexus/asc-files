// SPDX-License-Identifier: MIT

use std::io::Cursor;

use super::{Preview, previews, render};

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::from_pixel(width, height, image::Rgb([30, 120, 200]))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .unwrap();
    bytes
}

/// A TIFF-shaped RAW: IFD0 (orientation, a small JPEG by 0x201/0x202, a SubIFD), the SubIFD (a
/// large JPEG as its strip, compression 7), then the two JPEGs.
fn tiff_raw(little: bool, orientation: u16, small: &[u8], large: &[u8]) -> Vec<u8> {
    let w16 = |v: u16| {
        if little {
            v.to_le_bytes().to_vec()
        } else {
            v.to_be_bytes().to_vec()
        }
    };
    let w32 = |v: u32| {
        if little {
            v.to_le_bytes().to_vec()
        } else {
            v.to_be_bytes().to_vec()
        }
    };
    let ifd0 = 8u32;
    let ifd0_entries = 4u16;
    let sub = ifd0 + 2 + 12 * u32::from(ifd0_entries) + 4;
    let sub_entries = 3u16;
    let data = sub + 2 + 12 * u32::from(sub_entries) + 4;
    let small_at = data;
    let large_at = small_at + small.len() as u32;
    let entry = |tag: u16, kind: u16, count: u32, value: u32| {
        let mut e = Vec::new();
        e.extend(w16(tag));
        e.extend(w16(kind));
        e.extend(w32(count));
        if kind == 3 {
            e.extend(w16(value as u16));
            e.extend([0, 0]);
        } else {
            e.extend(w32(value));
        }
        e
    };
    let mut bytes = if little {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
    bytes.extend(w16(42));
    bytes.extend(w32(ifd0));
    bytes.extend(w16(ifd0_entries));
    bytes.extend(entry(0x0112, 3, 1, u32::from(orientation)));
    bytes.extend(entry(0x014A, 4, 1, sub));
    bytes.extend(entry(0x0201, 4, 1, small_at));
    bytes.extend(entry(0x0202, 4, 1, small.len() as u32));
    bytes.extend(w32(0));
    bytes.extend(w16(sub_entries));
    bytes.extend(entry(0x0103, 3, 1, 7));
    bytes.extend(entry(0x0111, 4, 1, large_at));
    bytes.extend(entry(0x0117, 4, 1, large.len() as u32));
    bytes.extend(w32(0));
    assert_eq!(bytes.len() as u32, data);
    bytes.extend(small);
    bytes.extend(large);
    bytes
}

fn size(png: &[u8]) -> (u32, u32) {
    let picture = image::load_from_memory(png).unwrap();
    (picture.width(), picture.height())
}

fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), bytes).unwrap();
    file
}

#[test]
fn every_preview_is_found_in_either_byte_order() {
    let (small, large) = (jpeg(16, 12), jpeg(64, 48));
    for little in [true, false] {
        let raw = tiff_raw(little, 1, &small, &large);
        let (found, orientation) = previews(&raw);
        assert_eq!(orientation, 1);
        assert_eq!(found.len(), 2, "little-endian {little}");
        assert!(found.iter().any(|p| p.length == large.len() as u64));
    }
}

#[test]
fn the_largest_preview_is_drawn_upright() {
    let raw = tiff_raw(true, 6, &jpeg(16, 12), &jpeg(64, 48));
    // Orientation 6: turned a quarter, so it stands taller than wide.
    assert_eq!(size(&render(file(&raw).path(), 256).unwrap()), (48, 64));
    let plain = tiff_raw(false, 1, &jpeg(16, 12), &jpeg(64, 48));
    assert_eq!(size(&render(file(&plain).path(), 32).unwrap()), (32, 24));
}

#[test]
fn raf_and_cr3_keep_theirs_elsewhere() {
    let picture = jpeg(40, 30);
    let mut raf = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
    raf.resize(84, 0);
    raf.extend(100u32.to_be_bytes());
    raf.extend((picture.len() as u32).to_be_bytes());
    raf.resize(100, 0);
    raf.extend(&picture);
    assert_eq!(
        previews(&raf).0,
        vec![Preview {
            offset: 100,
            length: picture.len() as u64
        }]
    );
    assert_eq!(size(&render(file(&raf).path(), 256).unwrap()), (40, 30));

    let mut cr3 = vec![0, 0, 0, 24];
    cr3.extend(b"ftypcrx ");
    cr3.resize(64, 0);
    cr3.extend(b"....PRVW....");
    cr3.extend(&picture);
    assert_eq!(size(&render(file(&cr3).path(), 256).unwrap()), (40, 30));
}

#[test]
fn what_is_not_a_preview_is_passed_over() {
    // Pointing past the end, or at something that isn't a JPEG.
    let mut raw = tiff_raw(true, 1, &jpeg(8, 8), &jpeg(16, 16));
    let len = raw.len();
    raw.truncate(len - 10);
    assert_eq!(previews(&raw).0.len(), 1);
    assert!(previews(b"not a raw file at all").0.is_empty());
    assert!(previews(b"II\x2a\x00\xff\xff\xff\xff").0.is_empty());
    assert!(render(file(b"II*\0\x08\0\0\0\0\0").path(), 64).is_err());
    // An IFD that points at itself is read once.
    let mut looped = b"II*\0\x08\0\0\0".to_vec();
    looped.extend(1u16.to_le_bytes());
    looped.extend(0x014Au16.to_le_bytes());
    looped.extend(4u16.to_le_bytes());
    looped.extend(1u32.to_le_bytes());
    looped.extend(8u32.to_le_bytes());
    looped.extend(8u32.to_le_bytes());
    assert!(previews(&looped).0.is_empty());
}
