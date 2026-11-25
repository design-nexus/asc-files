// SPDX-License-Identifier: MIT
//! Markdown as HTML for the preview: CommonMark with tables, strikethrough, task lists and maths.
//! HTML written in the document shows as text, pictures as their description and links as their
//! text: the preview reaches nothing outside the file. Mermaid diagrams and equations are drawn
//! (`media`) and stand in the HTML as `<img src="media:N">`, N their place in `Markdown::media`.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

/// Pictures a document may draw.
const MEDIA_LIMIT: usize = 32;

pub(crate) struct Markdown {
    pub html: String,
    /// PNGs, by their number in the HTML.
    pub media: Vec<Vec<u8>>,
}

/// `ink`: the colour equations are drawn in (`#rrggbb`), the text's where they show.
pub(crate) fn to_html(source: &str, ink: &str) -> Markdown {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH;
    let mut media: Vec<Vec<u8>> = Vec::new();
    // A drawn picture as HTML, or `None` past the limit or when it can't be drawn.
    let mut picture = |png: Result<Vec<u8>, String>, block: bool| -> Option<Event<'static>> {
        let png = png.ok().filter(|_| media.len() < MEDIA_LIMIT)?;
        media.push(png);
        let tag = format!("<img src=\"media:{}\">", media.len() - 1);
        Some(Event::Html(
            if block { format!("<p>{tag}</p>") } else { tag }.into(),
        ))
    };
    let mut events = Vec::new();
    // A Mermaid block's text, while one is open.
    let mut diagram: Option<String> = None;
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref language)))
                if language.trim() == "mermaid" =>
            {
                diagram = Some(String::new());
            }
            Event::Text(text) if diagram.is_some() => diagram.as_mut().unwrap().push_str(&text),
            Event::End(TagEnd::CodeBlock) if diagram.is_some() => {
                let text = diagram.take().unwrap_or_default();
                match picture(crate::media::mermaid(&text), true) {
                    Some(drawn) => events.push(drawn),
                    // Not drawn: shown as written.
                    None => {
                        events.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(
                            "mermaid".into(),
                        ))));
                        events.push(Event::Text(text.into()));
                        events.push(Event::End(TagEnd::CodeBlock));
                    }
                }
            }
            Event::InlineMath(tex) => {
                events.push(
                    picture(crate::media::math(&tex, false, ink), false)
                        .unwrap_or(Event::Code(tex.into_string().into())),
                );
            }
            Event::DisplayMath(tex) => {
                events.push(
                    picture(crate::media::math(&tex, true, ink), true)
                        .unwrap_or(Event::Code(tex.into_string().into())),
                );
            }
            // HTML the document holds is shown, not followed.
            Event::Html(text) | Event::InlineHtml(text) => events.push(Event::Text(text)),
            // A picture: its description, in brackets.
            Event::Start(Tag::Image { .. }) => events.push(Event::Text("[".into())),
            Event::End(TagEnd::Image) => events.push(Event::Text("]".into())),
            // Links show as their text: the preview opens nothing.
            Event::Start(Tag::Link { .. }) | Event::End(TagEnd::Link) => {}
            other => events.push(other),
        }
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    Markdown { html: out, media }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_reaches_nothing_outside_the_file() {
        let html = to_html(
            "# Title\n\nSome *text* and a [link](https://example.com).\n\n![a cat](https://example.com/cat.png)\n\n<img src=\"https://example.com/x.png\"><script>alert(1)</script>\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
            "#000000",
        )
        .html;
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        assert!(html.contains("<em>text</em>"), "{html}");
        assert!(html.contains("a link."), "{html}");
        assert!(!html.contains("href"), "{html}");
        assert!(html.contains("[a cat]"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(html.contains("<table>"), "{html}");
    }

    #[test]
    fn diagrams_and_equations_are_drawn_and_what_cannot_be_stays_as_written() {
        let markdown = to_html(
            "Energy: $E=mc^2$.\n\n$$\\frac{1}{2}$$\n\n```mermaid\nflowchart LR\n A --> B\n```\n\n```mermaid\nnot a diagram\n```\n\n$\\unknowncommand{x}$\n",
            "#000000",
        );
        assert_eq!(markdown.media.len(), 3, "{}", markdown.html);
        for (index, png) in markdown.media.iter().enumerate() {
            assert!(png.starts_with(b"\x89PNG"));
            assert!(
                markdown
                    .html
                    .contains(&format!("<img src=\"media:{index}\">")),
                "{}",
                markdown.html
            );
        }
        assert!(markdown.html.contains("not a diagram"), "{}", markdown.html);
        assert!(
            markdown.html.contains("<code>\\unknowncommand{x}</code>"),
            "{}",
            markdown.html
        );
    }
}
