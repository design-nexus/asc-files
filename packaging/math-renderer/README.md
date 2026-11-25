# Offline equation renderer

`crates/asc-files-docs/data/math-renderer.js` is a reproducible, bundled MathJax 4.1.3 renderer with
MathJax's TeX SVG font data. It is compiled into the document reader, asc-files-docs; Node/npm are needed only
when deliberately regenerating the bundle, never for application builds or previews.
The exact build inputs are in `package-lock.json`.

From this directory:

```sh
npm ci --ignore-scripts
npm audit
npm run build
```

Review the generated diff and rerun the equation helper and Markdown preview
regressions after any update. Keep the bundle, lockfile, license, and entry point
together. Do not enable dynamic font loading, TeX autoload/require, HTML extensions,
or host JavaScript capabilities.

Only the base and AMS TeX packages are registered. Equation source is passed to a
fixed function as a string, never evaluated as JavaScript. QuickJS runs only inside
the document reader, which the app runs in its Bubblewrap sandbox, without a module loader or host callbacks; it has a
64 MiB heap limit, a 1 MiB stack limit, and a two-second interrupt deadline within
the sandbox's own time limit. SVG image references are disabled
when rasterizing the result. Unsupported extensions and input limits produce a
readable source fallback.

MathJax and its TeX SVG font data are Apache-2.0 licensed. See
`../../crates/asc-files-docs/data/licenses/MathJax-Apache-2.0.txt` and `../../THIRD_PARTY_LICENSES.md`.
