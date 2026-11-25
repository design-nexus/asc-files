// SPDX-License-Identifier: MIT
//! asc-files-docs: reads one document for Files' preview, inside the sandbox Files runs it in,
//! and writes it out as JSON, `{"html", "truncated", "media"}`: HTML in the subset Qt's rich text
//! shows, with nothing in it that reaches out (no scripts, styles, remote pictures or links that
//! load), and the PNGs it draws (Markdown's diagrams and equations, as `media:N`), base64.
//! Untrusted document readers never run in the app. Legacy's readers are ported here: Word
//! (`docx`), RTF (`rtf`) and spreadsheets (`table`); Markdown and HTML are read with
//! pulldown-cmark and cleaned with ammonia.
//!
//!     asc-files-docs markdown|html|rtf|docx|sheet|csv|tsv <file> [<text colour, #rrggbb>]
//!     asc-files-docs cover cbz|epub|3mf|fcstd <file> <edge>   (a cover for a thumbnail, as a PNG)
//!     asc-files-docs archive zip|7z|tar|tar.gz <file>   (an archive's listing, as JSON nodes)

mod archive;
mod cover;
mod docx;
mod html;
mod markdown;
mod media;
mod rtf;
mod table;

use std::{path::Path, process::ExitCode};

/// Text documents are read up to this much.
pub(crate) const INPUT_LIMIT: usize = 1024 * 1024;
/// What the HTML may grow to.
pub(crate) const MARKUP_LIMIT: usize = 4 * 1024 * 1024;

/// `text` with `&`, `<`, `>` and `"` written as entities.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(character),
        }
    }
    out
}

/// What the preview shows.
#[derive(Default)]
pub(crate) struct Rendered {
    pub html: String,
    pub truncated: bool,
    pub media: Vec<Vec<u8>>,
}

/// The first `INPUT_LIMIT` bytes of `path` as text (invalid UTF-8 replaced), and whether there
/// was more.
fn read_text(path: &Path) -> Result<(String, bool), String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(INPUT_LIMIT as u64 + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    let truncated = bytes.len() > INPUT_LIMIT;
    bytes.truncate(INPUT_LIMIT);
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

/// `ink`: the text's colour where the preview shows it, for what is drawn (equations).
pub(crate) fn render(kind: &str, path: &Path, ink: &str) -> Result<Rendered, String> {
    match kind {
        "markdown" => {
            let (text, truncated) = read_text(path)?;
            let markdown = markdown::to_html(&text, ink);
            Ok(Rendered {
                html: markdown.html,
                truncated,
                media: markdown.media,
            })
        }
        "html" => {
            let (text, truncated) = read_text(path)?;
            Ok(Rendered {
                html: html::clean(&text),
                truncated,
                ..Default::default()
            })
        }
        "rtf" => {
            let (text, truncated) = read_text(path)?;
            Ok(Rendered {
                html: rtf::to_html(&text)?,
                truncated,
                ..Default::default()
            })
        }
        "docx" => {
            let data = docx::read_document(path)?;
            Ok(Rendered {
                html: data.html,
                truncated: data.truncated,
                ..Default::default()
            })
        }
        "sheet" => {
            let table = table::read_workbook(path)?;
            Ok(Rendered {
                html: table.to_html(),
                truncated: table.truncated,
                ..Default::default()
            })
        }
        "csv" | "tsv" => {
            let (text, cut) = read_text(path)?;
            let table = table::parse_delimited(&text, if kind == "csv" { b',' } else { b'\t' })?;
            Ok(Rendered {
                html: table.to_html(),
                truncated: cut || table.truncated,
                ..Default::default()
            })
        }
        other => Err(format!("unknown kind {other}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    // An archive's listing: `archive <format> <file>`, as JSON.
    if let [_, mode, format, file] = args.as_slice()
        && mode == "archive"
    {
        println!("{}", archive::listing(format, Path::new(file)));
        return ExitCode::SUCCESS;
    }
    // A cover: `cover <cbz|epub|3mf|fcstd> <file> <edge>`, written out as a PNG.
    if let [_, mode, format, file, edge] = args.as_slice()
        && mode == "cover"
    {
        let Some(format) = cover::CoverFormat::from_name(format) else {
            eprintln!("unknown cover format {format}");
            return ExitCode::from(2);
        };
        let edge = edge.parse().unwrap_or(256).clamp(1, 1024);
        return match cover::render(Path::new(file), format, edge) {
            Ok(png) => {
                use std::io::Write as _;
                if std::io::stdout().write_all(&png).is_ok() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    let (kind, file, ink) = match args.as_slice() {
        [_, kind, file] => (kind, file, "#000000"),
        [_, kind, file, ink] => (kind, file, ink.as_str()),
        _ => {
            eprintln!(
                "usage: asc-files-docs markdown|html|rtf|docx|sheet|csv|tsv <file> [#rrggbb] | cover cbz|epub|3mf|fcstd <file> <edge> | archive <format> <file>"
            );
            return ExitCode::from(2);
        }
    };
    match render(kind, Path::new(file), ink) {
        Ok(rendered) => {
            use base64::Engine as _;
            let media: Vec<String> = rendered
                .media
                .iter()
                .map(|png| base64::engine::general_purpose::STANDARD.encode(png))
                .collect();
            println!(
                "{}",
                serde_json::json!({"html": rendered.html, "truncated": rendered.truncated, "media": media})
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
