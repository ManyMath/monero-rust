#!/bin/sh
# Build w2_extra_send out of tree against a monero v0.18.5.3 build directory.
#   MONERO_BUILD=<cmake build dir> [OUT=<dir>] scripts/prerct/build_w2_extra_send.sh
set -eu

BLD="${MONERO_BUILD:?set MONERO_BUILD to a monero build directory}"
OUT="${OUT:-${PRERCT_RUN_DIR:?set OUT or PRERCT_RUN_DIR}/harness}"
HERE="$(cd "$(dirname "$0")" && pwd)"
TARGET="$BLD/src/wallet/CMakeFiles/wallet_rpc_server.dir"

[ -f "$TARGET/flags.make" ] && [ -f "$TARGET/link.txt" ] || {
    echo "$TARGET has no flags.make and link.txt; build the wallet_rpc_server target first" >&2
    exit 1
}

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
flags() { sed -n "s/^$1 = //p" "$TARGET/flags.make"; }

cd "$BLD/src/wallet"
eval "${CXX:-c++} $(flags CXX_DEFINES) $(flags CXX_INCLUDES) $(flags CXX_FLAGS) -w \
    -c '$HERE/w2_extra_send.cpp' -o '$OUT/w2_extra_send.o'"
eval "$(sed -e "s#CMakeFiles/wallet_rpc_server.dir/wallet_rpc_server.cpp.o#'$OUT/w2_extra_send.o'#" \
    -e "s#-o [^ ]*monero-wallet-rpc#-o '$OUT/w2_extra_send'#" "$TARGET/link.txt")"
echo "$OUT/w2_extra_send"
