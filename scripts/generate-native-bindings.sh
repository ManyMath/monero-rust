#!/bin/sh
# Regenerate the Dart FFI bindings for the native (dart:ffi) transport.
#
# The chain has two links, and only the first was ever automated:
#
#   monero_wasm's build.rs --cbindgen--> native/monero_wasm/target/bridge/bindings.h
#   ffigen.yaml            --ffigen---> flutter/web/lib/src/bindings/hub_native_ffi.g.dart
#
# The header lives under target/ and so is never committed, and nothing ran ffigen, so
# hub_native_transport.dart imported a file that did not exist and `dart analyze` reported
# eight errors on a clean checkout. The generated Dart is committed now, which is what lets
# someone analyse or edit the Dart without a Rust toolchain; this script keeps it well-behaved.
#
#   scripts/generate-native-bindings.sh          # regenerate in place
#   scripts/generate-native-bindings.sh --check   # fail if the committed copy is stale
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WEB="$ROOT/flutter/web"
GENERATED="$WEB/lib/src/bindings/hub_native_ffi.g.dart"
CHECK=0

[ "${1:-}" = "--check" ] && CHECK=1

command -v flutter >/dev/null 2>&1 || {
    echo "flutter not on PATH; needed to run ffigen" >&2
    exit 1
}

# build.rs runs cbindgen, so a native check refreshes the header. The wasm32 target skips
# it, which is why this asks for the host target explicitly.
CARGO_LOG="$(mktemp)"
if ! (cd "$WEB/native/monero_wasm" && cargo check --quiet) >"$CARGO_LOG" 2>&1; then
    cat "$CARGO_LOG" >&2
    rm -f "$CARGO_LOG"
    echo "cargo check failed; cannot refresh the C header" >&2
    exit 1
fi
rm -f "$CARGO_LOG"

HEADER="$WEB/native/monero_wasm/target/bridge/bindings.h"
[ -f "$HEADER" ] || {
    echo "cbindgen did not produce $HEADER" >&2
    exit 1
}

if [ "$CHECK" = 1 ]; then
    BEFORE="$(mktemp)"
    cp "$GENERATED" "$BEFORE" 2>/dev/null || true
fi

(cd "$WEB" && dart run ffigen --config ffigen.yaml >/dev/null)

if [ "$CHECK" = 1 ]; then
    if ! cmp -s "$BEFORE" "$GENERATED"; then
        echo "hub_native_ffi.g.dart is out of date with the Rust FFI surface." >&2
        echo "Run scripts/generate-native-bindings.sh and commit the result." >&2
        cp "$BEFORE" "$GENERATED" 2>/dev/null || true
        rm -f "$BEFORE"
        exit 1
    fi
    rm -f "$BEFORE"
    echo "bindings are current"
else
    echo "regenerated $GENERATED"
fi
