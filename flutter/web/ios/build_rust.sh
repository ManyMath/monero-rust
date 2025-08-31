#!/usr/bin/env bash
# Build the monero_wasm Rust native library for iOS (aarch64).
#
# Produces a static library (.a) for aarch64-apple-ios and copies it
# into the Runner directory for Xcode to link.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MANIFEST="${SCRIPT_DIR}/../native/monero_wasm/Cargo.toml"
TARGET_DIR="${SCRIPT_DIR}/../target"

RUST_TARGET="aarch64-apple-ios"

CARGO_PROFILE="${CARGO_PROFILE:-release}"
CARGO_FLAGS=""
if [[ "$CARGO_PROFILE" == "release" ]]; then
  CARGO_FLAGS="--release"
fi

echo "Building monero_wasm for iOS (${RUST_TARGET})..."
cargo build --manifest-path "${MANIFEST}" --target "${RUST_TARGET}" ${CARGO_FLAGS}

LIB_PATH="${TARGET_DIR}/${RUST_TARGET}/${CARGO_PROFILE}/libmonero_wasm.a"
if [[ ! -f "${LIB_PATH}" ]]; then
  echo "ERROR: Expected static library not found at ${LIB_PATH}" >&2
  exit 1
fi

# Copy into the Runner directory for Xcode to pick up.
DEST_DIR="${SCRIPT_DIR}/Runner"
mkdir -p "${DEST_DIR}"
cp "${LIB_PATH}" "${DEST_DIR}/libmonero_wasm.a"

echo "iOS Rust build complete: ${DEST_DIR}/libmonero_wasm.a"
