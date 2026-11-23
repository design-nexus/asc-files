#!/usr/bin/env bash
# Fails if a GLib, GTK, Qt or GPUI binding enters the engine's or backend's dependency tree.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/asc-files-target}"
forbidden='^(glib|gio|gio-unix|gobject|gtk|gtk4|gdk|gdk4|gdk-pixbuf|pango|cairo|cairo-rs|graphene|gstreamer|qt|cxx-qt|qmetaobject|gpui|system-deps)([-_].*)?$'
status=0
for crate in asc-files-engine asc-files-backend; do
    found=$(cargo tree --locked -p "$crate" -e normal,build --prefix none --format '{p}' \
        | awk '{print $1}' | sort -u | grep -E "$forbidden" || true)
    if [ -n "$found" ]; then
        echo "$crate depends on forbidden crates:" >&2
        echo "$found" >&2
        status=1
    else
        echo "$crate: no GLib/GTK/Qt/GPUI crates ($(cargo tree --locked -p "$crate" -e normal,build --prefix none --format '{p}' | awk '{print $1}' | sort -u | wc -l) crates checked)"
    fi
done
exit $status
