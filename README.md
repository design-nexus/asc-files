# Files

The file manager for Ascendant: a Qt Quick app over a Rust backend process. This repository holds the
backend. The Qt app arrives in Phase B.

- `crates/asc-files-engine`: the copy, move, replace and trash engine. A synchronous API (`ops`) and batch jobs
  (`jobs`), with no GLib, GTK, Qt or GPUI dependency (checked in CI).
- `crates/asc-files-backend`: the `asc-files-backend` process, JSON-RPC 2.0 with one message per line on stdio.

Build: `cargo build --workspace --locked`. Test: `scripts/test.sh`, which uses a private home and XDG dirs and caps
memory at 4 GB; run tests only through it. Dependency guard: `scripts/check-no-glib.sh`.

Plan and status: `NYX-FILES-2.0-PLAN.md` (Phase A). Backend protocol: `docs/PROTOCOL.md`. Provenance: `docs/PROVENANCE.md`. Tests still to port:
`docs/PORTING-LEDGER.md`.
