#!/usr/bin/env bash
# Builds and tests the Qt app. `scripts/app.sh build` builds the backend and the app;
# `scripts/app.sh test` also runs the app's tests with a private HOME and XDG dirs (as
# scripts/test.sh does) under a memory cap; `scripts/app.sh run [folder]` starts Files.
# Without Qt 6 on the system (NixOS), it runs itself inside a nix shell that has it.
set -euo pipefail
cd "$(dirname "$0")/.."
has_qt() {
    command -v qtpaths6 >/dev/null || command -v qtpaths-qt6 >/dev/null || command -v qtpaths >/dev/null ||
        pkg-config --exists Qt6Quick 2>/dev/null
}
if ! has_qt && [ -z "${ASC_FILES_IN_SHELL:-}" ]; then
    if command -v nix-shell >/dev/null; then
        exec env ASC_FILES_IN_SHELL=1 nix-shell -p qt6.qtbase qt6.qtdeclarative qt6.qtwayland qt6.qtimageformats qt6.qtsvg qt6.qtmultimedia kdePackages.syntax-highlighting cmake ninja poppler-utils ffmpeg \
            --run "$(printf '%q ' "$0" "$@")"
    fi
    echo "Qt 6 not found: install qt6-qtbase-devel and qt6-qtdeclarative-devel" >&2
    exit 1
fi
if [ -n "${ASC_FILES_IN_SHELL:-}" ]; then
    # A nix shell does not wrap programs with Qt's plugin and QML paths: give them.
    # The shell holds the -dev outputs; the modules are in the outputs they refer to.
    for p in $(nix-store -q --references $buildInputs $nativeBuildInputs 2>/dev/null | sort -u); do
        if [ -d "$p/lib/qt-6/qml" ]; then QML_IMPORT_PATH="$p/lib/qt-6/qml:${QML_IMPORT_PATH:-}"; fi
        if [ -d "$p/lib/qt-6/plugins" ]; then QT_PLUGIN_PATH="$p/lib/qt-6/plugins:${QT_PLUGIN_PATH:-}"; fi
    done
    export QML_IMPORT_PATH QT_PLUGIN_PATH
fi
command=${1:-build}
shift || true
cache="${XDG_CACHE_HOME:-$HOME/.cache}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$cache/asc-files-target}"
build="${ASC_FILES_APP_BUILD:-$cache/asc-files-app-build}"
cargo build -p asc-files-backend --locked --quiet
backend="$CARGO_TARGET_DIR/debug/asc-files-backend"
cmake -S app -B "$build" -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DASC_FILES_BACKEND="$backend" >/dev/null
cmake --build "$build"
case "$command" in
build) ;;
run)
    exec env ASC_FILES_BACKEND="$backend" "$build/asc-files" "$@"
    ;;
test)
    home="$cache/asc-files-test-home"
    tmp="$cache/asc-files-tmp"
    rm -rf "$home"
    mkdir -p "$home"/{config,data,cache,state,run} "$tmp"
    chmod 700 "$home/run"
    run=(env HOME="$home" XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data"
         XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" XDG_RUNTIME_DIR="$home/run"
         TMPDIR="$tmp" PATH="$PATH" QML_IMPORT_PATH="${QML_IMPORT_PATH:-}" QT_PLUGIN_PATH="${QT_PLUGIN_PATH:-}" ctest --test-dir "$build" --output-on-failure "$@")
    if command -v systemd-run >/dev/null && systemd-run --user --scope -q true 2>/dev/null; then
        exec systemd-run --user --scope -q -p MemoryMax=4G -p MemorySwapMax=0 "${run[@]}"
    fi
    ulimit -v $((8 * 1024 * 1024))
    exec "${run[@]}"
    ;;
*)
    echo "usage: scripts/app.sh build|test|run [folder]" >&2
    exit 2
    ;;
esac
