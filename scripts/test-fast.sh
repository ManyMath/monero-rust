#!/bin/sh
# Check formatting, compile, and run selected unit tests during development.
# Use test-all.sh for the full suite, including vectors, wasm, and regtest.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/rust"

cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test -p monero-rust --lib crypto::tests
cargo test -p monero-rust --lib coin_selection::tests
cargo test -p monero-rust --lib scan_coordinator::tests
cargo test -p monero-rust --lib wallet_output::tests
cargo test -p monero-rust --lib wallet_state::tests
