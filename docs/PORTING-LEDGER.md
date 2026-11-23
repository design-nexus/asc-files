# Porting ledger

Tests from the legacy suite that are not running here yet, and why. Phase A8 moves the behavioural suite onto the
public API; this list must be empty, or every entry justified, before Phase A ends.

## Behavioural suite (legacy `adapters/local_operations/tests/**`, 520 non-archive tests)

Not carried yet. In legacy they drive the engine through GLib async wrappers, which this repo does not have. They
come back test by test against `ops`/`jobs` (A8). The spike ran 509 of them on a GTK-free build: 503 passed, 3 were
ignored as upstream, and 3 failed only on test-support shims (`SPIKE-LOG.md`).

## Engine unit tests disabled with `#[cfg(any())]` (32)

Each marked at its site with the reason. Groups:
- **26: need GIO or an app layer.** Drag-and-drop volume lookup (UI policy), GIO destination queries, and the app's
  operation test helpers. They return when the matching adapter exists, or are deleted with their feature.
- **6: by design.** URI-location semantics in `model/tests.rs` (remote parents, breadcrumbs, `smb://` and `trash://`
  names, camera roots). The engine handles native paths only, so these belong to the remote/GVfs adapter's tests.

## Archives (legacy: 225 tests)

Out of scope for Phase A.
