#!/bin/sh
set -e
M="${MONERO_SRC:-$HOME/src/monero/monero}"
B="${MONERO_BUILD:-$M/build/Linux/master/release}"
OUT="${1:-harness}"
SRC="$(dirname "$0")/harness.cpp"

g++ -std=c++17 -DAUTO_INITIALIZE_EASYLOGGINGPP -DNDEBUG -O2 -maes -march=native \
  -I"$M/external/rapidjson/include" -I"$M/external/easylogging++" -I"$M/src" \
  -I"$M/contrib/epee/include" -I"$M/external" -I"$M/external/supercop/include" \
  -I"$B/generated_include" \
  "$SRC" -o "$OUT" \
  "$B/src/cryptonote_basic/libcryptonote_basic.a" \
  "$B/src/cryptonote_basic/libcryptonote_format_utils_basic.a" \
  "$B/src/device/libdevice.a" \
  "$B/src/ringct/libringct_basic.a" \
  "$B/src/crypto/libcncrypto.a" \
  "$B/src/crypto/wallet/libwallet-crypto.a" \
  "$B/src/common/libcommon.a" \
  "$B/contrib/epee/src/libepee.a" \
  "$B/external/easylogging++/libeasylogging.a" \
  "$B/src/libversion.a" \
  "$B/external/randomx/librandomx.a" \
  -lunbound -lhidapi-libusb -lusb-1.0 \
  -lboost_system -lboost_thread -lboost_filesystem -lboost_program_options \
  -lboost_regex -lboost_chrono -lboost_serialization \
  -lssl -lcrypto -lsodium -lpthread -ldl
