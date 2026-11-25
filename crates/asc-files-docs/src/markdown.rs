// SPDX-License-Identifier: MIT
//! Markdown as HTML for the preview: CommonMark with tables, strikethrough and task lists. HTML
//! written in the document shows as text, and pictures as their description: the preview reaches
//! nothing outside the file.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

pub(crate) fn to_html(source: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut in_image = 0usize;
    let events = Parser::new_ext(source, options).filter_map(|event| match event {
        // HTML the document holds is shown, not followed.
        Event::Html(text) | Event::InlineHtml(text) => Some(Event::Text(text)),
        // A picture: its description, in brackets.
        Event::Start(Tag::Image { .. }) => {
            in_image += 1;
            Some(Event::Text("[".into()))
        }
        Event::End(TagEnd::Image) => {
            in_image = in_image.saturating_sub(1);
            Some(Event::Text("]".into()))
        }
        // Links show as their text: the preview opens nothing.
        Event::Start(Tag::Link { .. }) | Event::End(TagEnd::Link) => None,
        other => Some(other),
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_reaches_nothing_outside_the_file() {
        let html = to_html(
            "# Title\n\nSome *text* and a [link](https://example.com).\n\n![a cat](https://example.com/cat.png)\n\n<img src=\"https://example.com/x.png\"><script>alert(1)</script>\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        );
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        assert!(html.contains("<em>text</em>"), "{html}");
        assert!(html.contains("a link."), "{html}");
        assert!(!html.contains("href"), "{html}");
        assert!(html.contains("[a cat]"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(html.contains("<table>"), "{html}");
    }
}
