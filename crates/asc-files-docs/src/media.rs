// SPDX-License-Identifier: MIT
//! Diagrams and equations in Markdown, drawn as pictures: legacy's renderer
//! (`sandbox_helper/document_media.rs`). SVG is drawn by resvg with every outside reference
//! (embedded or linked pictures) refused.

use resvg::{tiny_skia, usvg};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
};

/// A Mermaid diagram's source.
pub(crate) const DIAGRAM_INPUT_LIMIT: usize = 16 * 1024;
/// A TeX equation's source.
pub(crate) const MATH_INPUT_LIMIT: usize = 4096;

/// Whether `text` is a colour as `#rrggbb`.
pub(crate) fn is_colour(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// A Mermaid diagram, drawn without a browser, as a PNG, on a light card.
pub(crate) fn mermaid(source: &str) -> Result<Vec<u8>, String> {
    if source.len() > DIAGRAM_INPUT_LIMIT || source.lines().count() > 256 {
        return Err("Mermaid preview limit exceeded".into());
    }
    let source = mermaid_svg::render(source).map_err(|error| error.to_string())?;
    if source.len() > 2 * 1024 * 1024 {
        return Err("Mermaid output limit exceeded".into());
    }
    svg_on(&source, 800, Some(CARD)).map(|rendered| rendered.png)
}

/// A TeX equation (MathJax's base and AMS packages, in QuickJS with no host bindings), as a PNG.
/// `ink`: the colour it is drawn in (`#rrggbb`), the text's where it shows.
pub(crate) fn math(source: &str, display: bool, ink: &str) -> Result<Vec<u8>, String> {
    if source.len() > MATH_INPUT_LIMIT {
        return Err("Equation input limit exceeded".into());
    }
    let runtime = rquickjs::Runtime::new().map_err(|error| error.to_string())?;
    runtime.set_memory_limit(64 * 1024 * 1024);
    runtime.set_max_stack_size(1024 * 1024);
    let started = std::time::Instant::now();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        started.elapsed() > std::time::Duration::from_secs(2)
    })));
    let context = rquickjs::Context::full(&runtime).map_err(|error| error.to_string())?;
    let result = context
        .with(|ctx| -> rquickjs::Result<String> {
            // Only our bundled program is evaluated. TeX is passed as a string argument,
            // with no host functions, module loader, filesystem, or network bindings.
            ctx.eval::<(), _>(include_str!("../data/math-renderer.js"))?;
            let render: rquickjs::Function = ctx.globals().get("strataMath")?;
            render.call((source, display))
        })
        .map_err(|_| "Invalid or unsupported LaTeX equation".to_owned())?;
    if result.len() > 2 * 1024 * 1024 {
        return Err("Equation output limit exceeded".into());
    }
    // MathJax draws in the text's colour, which an SVG on its own takes as black.
    let ink = if is_colour(ink) { ink } else { "#000000" };
    svg(&result.replace("currentColor", ink), 800).map(|rendered| rendered.png)
}

fn svg_options() -> usvg::Options<'static> {
    usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    }
}

#[cfg(test)]
pub(crate) fn svg_dimensions(source: &str) -> Option<(i32, i32)> {
    let tree = usvg::Tree::from_str(source, &svg_options()).ok()?;
    let size = tree.size();
    Some((size.width().round() as i32, size.height().round() as i32))
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the native size is read by tests")
)]
pub(crate) struct RenderedSvg {
    pub png: Vec<u8>,
    pub width: i32,
    pub height: i32,
}

pub(crate) fn svg(source: &str, edge: u32) -> Result<RenderedSvg, String> {
    svg_on(source, edge, None)
}

/// A diagram's light card: Mermaid's colours are made for a light page.
const CARD: (u8, u8, u8) = (0xf4, 0xf4, 0xf7);
const CARD_MARGIN: u32 = 12;

/// `svg`, on a card of `background` with a margin, when one is given.
pub(crate) fn svg_on(
    source: &str,
    edge: u32,
    background: Option<(u8, u8, u8)>,
) -> Result<RenderedSvg, String> {
    let mut options = svg_options();
    if source.contains("<text") {
        load_text_fonts(source, &mut options);
        // Without fontconfig integration, fontdb's generic aliases can name missing
        // fonts. Resolve them to installed families before converting SVG text.
        let families: Vec<String> = options
            .fontdb
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect();
        let fallback = |preferred: &str| {
            families
                .iter()
                .find(|name| name.as_str() == preferred)
                .or_else(|| families.first())
                .cloned()
                .unwrap_or_else(|| preferred.to_owned())
        };
        let sans = fallback("DejaVu Sans");
        options.font_family = sans.clone();
        options.fontdb_mut().set_sans_serif_family(sans);
        options
            .fontdb_mut()
            .set_serif_family(fallback("DejaVu Serif"));
        options
            .fontdb_mut()
            .set_monospace_family(fallback("DejaVu Sans Mono"));
    }
    let tree = usvg::Tree::from_str(source, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    let edge = edge.clamp(1, 800) as f32;
    let scale = (edge / size.width().max(size.height())).min(1.0);
    let width = (size.width() * scale).ceil().clamp(1.0, edge) as u32;
    let height = (size.height() * scale).ceil().clamp(1.0, edge) as u32;
    let margin = if background.is_some() { CARD_MARGIN } else { 0 };
    let mut pixmap = tiny_skia::Pixmap::new(width + 2 * margin, height + 2 * margin)
        .ok_or("Cannot allocate image")?;
    if let Some((red, green, blue)) = background {
        pixmap.fill(tiny_skia::Color::from_rgba8(red, green, blue, 255));
    }
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale).post_translate(margin as f32, margin as f32),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png().map_err(|error| error.to_string())?;
    Ok(RenderedSvg {
        png,
        width: size.width().round() as i32,
        height: size.height().round() as i32,
    })
}

const FONT_FILE_LIMIT: usize = 32;
const FONT_LIST_LIMIT: u64 = 4 * 1024 * 1024;
const FONT_FAMILY_LIMIT: usize = 16;

const GENERIC_FAMILIES: &[&[&str]] = &[
    &[
        "DejaVu Sans",
        "Liberation Sans",
        "Noto Sans",
        "FreeSans",
        "Arial",
    ],
    &[
        "DejaVu Serif",
        "Liberation Serif",
        "Noto Serif",
        "FreeSerif",
        "Times New Roman",
    ],
    &[
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "FreeMono",
        "Courier New",
    ],
];

// Escaped non-ASCII glyphs need the full font fallback scan even in ASCII sources.
fn needs_full_font_scan(source: &str) -> bool {
    !source.is_ascii() || source.contains("&#")
}

fn load_text_fonts(source: &str, options: &mut usvg::Options) {
    if !needs_full_font_scan(source)
        && let Some(paths) = resolve_font_files(source)
    {
        for path in paths {
            let _ = options.fontdb_mut().load_font_file(path);
        }
    }
    if options.fontdb.faces().next().is_none() {
        options.fontdb_mut().load_system_fonts();
    }
}

fn resolve_font_files(source: &str) -> Option<Vec<PathBuf>> {
    // fc-list may be missing (in the sandbox, say): then the system's fonts are scanned.
    let output = Command::new("fc-list")
        .args(["--format", "%{file}\t%{family}\n"])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() as u64 > FONT_LIST_LIMIT {
        return None;
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    let mut available: HashMap<String, PathBuf> = HashMap::new();
    for line in listing.lines() {
        let Some((file, families)) = line.split_once('\t') else {
            continue;
        };
        for family in families.split(',') {
            let family = family.trim().to_lowercase();
            if !family.is_empty() {
                available
                    .entry(family)
                    .or_insert_with(|| PathBuf::from(file));
            }
        }
    }
    if available.is_empty() {
        return None;
    }
    let mut wanted = svg_font_families(source);
    for slot in GENERIC_FAMILIES {
        if let Some(name) = slot
            .iter()
            .find(|name| available.contains_key(&name.to_lowercase()))
        {
            wanted.push((*name).to_owned());
        }
    }
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for name in wanted {
        if paths.len() >= FONT_FILE_LIMIT {
            break;
        }
        if let Some(path) = available.get(&name.to_lowercase())
            && seen.insert(path)
        {
            paths.push(path.clone());
        }
    }
    (!paths.is_empty()).then_some(paths)
}

fn svg_font_families(source: &str) -> Vec<String> {
    let mut families = Vec::new();
    for (index, _) in source.match_indices("font-family") {
        let mut rest = source[index + "font-family".len()..]
            .trim_start_matches(|c: char| c == '=' || c == ':' || c.is_whitespace());
        loop {
            let (value, quoted_end) = match rest.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let inner = &rest[quote.len_utf8()..];
                    let end = inner.find(quote).unwrap_or(inner.len());
                    (&inner[..end], end + 2 * quote.len_utf8())
                }
                _ => {
                    let end = rest
                        .find(['"', '\'', ';', '<', '>', '/', '}', ')'])
                        .unwrap_or(rest.len());
                    (&rest[..end], end)
                }
            };
            for name in value.split(',') {
                let name = name.trim().trim_matches(['"', '\'']);
                if (1..=128).contains(&name.len()) {
                    families.push(name.to_owned());
                }
            }
            let after =
                rest[quoted_end.min(rest.len())..].trim_start_matches(|c: char| c.is_whitespace());
            if let Some(tail) = after.strip_prefix(',') {
                rest = tail;
                continue;
            }
            break;
        }
        if families.len() >= FONT_FAMILY_LIMIT {
            families.truncate(FONT_FAMILY_LIMIT);
            break;
        }
    }
    families
}

#[cfg(test)]
mod tests;
