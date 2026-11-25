#!/usr/bin/env bash
# Runs the workspace tests with a private HOME and XDG dirs (so Trash tests never reach a real
# Trash), a short TMPDIR, and a 4 GB memory cap on their own scope, so a runaway test is killed
# alone. Extra arguments go to `cargo test`.
set -euo pipefail
cd "$(dirname "$0")/.."
home="${XDG_CACHE_HOME:-$HOME/.cache}/asc-files-test-home"
tmp="${XDG_CACHE_HOME:-$HOME/.cache}/asc-files-tmp"
rm -rf "$home"
mkdir -p "$home"/{config,data,cache,state,run} "$tmp"
chmod 700 "$home/run"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/asc-files-target}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
# The RAR helper is a program of its own, which the engine's RAR tests run.
cargo build --locked -q -p asc-files-rar
run=(env HOME="$home" XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data"
     ASC_FILES_RAR="$CARGO_TARGET_DIR/debug/asc-files-rar"
     XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" XDG_RUNTIME_DIR="$home/run"
     TMPDIR="$tmp" CARGO_TARGET_DIR="$CARGO_TARGET_DIR" CARGO_HOME="$CARGO_HOME" PATH="$PATH"
     cargo test --workspace --locked "$@")
if command -v systemd-run >/dev/null && systemd-run --user --scope -q true 2>/dev/null; then
    exec systemd-run --user --scope -q -p MemoryMax=4G -p MemorySwapMax=0 "${run[@]}"
fi
# Without a user systemd (CI containers), cap the address space instead.
ulimit -v $((8 * 1024 * 1024))
exec "${run[@]}"
