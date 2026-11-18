#!/bin/sh
# Picker-level samples for one distribution, from both implementations.
#   run_picker.sh <name> [builds per seed]
# Set STAT_PARITY_SKIP_WALLET2=1 to produce only the monero-rust half.
set -eu
NAME="$1"
BUILDS="${2:-5000}"
HERE="$(cd "$(dirname "$0")" && pwd)"
R="${STAT_PARITY_DIR:-${TMPDIR:-/tmp}/monero-rust-stat-parity}"
PICKS="${STAT_PARITY_PICKS:-$R/picks}"
W2="${W2HARNESS_DIR:-$R/w2build}"
TARGET="${CARGO_TARGET_DIR:-$HERE/../../rust/target}"
mkdir -p "$PICKS"
if [ -z "${STAT_PARITY_SKIP_WALLET2:-}" ]; then
    "$W2/w2harness-seeded" pick "$R/dist/$NAME.u64" 40000000 1 "$PICKS/$NAME.w2.seed1.u64"
    "$W2/w2harness-seeded" pick "$R/dist/$NAME.u64" 40000000 2 "$PICKS/$NAME.w2.seed2.u64"
    "$W2/w2harness-native" pick "$R/dist/$NAME.u64" 40000000 0 "$PICKS/$NAME.w2.native.u64"
fi
BIN="$(ls -t "$TARGET"/release/deps/stat_parity_harness-* | grep -v '\.d$' | head -1)"
for seed in 101 102 103 104; do
    STATP_DIST="$R/dist/$NAME.u64" STATP_OUT="$PICKS/$NAME.rust.seed$seed" STATP_SEED=$seed \
        STATP_BUILDS="$BUILDS" STATP_INPUTS=8 "$BIN" picker_offline --exact >/dev/null 2>&1 &
done
wait
ls -la "$PICKS" | grep "$NAME"
