#!/usr/bin/env bash
# Verify that monero-rust compiles for wasm32-unknown-unknown.
#
# Dev-dependencies (reqwest, monero-serai http-rpc) pull in Send-bound futures
# that are incompatible with WASM's single-threaded runtime, so we check the
# library only (--lib). Full wasm-pack test would require a WASM-only test
# workspace crate that excludes those dev-deps.
#
# Usage: ./scripts/check-wasm.sh [--release]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$SCRIPT_DIR/../rust/monero-rust"

RELEASE_FLAG=""
if [[ "${1:-}" == "--release" ]]; then
    RELEASE_FLAG="--release"
fi

echo "==> Checking WASM build for monero-rust..."
cargo build \
    --manifest-path "$CRATE_DIR/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --lib \
    $RELEASE_FLAG

echo "==> WASM build OK"
