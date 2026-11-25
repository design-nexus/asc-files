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

## UnRAR

- Project: <https://www.rarlab.com/rar_add.htm>
- Bundled source: UnRAR 7.01 (2024-05-12), statically linked by `unrar_sys` 0.5.8 in the separately
  licensed `asc-files-rar` helper (`crates/asc-files-rar`); never linked into Files, its engine or its
  backend, which run the helper as a program of its own
- Copyright: Alexander Roshal
- License: custom UnRAR license; **not** MIT or Apache-2.0
- Full license: [`crates/asc-files-rar/licenses/UnRAR.txt`](crates/asc-files-rar/licenses/UnRAR.txt)

UnRAR permits RAR extraction but prohibits using its source to develop a RAR-compatible archiver or
recreate the proprietary RAR compression algorithm. The wrapper crates' permissive metadata does not
cover this native implementation. Files does not modify its vendored source. The RAR test fixtures in
`crates/asc-files-rar/tests/fixtures/rar` are the `unrar` crate's own, under its MIT option.
