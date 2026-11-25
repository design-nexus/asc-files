// SPDX-License-Identifier: MIT
//! An HTML file as the preview shows it: its text and structure only. Scripts, styles, forms,
//! frames, pictures and links' targets go (ammonia, with no URL kept), so viewing it reaches
//! nothing.

pub(crate) fn clean(source: &str) -> String {
    ammonia::Builder::default()
        .rm_tags(&[
            "img", "a", "picture", "source", "video", "audio", "svg", "math",
        ])
        .url_schemes(std::collections::HashSet::new())
        .link_rel(None)
        .clean(source)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_keeps_its_text_and_loses_what_reaches_out() {
        let html = clean(
            "<html><head><style>p{color:red}</style><script>alert(1)</script></head><body>\
             <h1 onclick=\"x()\">Hello</h1><p>A <b>bold</b> <a href=\"https://example.com\">link</a>.</p>\
             <img src=\"https://example.com/track.png\"><iframe src=\"https://example.com\"></iframe>\
             <table><tr><td>1</td></tr></table></body></html>",
        );
        assert!(html.contains("<h1>Hello</h1>"), "{html}");
        assert!(html.contains("<b>bold</b>"), "{html}");
        assert!(html.contains("link"), "{html}");
        for gone in [
            "script",
            "style",
            "onclick",
            "href",
            "<img",
            "iframe",
            "example.com",
            "color:red",
        ] {
            assert!(!html.contains(gone), "{gone} in {html}");
        }
        assert!(html.contains("<table>"), "{html}");
    }
}
