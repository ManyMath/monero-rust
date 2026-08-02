#!/bin/sh
# Run every tier of this repo's tests, including the ones that are opt-in.
#
# They are opt-in for good reasons - a feature flag, a daemon, a second toolchain - but the
# consequence is that nothing runs them. The fixture-driven send-path tests broke at the
# oxide cutover and stayed broken for months because of that, and the dart and
# wasm tiers had never run at all. This exists so "run the tests" means all of them.
#
# A tier whose toolchain is missing or broken says so and is skipped; a tier that runs and
# fails is reported as a failure. The two are never conflated.
#
#   scripts/test-all.sh              # everything
#   scripts/test-all.sh --no-regtest # skip the tier that needs monerod
#
# List failed groups and exit nonzero if any fail.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REGTEST="$ROOT/scripts/regtest-node.sh"
WITH_REGTEST=1
STARTED_NODE=0
FAILED=""

for arg in "$@"; do
    case "$arg" in
        --no-regtest) WITH_REGTEST=0 ;;
        *)
            echo "usage: $0 [--no-regtest]" >&2
            exit 2
            ;;
    esac
done

step() {
    echo
    echo "=== $1 ==="
}

record_failure() {
    FAILED="$FAILED
  - $1"
}

cleanup() {
    if [ "$STARTED_NODE" = 1 ]; then
        "$REGTEST" stop >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT INT TERM

cd "$ROOT/rust"

step "unit and integration tests (live tiers skip)"
if ! cargo test --workspace --no-fail-fast; then
    record_failure "default suite"
fi

step "wasm32 builds"
if ! cargo check -p monero-rust --target wasm32-unknown-unknown; then
    record_failure "monero-rust wasm32"
fi
if ! (cd "$ROOT/flutter/web/native/monero_wasm" && cargo check --target wasm32-unknown-unknown); then
    record_failure "monero_wasm wasm32"
fi

step "wasm32 unit tests"
# The runner from wasm-bindgen-cli must match the wasm-bindgen dependency.
# Skip if the runner is missing; a failed run is an error. Only run library
# tests on wasm32 because integration tests require native support.
if ! command -v wasm-bindgen-test-runner >/dev/null 2>&1; then
    echo "wasm-bindgen-test-runner not on PATH - skipping the wasm32 tests." >&2
    echo "Install a CLI matching the wasm-bindgen crate version, e.g." >&2
    echo "  cargo install -f wasm-bindgen-cli --version 0.2.126" >&2
elif ! cargo test -p monero-rust --lib --target wasm32-unknown-unknown; then
    record_failure "wasm32 unit tests"
fi

step "send path against recorded RPC (--features mock-rpc)"
# Enable recorded RPC responses for tests that otherwise require a live node.
if ! cargo test -p monero-rust --features mock-rpc \
    --test tx_construction_test --test unsigned_tx_roundtrip_test --test sign_unsigned_unit_test; then
    record_failure "mock-rpc send path"
fi

step "native ffi bindings are current"
# The generated bindings are committed so Dart work needs no Rust toolchain, which means
# they can drift from the Rust FFI surface without anything noticing.
if ! command -v flutter >/dev/null 2>&1; then
    echo "flutter not on PATH - skipping the bindings check" >&2
elif ! "$ROOT/scripts/generate-native-bindings.sh" --check; then
    record_failure "native ffi bindings out of date"
fi

step "flutter/web dart tests"
if ! command -v flutter >/dev/null 2>&1; then
    echo "flutter not on PATH - skipping the dart tests" >&2
else
    DART_LOG="$(mktemp)"
    if (cd "$ROOT/flutter/web" && flutter test --no-pub) >"$DART_LOG" 2>&1; then
        tail -n 3 "$DART_LOG"
    elif grep -q '/packages/flutter/lib/' "$DART_LOG"; then
        # The failures are inside the SDK's own framework, not our tests: a Flutter
        # checkout whose framework revision and Dart SDK don't match can't compile
        # itself. Report it, but don't blame the repo for it.
        echo "SKIPPED: the Flutter SDK cannot compile its own framework, so no dart" >&2
        echo "test could load. This is the toolchain, not this repo - check" >&2
        echo "'flutter --version' (a detached user-branch checkout will do this) and" >&2
        echo "repair with 'flutter channel stable && flutter upgrade'." >&2
        echo "Full output: $DART_LOG" >&2
    else
        tail -n 20 "$DART_LOG" >&2
        record_failure "dart tests"
    fi
fi

step "flutter/web browser tests"
# @TestOn('browser') files are skipped by `flutter test`, so they need their own
# invocation. They went unrun long enough for ten of them to rot unnoticed.
if ! command -v flutter >/dev/null 2>&1; then
    echo "flutter not on PATH - skipping the browser tests" >&2
elif [ ! -x "$(command -v google-chrome || command -v chromium || true)" ]; then
    echo "no chrome on PATH - skipping the browser tests" >&2
else
    BROWSER_TESTS="test/widget_test.dart \
        test/services/indexed_db_storage_backend_web_test.dart \
        test/services/js_interop_bindings_web_test.dart"
    # shellcheck disable=SC2086
    if ! (cd "$ROOT/flutter/web" && flutter test --no-pub --platform chrome $BROWSER_TESTS); then
        record_failure "browser tests"
    fi
fi

if [ "$WITH_REGTEST" = 1 ]; then
    step "end to end against a regtest chain"
    if ! command -v "${MONEROD:-monerod}" >/dev/null 2>&1; then
        echo "monerod not on PATH - skipping the regtest tier (pass --no-regtest to silence)" >&2
    else
        # Reuse an existing regtest node; stop only a node started by this script.
        if MONERO_REGTEST_RPC="$("$REGTEST" status 2>/dev/null)"; then
            echo "using the regtest node already running at $MONERO_REGTEST_RPC"
        else
            eval "$("$REGTEST" start)"
            STARTED_NODE=1
        fi
        export MONERO_REGTEST_RPC

        if ! cargo test -p monero-rust --test regtest_e2e_test; then
            record_failure "regtest end to end"
        fi
    fi
fi

echo
if [ -n "$FAILED" ]; then
    echo "FAILED:$FAILED"
    exit 1
fi
echo "all tiers passed"
