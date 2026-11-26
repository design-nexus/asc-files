// SPDX-License-Identifier: MIT
//! 3D models drawn for their thumbnails: an STL's triangles, or a 3MF's that carries no picture of
//! itself, shaded from above at the front left on a clear background. Legacy's
//! `sandbox_helper/model.rs` (its parsers, limits and rasteriser), for thumbnails only: one view,
//! one palette, since thumbnails are kept across themes.

use std::{collections::HashMap, fs, io::Read, path::Path};

use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use resvg::tiny_skia::Pixmap;

#[cfg(test)]
mod tests;

pub(crate) const MAX_MODEL_INPUT_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_MODEL_XML_BYTES: u64 = 128 * 1024 * 1024;
const MAX_3MF_OBJECTS: usize = 1024;
const MAX_3MF_BUILD_ITEMS: usize = 1024;
const MAX_3MF_COMPONENT_DEPTH: usize = 16;
pub(crate) const MAX_MODEL_TRIANGLES: usize = 2_000_000;
const MAX_MODEL_VERTICES: usize = 2_000_000;
pub(crate) const MAX_MODEL_COMPONENT_REFERENCES: usize = 100_000;
const MAX_MODEL_COMPONENT_EXPANSIONS: usize = 100_000;
const MAX_MODEL_RASTER_WORK: u64 = 100_000_000;

/// The model's colour, lit; its shadows lean toward black.
const MODEL_COLOUR: [u8; 3] = [118, 150, 196];

type Point = [f32; 3];
type Matrix = [f32; 12];
const IDENTITY: Matrix = [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.];

#[derive(Default)]
struct Object {
    vertices: Vec<Point>,
    triangles: Vec<[usize; 3]>,
    components: Vec<(u32, Matrix)>,
}

/// An STL file drawn.
pub(crate) fn render_stl(input: &Path, edge: i32) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(input)
        .map_err(|error| error.to_string())?
        .take(MAX_MODEL_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_MODEL_INPUT_BYTES {
        return Err("Model exceeds the input size limit".into());
    }
    let faces = stl(&bytes)?;
    drop(bytes);
    shade(&faces, edge.clamp(16, 1024) as u32)
}

/// A 3MF package's model drawn: the part its relationships name (else `3D/3dmodel.model`).
pub(crate) fn render_3mf(
    archive: &mut zip::ZipArchive<fs::File>,
    edge: i32,
) -> Result<Vec<u8>, String> {
    let part = root_model(archive).unwrap_or_else(|| "3D/3dmodel.model".to_owned());
    let file = archive
        .by_name(part.trim_start_matches('/'))
        .map_err(|_| "3MF package has no model")?;
    if file.size() > MAX_MODEL_XML_BYTES {
        return Err("The unpacked 3MF model exceeds the limit".into());
    }
    let mut xml = Vec::new();
    file.take(MAX_MODEL_XML_BYTES + 1)
        .read_to_end(&mut xml)
        .map_err(|_| "Invalid 3MF model")?;
    let faces = triangles_3mf(&xml)?;
    drop(xml);
    shade(&faces, edge.clamp(16, 1024) as u32)
}

/// The model part the package's root relationships point at.
fn root_model(archive: &mut zip::ZipArchive<fs::File>) -> Option<String> {
    let file = archive.by_name("_rels/.rels").ok()?;
    let mut xml = Vec::new();
    file.take(64 * 1024).read_to_end(&mut xml).ok()?;
    let mut reader = Reader::from_reader(xml.as_slice());
    loop {
        match reader.read_event().ok()? {
            Event::Start(tag) | Event::Empty(tag)
                if tag.local_name().as_ref() == b"Relationship" =>
            {
                let kind = attribute(&tag, b"Type").ok()??;
                if kind.ends_with("/3dmodel") {
                    return attribute(&tag, b"Target").ok()?;
                }
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

fn attribute(tag: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>, String> {
    for attr in tag.attributes() {
        let attr = attr.map_err(|_| "Invalid 3MF attribute")?;
        if attr.key.local_name().as_ref() == name {
            return String::from_utf8(attr.value.into_owned())
                .map(Some)
                .map_err(|_| "Invalid 3MF attribute".into());
        }
    }
    Ok(None)
}

fn number(tag: &BytesStart<'_>, name: &[u8]) -> Result<f32, String> {
    let value = attribute(tag, name)?.ok_or("Missing 3MF coordinate")?;
    let number: f32 = value.parse().map_err(|_| "Invalid 3MF coordinate")?;
    number
        .is_finite()
        .then_some(number)
        .ok_or("Invalid 3MF coordinate".into())
}

fn index(tag: &BytesStart<'_>, name: &[u8]) -> Result<u32, String> {
    attribute(tag, name)?
        .ok_or("Missing 3MF index")?
        .parse()
        .map_err(|_| "Invalid 3MF index".into())
}

fn matrix(value: Option<String>) -> Result<Matrix, String> {
    let Some(value) = value else {
        return Ok(IDENTITY);
    };
    let parts: Vec<f32> = value
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| "Invalid 3MF transform")?;
    if parts.len() != 12 || parts.iter().any(|part| !part.is_finite()) {
        return Err("Invalid 3MF transform".into());
    }
    parts.try_into().map_err(|_| "Invalid 3MF transform".into())
}

fn transform(m: Matrix, p: Point) -> Point {
    [
        m[0] * p[0] + m[3] * p[1] + m[6] * p[2] + m[9],
        m[1] * p[0] + m[4] * p[1] + m[7] * p[2] + m[10],
        m[2] * p[0] + m[5] * p[1] + m[8] * p[2] + m[11],
    ]
}

fn combine(a: Matrix, b: Matrix) -> Matrix {
    let origin = transform(a, transform(b, [0., 0., 0.]));
    let mut result = [0.; 12];
    for axis in 0..3 {
        let mut basis = [0.; 3];
        basis[axis] = 1.;
        let point = transform(a, transform(b, basis));
        for component in 0..3 {
            result[axis * 3 + component] = point[component] - origin[component];
        }
    }
    result[9..].copy_from_slice(&origin);
    result
}

pub(crate) fn triangles_3mf(xml: &[u8]) -> Result<Vec<[Point; 3]>, String> {
    let mut reader = Reader::from_reader(xml);
    let mut objects = HashMap::<u32, Object>::new();
    let mut current = None;
    let mut items = Vec::new();
    let mut vertices = 0;
    let mut triangles = 0;
    let mut components = 0;
    loop {
        let event = reader.read_event().map_err(|_| "Invalid 3MF model XML")?;
        match event {
            Event::Start(tag) | Event::Empty(tag) => match tag.local_name().as_ref() {
                b"object" => {
                    let id = index(&tag, b"id")?;
                    if objects.len() >= MAX_3MF_OBJECTS
                        || objects.insert(id, Object::default()).is_some()
                    {
                        return Err("3MF object limit exceeded".into());
                    }
                    current = Some(id);
                }
                b"vertex" if current.is_some() => {
                    vertices += 1;
                    if vertices > MAX_MODEL_VERTICES {
                        return Err("3MF vertex limit exceeded".into());
                    }
                    let point = [
                        number(&tag, b"x")?,
                        number(&tag, b"y")?,
                        number(&tag, b"z")?,
                    ];
                    objects
                        .get_mut(&current.unwrap_or_default())
                        .ok_or("Invalid 3MF object")?
                        .vertices
                        .push(point);
                }
                b"triangle" if current.is_some() => {
                    triangles += 1;
                    if triangles > MAX_MODEL_TRIANGLES {
                        return Err("Model triangle limit exceeded".into());
                    }
                    let indices = [
                        index(&tag, b"v1")? as usize,
                        index(&tag, b"v2")? as usize,
                        index(&tag, b"v3")? as usize,
                    ];
                    objects
                        .get_mut(&current.unwrap_or_default())
                        .ok_or("Invalid 3MF object")?
                        .triangles
                        .push(indices);
                }
                b"component" if current.is_some() => {
                    components += 1;
                    if components > MAX_MODEL_COMPONENT_REFERENCES {
                        return Err("3MF component reference limit exceeded".into());
                    }
                    if attribute(&tag, b"path")?.is_some() {
                        return Err("Multipart model".into());
                    }
                    let id = index(&tag, b"objectid")?;
                    let matrix = matrix(attribute(&tag, b"transform")?)?;
                    objects
                        .get_mut(&current.unwrap_or_default())
                        .ok_or("Invalid 3MF object")?
                        .components
                        .push((id, matrix));
                }
                b"item" => {
                    if attribute(&tag, b"path")?.is_some() {
                        return Err("Multipart model".into());
                    }
                    if items.len() >= MAX_3MF_BUILD_ITEMS {
                        return Err("3MF build item limit exceeded".into());
                    }
                    items.push((
                        index(&tag, b"objectid")?,
                        matrix(attribute(&tag, b"transform")?)?,
                    ));
                }
                _ => {}
            },
            Event::End(tag) if tag.local_name().as_ref() == b"object" => current = None,
            Event::Eof => break,
            _ => {}
        }
    }
    let mut faces = Vec::new();
    let mut stack: Vec<_> = items.into_iter().map(|(id, m)| (id, m, 0)).collect();
    let mut expanded = 0;
    while let Some((id, matrix, depth)) = stack.pop() {
        expanded += 1;
        if expanded > MAX_MODEL_COMPONENT_EXPANSIONS {
            return Err("3MF component expansion limit exceeded".into());
        }
        if depth > MAX_3MF_COMPONENT_DEPTH {
            return Err("3MF component nesting limit exceeded".into());
        }
        let object = objects.get(&id).ok_or("3MF references a missing object")?;
        if expanded + stack.len() + object.components.len() > MAX_MODEL_COMPONENT_EXPANSIONS {
            return Err("3MF component expansion limit exceeded".into());
        }
        for &(id, child) in &object.components {
            stack.push((id, combine(matrix, child), depth + 1));
        }
        for indices in &object.triangles {
            let [Some(a), Some(b), Some(c)] =
                indices.map(|index| object.vertices.get(index).copied())
            else {
                return Err("3MF triangle references a missing vertex".into());
            };
            if faces.len() >= MAX_MODEL_TRIANGLES {
                return Err("Model triangle limit exceeded".into());
            }
            faces.push([
                transform(matrix, a),
                transform(matrix, b),
                transform(matrix, c),
            ]);
        }
    }
    if faces.is_empty() {
        return Err("3MF contains no printable triangles".into());
    }
    Ok(faces)
}

/// An STL's triangles, binary or text.
pub(crate) fn stl(bytes: &[u8]) -> Result<Vec<[Point; 3]>, String> {
    if bytes.len() >= 84 {
        let count =
            u32::from_le_bytes(bytes[80..84].try_into().map_err(|_| "Invalid STL")?) as usize;
        // Binary when the count fits the size exactly, even after a "solid" header.
        if count.checked_mul(50).and_then(|n| n.checked_add(84)) == Some(bytes.len()) {
            if count > MAX_MODEL_TRIANGLES {
                return Err("Model triangle limit exceeded".into());
            }
            let mut faces = Vec::with_capacity(count);
            for face in bytes[84..].chunks_exact(50) {
                let mut points = [[0.; 3]; 3];
                for (vertex, point) in points.iter_mut().enumerate() {
                    for (axis, coordinate) in point.iter_mut().enumerate() {
                        let start = 12 + vertex * 12 + axis * 4;
                        *coordinate = f32::from_le_bytes(
                            face[start..start + 4]
                                .try_into()
                                .map_err(|_| "Invalid STL")?,
                        );
                        if !coordinate.is_finite() {
                            return Err("Invalid STL coordinate".into());
                        }
                    }
                }
                faces.push(points);
            }
            return Ok(faces);
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid STL")?;
    if !text.trim_start().starts_with("solid") {
        return Err("Invalid STL".into());
    }
    let mut faces = Vec::new();
    let mut points = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("vertex") {
            continue;
        }
        let coordinate = |field: Option<&str>| -> Result<f32, String> {
            let value: f32 = field
                .ok_or("Invalid STL vertex")?
                .parse()
                .map_err(|_| "Invalid STL vertex")?;
            value
                .is_finite()
                .then_some(value)
                .ok_or("Invalid STL coordinate".into())
        };
        let point = [
            coordinate(fields.next())?,
            coordinate(fields.next())?,
            coordinate(fields.next())?,
        ];
        points.push(point);
        if points.len() == 3 {
            if faces.len() >= MAX_MODEL_TRIANGLES {
                return Err("Model triangle limit exceeded".into());
            }
            faces.push([points[0], points[1], points[2]]);
            points.clear();
        }
    }
    if !points.is_empty() || faces.is_empty() {
        return Err("STL contains no valid triangles".into());
    }
    Ok(faces)
}

/// The faces drawn into a square `side` across: an isometric view from the front left, lit from
/// above, nearer faces over farther ones.
pub(crate) fn shade(faces: &[[Point; 3]], side: u32) -> Result<Vec<u8>, String> {
    let (width, height) = (side, side);
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for face in faces {
        for p in face {
            for axis in 0..3 {
                if !p[axis].is_finite() {
                    return Err("Invalid transformed model coordinate".into());
                }
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
    }
    let span: Point = std::array::from_fn(|axis| max[axis] - min[axis]);
    let center: Point = std::array::from_fn(|axis| min[axis] + span[axis] * 0.5);
    let root_six = 6_f32.sqrt();
    let extent = ((span[0] + span[1]) * std::f32::consts::FRAC_1_SQRT_2)
        .max((span[0] + span[1] + 2. * span[2]) / root_six);
    if !extent.is_finite() || extent <= 0. {
        return Err("Model has no visible extent".into());
    }
    let scale = (side as f32 * 0.82) / extent;
    // The camera at the front left, above.
    let [sx, sy] = [-1_f32, -1.];
    let project = |face: [Point; 3]| {
        face.map(|p| {
            let [x, y, z] = std::array::from_fn(|axis| p[axis] - center[axis]);
            [
                (-sy * x + sx * y) * std::f32::consts::FRAC_1_SQRT_2,
                (-sx * x - sy * y + 2. * z) / root_six,
                (sx * x + sy * y + z) / 3_f32.sqrt(),
            ]
        })
    };
    let mut pixmap = Pixmap::new(width, height).ok_or("Cannot allocate model preview")?;
    let mut depths = vec![f32::NEG_INFINITY; (width * height) as usize];
    let mut work = 0u64;
    for face in faces {
        let points = project(*face);
        let a = [
            points[1][0] - points[0][0],
            points[1][1] - points[0][1],
            points[1][2] - points[0][2],
        ];
        let b = [
            points[2][0] - points[0][0],
            points[2][1] - points[0][1],
            points[2][2] - points[0][2],
        ];
        let normal = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let length = (normal.iter().map(|n| n * n).sum::<f32>()).sqrt();
        let light = if length > 0. {
            (0.58 + 0.36 * (normal[0] * 0.3 - normal[1] * 0.5 + normal[2] * 0.8).abs() / length)
                .clamp(0.35, 0.94)
        } else {
            0.58
        };
        let face = points.map(|point| {
            [
                width as f32 * 0.5 + point[0] * scale,
                height as f32 * 0.5 - point[1] * scale,
                point[2],
            ]
        });
        let edge = |a: Point, b: Point, p: Point| {
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
        };
        let area = edge(face[0], face[1], face[2]);
        if area.abs() < 1e-8 {
            continue;
        }
        let colour = MODEL_COLOUR.map(|channel| (f32::from(channel) * light) as u8);
        let colour = [colour[0], colour[1], colour[2], 255];
        let lower = |axis| {
            face.iter()
                .map(|p| p[axis])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.) as u32
        };
        let upper = |axis, bound| {
            face.iter()
                .map(|p| p[axis])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .max(0.)
                .min(bound as f32) as u32
        };
        let (left, right, top, bottom) = (lower(0), upper(0, width), lower(1), upper(1, height));
        work += u64::from(right.saturating_sub(left)) * u64::from(bottom.saturating_sub(top));
        if work > MAX_MODEL_RASTER_WORK {
            return Err("Model exceeds the rendering limit".into());
        }
        for y in top..bottom {
            for x in left..right {
                let point = [x as f32 + 0.5, y as f32 + 0.5, 0.];
                let weights = [
                    edge(face[1], face[2], point) / area,
                    edge(face[2], face[0], point) / area,
                    edge(face[0], face[1], point) / area,
                ];
                if weights.iter().any(|weight| *weight < 0.) {
                    continue;
                }
                let depth = weights
                    .iter()
                    .zip(face)
                    .map(|(weight, point)| weight * point[2])
                    .sum();
                let index = (y * width + x) as usize;
                if depth > depths[index] {
                    depths[index] = depth;
                    pixmap.data_mut()[index * 4..index * 4 + 4].copy_from_slice(&colour);
                }
            }
        }
    }
    pixmap.encode_png().map_err(|error| error.to_string())
}
