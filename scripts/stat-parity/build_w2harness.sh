#!/bin/sh
# Build w2harness-seeded and w2harness-native out of tree against a wallet2 build.
#   MONERO_SRC=<monero tree> MONERO_BUILD=<its cmake build dir> build_w2harness.sh [out dir]
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
: "${MONERO_SRC:?set MONERO_SRC to the monero source tree}"
: "${MONERO_BUILD:?set MONERO_BUILD to its cmake build directory}"
OUT="${1:-${STAT_PARITY_DIR:-${TMPDIR:-/tmp}/monero-rust-stat-parity}/w2build}"
mkdir -p "$OUT"
RULES="$MONERO_BUILD/src/wallet/CMakeFiles/wallet_rpc_server.dir"

value() { sed -n "s/^$1 = //p" "$RULES/flags.make"; }
COMPILE="$(value CXX_FLAGS) $(value CXX_DEFINES) $(value CXX_INCLUDES) -w"

link_line() {
    sed -e "s#CMakeFiles/wallet_rpc_server.dir/[^ ]*\\.o#$1#" \
        -e "s#-o [^ ]*monero-wallet-rpc#-o $2#" \
        -e "s# \\.\\./# $MONERO_BUILD/src/wallet/../#g" "$RULES/link.txt"
}

for kind in seeded native; do
    extra=""
    link=""
    # random.c.o still links for its other symbols; the harness object comes first, so its two functions win.
    [ "$kind" = seeded ] && extra="-DSEEDED_RNG" && link="-Wl,--allow-multiple-definition"
    eval "c++ $COMPILE $extra -c '$HERE/w2harness.cpp' -o '$OUT/w2harness-$kind.o'"
    eval "$(link_line "$OUT/w2harness-$kind.o" "$OUT/w2harness-$kind") $link"
done
ls -la "$OUT"
