#!/bin/sh
# Check formatting, compile, and run selected unit tests during development.
# Use test-all.sh for the full suite, including vectors, wasm, and regtest.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
cargo check --all-targets
cargo test --lib crypto::tests
cargo test --lib coin_selection::tests
cargo test --lib scan_coordinator::tests
cargo test --lib wallet_output::tests
cargo test --lib wallet_state::tests
