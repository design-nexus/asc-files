// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn renders_flowchart_and_sequence_diagrams_without_a_browser() {
    for source in [
        "flowchart LR\n A[Open file] --> B{Supported?}\n B -->|Yes| C[Render diagram]",
        "sequenceDiagram\n participant User\n participant Files\n User->>Files: Select Markdown\n Files-->>User: Render preview",
    ] {
        let png = mermaid(source).expect("render Mermaid");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.len() > 100, "nonempty rendered content");
    }
    assert!(mermaid("not a diagram").is_err());
}

#[test]
fn math_renders_fractions_roots_and_ams_without_host_capabilities() {
    for (source, display) in [
        (r"E=mc^2", false),
        (r"\frac{-b\pm\sqrt{b^2-4ac}}{2a}", true),
        (r"\begin{pmatrix}a&b\\c&d\end{pmatrix}", true),
        (r"\sum_{n=1}^{\infty}\frac{1}{n^2}=\frac{\pi^2}{6}", true),
    ] {
        let png = math(source, display, "#e7e8ee").expect("render equation");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
    for source in [
        r"\unknowncommand{x}",
        r"\frac{1}",
        r"\input{/etc/passwd}",
        r"\require{html}",
        r"\href{https://example.test}{x}",
        r"\def\x{\x}\x",
    ] {
        assert!(math(source, true, "#e7e8ee").is_err(), "reject {source}");
    }
}

#[test]
fn svg_text_remains_visible_with_generic_font_families() {
    for family in ["sans-serif", "serif", "monospace"] {
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40"><text x="4" y="24" font-family="{family}" font-size="20">Readable label</text></svg>"#
        );
        let rendered = svg(&source, 800).expect("text SVG");
        let pixels = resvg::tiny_skia::Pixmap::decode_png(&rendered.png).expect("PNG");
        assert!(
            pixels.pixels().iter().any(|pixel| pixel.alpha() != 0),
            "{family} must render text rather than an empty image"
        );
    }
}

#[test]
fn svg_images_ignore_external_resources_and_scripts() {
    let rendered = svg(r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"><script>alert('never')</script><image href="file:///private.png" width="80" height="60"/><image href="https://example.test/tracker.png" width="80" height="60"/><rect width="30" height="20" fill="#cc0000"/></svg>"##, 800).expect("inert SVG");
    let plain = svg(r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"><rect width="30" height="20" fill="#cc0000"/></svg>"##, 800).expect("plain SVG");
    assert_eq!(
        rendered.png, plain.png,
        "only supported local vector content should render"
    );
    assert!(svg("invalid SVG", 800).is_err());
}

#[test]
fn svg_reports_native_dimensions_and_bounds_the_render() {
    let rendered = svg(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="800"><rect width="1600" height="800" fill="#cc0000"/></svg>"##,
        256,
    )
    .expect("large SVG");
    assert_eq!((rendered.width, rendered.height), (1600, 800));
    let pixels = resvg::tiny_skia::Pixmap::decode_png(&rendered.png).expect("PNG");
    assert_eq!((pixels.width(), pixels.height()), (256, 128));
    let small = svg(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#cc0000"/></svg>"##,
        256,
    )
    .expect("small SVG");
    let pixels = resvg::tiny_skia::Pixmap::decode_png(&small.png).expect("PNG");
    assert_eq!((pixels.width(), pixels.height()), (40, 20));
    assert_eq!(
        svg_dimensions("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 12 6\"/>"),
        Some((12, 6))
    );
}

#[test]
fn svg_font_families_reads_attributes_styles_and_css() {
    let source = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg"><style>text { font-family: 'Brand Sans', sans-serif; }</style>"##,
        r#"<text font-family="Title, 'Alt Title'">a</text>"#,
        r##"<text style="font-family:Mono; fill:#000">b</text></svg>"##,
    );
    assert_eq!(
        svg_font_families(source),
        ["Brand Sans", "sans-serif", "Title", "Alt Title", "Mono"].map(str::to_owned)
    );
    assert!(svg_font_families("<svg/>").is_empty());
}

#[test]
fn numeric_character_references_keep_the_full_font_scan() {
    assert!(needs_full_font_scan(r#"<svg><text>&#x1F600;</text></svg>"#));
    assert!(needs_full_font_scan(
        "<svg><text>\u{4e2d}\u{6587}</text></svg>"
    ));
    assert!(!needs_full_font_scan("<svg><text>plain</text></svg>"));
}

#[test]
fn equations_take_the_text_colour_and_diagrams_a_light_card() {
    let png = math("x", false, "#ff0000").expect("equation");
    let pixels = resvg::tiny_skia::Pixmap::decode_png(&png).expect("PNG");
    let inked: Vec<_> = pixels
        .pixels()
        .iter()
        .filter(|pixel| pixel.alpha() > 200)
        .collect();
    assert!(!inked.is_empty());
    assert!(
        inked
            .iter()
            .all(|pixel| pixel.red() > 200 && pixel.green() < 60),
        "drawn in red"
    );
    let card = mermaid("flowchart LR\n A --> B").expect("diagram");
    let pixels = resvg::tiny_skia::Pixmap::decode_png(&card).expect("PNG");
    let corner = pixels.pixel(0, 0).expect("corner");
    assert_eq!(
        (corner.red(), corner.green(), corner.blue(), corner.alpha()),
        (0xf4, 0xf4, 0xf7, 255)
    );
    assert!(is_colour("#a1b2c3") && !is_colour("red") && !is_colour("#12345g"));
}
