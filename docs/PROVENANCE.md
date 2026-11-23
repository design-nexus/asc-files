# Provenance

Files for Ascendant is MIT-licensed (`LICENSE`). Code ported from elsewhere keeps its notices.

| Source | Revision | Licence | What came from it |
|---|---|---|---|
| Legacy Nyx Files, `nyx-legacy/nyx-files` (itself derived from `lgse/strata@809e2f5`) | `36887fe5f3b40c2a5be50bda1d13f80a426e1a2b`, plus the `fix/replace-in-multi-item-paste` change to `local_operations.rs` | MIT for every ported file (`SPDX-License-Identifier: MIT`, © LGSE Ltd.); the legacy app as a whole is GPL-3.0-or-later, and **no GPL-only file is ported** | The copy/move/placement engine (`adapters/local_operations/**`), mount tables and name rules (`adapters/volume/**`), `model`, the `services` types the engine uses, the fault-injection test harness (`tests/fs_quirks.rs`) and the engine's unit tests |
| Flea, `thisisgm/flea` | `1af3fa944c54506e808d30f574be9e2ee8bd350f` (v0.4.3) | MIT (© 2026 GM) | Nothing yet. Trash, undo, listing and the job-queue design are planned (Phase A2, A5–A7). The Flea notice is added to `LICENSE` with the first ported file. |

The port to a GLib-free engine was done in the Files 2.0 spike (`~/Projects/work/nyx-files-2.0-spike/SPIKE-LOG.md`).
Changes against legacy are recorded there: GLib error and cancellation types replaced, `unicode-segmentation` for
Pango's grapheme breaks, ring's RNG for staging names, native-path-only `Location`, the `rustix` ≥ 1.1.5 xattr size
query, and the engine-level into-itself guard.
