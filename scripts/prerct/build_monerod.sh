#!/bin/sh
# Build a monerod with a stageable regtest fork table, for scripts/prerct_chain.py.
#   MONERO_SRC=<monero v0.18.5.3 checkout> OUT=<dir> scripts/prerct/build_monerod.sh
set -eu

SRC="${MONERO_SRC:?set MONERO_SRC to a monero checkout at v0.18.5.3}"
OUT="${OUT:?set OUT to a build directory}"
HERE="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$OUT"
rsync -a --delete --exclude=/build --exclude=/.git "$SRC/" "$OUT/src/"
patch -d "$OUT/src" -p1 <"$HERE/staged-regtest-forks.patch"
cmake -S "$OUT/src" -B "$OUT/build" -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTS=OFF \
    -DBUILD_DOCUMENTATION=OFF -DMANUAL_SUBMODULES=1
cmake --build "$OUT/build" --target daemon -- -j"${JOBS:-4}"
echo "$OUT/build/bin/monerod"
