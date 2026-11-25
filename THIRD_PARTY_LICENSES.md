# Third-party notices

Files includes or derives assets from the following projects.

## MathJax

- Projects: <https://github.com/mathjax/MathJax-src>, <https://github.com/mathjax/MathJax-fonts>
- Version: MathJax and MathJax TeX SVG font data 4.1.3
- Copyright: 2017–2026 The MathJax Consortium
- License: Apache License 2.0
- Included asset: `crates/asc-files-docs/data/math-renderer.js` (bundled/minified base and AMS
  renderer with TeX SVG glyph data), from legacy Nyx Files
- Full license: [`crates/asc-files-docs/data/licenses/MathJax-Apache-2.0.txt`](crates/asc-files-docs/data/licenses/MathJax-Apache-2.0.txt)

The bundle adds an offline rendering entry point. Build inputs and regeneration instructions are
in [`packaging/math-renderer/`](packaging/math-renderer/README.md).
