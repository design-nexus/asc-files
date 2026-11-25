// SPDX-License-Identifier: MIT
//! asc-files-docs: reads one document for Files' preview, inside the sandbox Files runs it in,
//! and writes it out as JSON, `{"html", "truncated"}`: HTML in the subset Qt's rich text shows,
//! with nothing in it that reaches out (no scripts, styles, remote pictures or links that load).
//! Untrusted document readers never run in the app. Legacy's readers are ported here: Word
//! (`docx`), RTF (`rtf`) and spreadsheets (`table`); Markdown and HTML are read with
//! pulldown-cmark and cleaned with ammonia.
//!
//!     asc-files-docs markdown|html|rtf|docx|sheet|csv|tsv <file>

mod docx;
mod html;
mod markdown;
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
pub(crate) struct Rendered {
    pub html: String,
    pub truncated: bool,
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

pub(crate) fn render(kind: &str, path: &Path) -> Result<Rendered, String> {
    match kind {
        "markdown" => {
            let (text, truncated) = read_text(path)?;
            Ok(Rendered {
                html: markdown::to_html(&text),
                truncated,
            })
        }
        "html" => {
            let (text, truncated) = read_text(path)?;
            Ok(Rendered {
                html: html::clean(&text),
                truncated,
            })
        }
        "rtf" => {
            let (text, truncated) = read_text(path)?;
            Ok(Rendered {
                html: rtf::to_html(&text)?,
                truncated,
            })
        }
        "docx" => {
            let data = docx::read_document(path)?;
            Ok(Rendered {
                html: data.html,
                truncated: data.truncated,
            })
        }
        "sheet" => {
            let table = table::read_workbook(path)?;
            Ok(Rendered {
                html: table.to_html(),
                truncated: table.truncated,
            })
        }
        "csv" | "tsv" => {
            let (text, cut) = read_text(path)?;
            let table = table::parse_delimited(&text, if kind == "csv" { b',' } else { b'\t' })?;
            Ok(Rendered {
                html: table.to_html(),
                truncated: cut || table.truncated,
            })
        }
        other => Err(format!("unknown kind {other}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, kind, file] = args.as_slice() else {
        eprintln!("usage: asc-files-docs markdown|html|rtf|docx|sheet|csv|tsv <file>");
        return ExitCode::from(2);
    };
    match render(kind, Path::new(file)) {
        Ok(rendered) => {
            println!(
                "{}",
                serde_json::json!({"html": rendered.html, "truncated": rendered.truncated})
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
