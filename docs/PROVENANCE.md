# Provenance

Files for Ascendant is MIT-licensed (`LICENSE`). Code ported from elsewhere keeps its notices.

| Source | Revision | Licence | What came from it |
|---|---|---|---|
| Legacy Nyx Files, `nyx-legacy/nyx-files` (itself derived from `lgse/strata@809e2f5`) | `36887fe5f3b40c2a5be50bda1d13f80a426e1a2b`, plus the `fix/replace-in-multi-item-paste` change to `local_operations.rs` | MIT for every ported file (`SPDX-License-Identifier: MIT`, © LGSE Ltd.); the legacy app as a whole is GPL-3.0-or-later, and **no GPL-only file is ported** | The copy/move/placement engine (`adapters/local_operations/**`), mount tables and name rules (`adapters/volume/**`), `model`, the `services` types the engine uses, the fault-injection test harness (`tests/fs_quirks.rs`) and the engine's unit tests |
| Flea, `thisisgm/flea` | `1af3fa944c54506e808d30f574be9e2ee8bd350f` (v0.4.3) | MIT (© 2026 GM, in `LICENSE`) | **Trash location and record rules** (`crates/asc-files-engine/src/trash.rs`, from `src/backend/topdirtrash.rs` and `topdirtrash/locations.rs`): an administrator's `.Trash` only when it is a real sticky folder on the same drive, `$uid` and `.Trash-$uid` private and same-drive, no mount roots or items already in a Trash, records relative to the drive and never restoring outside it, one drive listed once. Rewritten on `rustix`, not copied. **Listing and watching** (`crates/asc-files-engine/src/listing.rs`): Finder name order (`src/backend/sort.rs`, ported nearly line for line), the inotify mask without `IN_MODIFY` and the 100 ms burst coalescing (`watch.rs`), gone and unmounted folders answered with the nearest folder left, network folders polled by mtime every 5 s under a 10 s deadline with a stalled share left alone for 60 s (`watchpoll.rs`), and at most two reads per drive (`listwork_scanner.rs`); rewritten on `rustix` and threads, since Flea's listing is woven into its single-listing loop, search sessions and thumbnail pool. **Undo and redo** (`crates/asc-files-engine/src/undo.rs`): Flea's journal design (`src/backend/undo.rs`, `redo.rs`, `undoshare.rs`): entries of steps reversed newest first, each guarded by the identity of the item it left (device, inode, kind, birth time), redo from recorded inputs that never replaces, one locked history file per session under `$XDG_RUNTIME_DIR`, 50 deep, a too-large entry kept as a placeholder. New code over the engine's `ops`; undoing a copy to the Trash follows legacy. **Job queue** (`crates/asc-files-engine/src/jobs.rs`): Flea's per-drive queue (batches for one drive in order, other drives in parallel), reimplemented over the engine's jobs. Merge plans with a choice per clash follow the legacy app's merge dialog and its undo of overwritten originals; `merge.rs` is new code over `ops`. |

The port to a GLib-free engine was done in the Files 2.0 spike (`~/Projects/work/nyx-files-2.0-spike/SPIKE-LOG.md`).
Changes against legacy are recorded there: GLib error and cancellation types replaced, `unicode-segmentation` for
Pango's grapheme breaks, ring's RNG for staging names, native-path-only `Location`, the `rustix` ≥ 1.1.5 xattr size
query, and the engine-level into-itself guard.

## Trash: what Flea did and did not have

Flea's own Trash is native only for **putting** items on the home filesystem and in per-drive Trash folders for btrfs
subvolume and bind mounts. Browsing, restoring and emptying go through the `gio` command-line tool
(`src/backend/trashbrowse.rs`: "GIO owns the shared Trash store"). Files implements the whole freedesktop.org
Trash specification natively instead: put, list, restore, delete, empty, the `directorysizes` cache, home and
per-drive.
