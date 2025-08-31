#!/usr/bin/env bash
# Build the monero_wasm Rust native library for macOS.
#
# Builds for the native host target and copies the dylib into
# the macOS app bundle's Frameworks directory.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MANIFEST="${SCRIPT_DIR}/../native/monero_wasm/Cargo.toml"
TARGET_DIR="${SCRIPT_DIR}/../target"

CARGO_PROFILE="${CARGO_PROFILE:-release}"
CARGO_FLAGS=""
if [[ "$CARGO_PROFILE" == "release" ]]; then
  CARGO_FLAGS="--release"
fi

echo "Building monero_wasm for macOS (native target)..."
cargo build --manifest-path "${MANIFEST}" ${CARGO_FLAGS}

# Determine the library path.
LIB_PATH="${TARGET_DIR}/${CARGO_PROFILE}/libmonero_wasm.dylib"
if [[ ! -f "${LIB_PATH}" ]]; then
  echo "ERROR: Expected library not found at ${LIB_PATH}" >&2
  exit 1
fi

# Copy into the Runner's Frameworks directory (created by Xcode build).
FRAMEWORKS_DIR="${SCRIPT_DIR}/Runner/Frameworks"
mkdir -p "${FRAMEWORKS_DIR}"
cp "${LIB_PATH}" "${FRAMEWORKS_DIR}/libmonero_wasm.dylib"

# Fix the install name so the app can load it at runtime.
install_name_tool -id "@rpath/libmonero_wasm.dylib" \
  "${FRAMEWORKS_DIR}/libmonero_wasm.dylib"

echo "macOS Rust build complete: ${FRAMEWORKS_DIR}/libmonero_wasm.dylib"
