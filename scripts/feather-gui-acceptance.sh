#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
feather_appimage="${FEATHER_APPIMAGE:-/home/user/Downloads/feather-2.8.1.AppImage}"
timeout_s="${FEATHER_GUI_TIMEOUT:-45}"
vector_dir="$repo_root/rust/monero-rust/tests/vectors/cold_signing_regtest_v0_18_5_0"
wallet_keys="$vector_dir/cold_full.keys"
unsigned_tx="$vector_dir/unsigned_monero_tx"

skip() {
  echo "SKIP: $*" >&2
  exit 77
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || skip "$1 is required"
}

need_cmd timeout
need_cmd xvfb-run
need_cmd xdotool

[[ -x "$feather_appimage" ]] || skip "FEATHER_APPIMAGE is not executable: $feather_appimage"
[[ -r "$wallet_keys" ]] || skip "missing wallet keys vector: $wallet_keys"
[[ -r "$unsigned_tx" ]] || skip "missing unsigned tx vector: $unsigned_tx"

work="$(mktemp -d)"
cleanup() {
  rm -rf "$work"
}
trap cleanup EXIT

home_dir="$work/home"
wallet_dir="$home_dir/Monero/wallets"
settings_dir="$home_dir/.config/feather"
mkdir -p "$wallet_dir" "$settings_dir"
cp "$wallet_keys" "$wallet_dir/cold_full.keys"

cat >"$settings_dir/settings.json" <<JSON
{
  "firstRun": false,
  "walletDirectory": "$wallet_dir",
  "warnOnStagenet": false,
  "warnOnTestnet": false,
  "disableLogging": true,
  "offlineTxSigningMethod": 0,
  "useLocalTor": true,
  "skin": "light",
  "lastPath": "$vector_dir"
}
JSON

log="$work/feather.log"

timeout "$timeout_s" xvfb-run -a -s '-screen 0 1280x900x24' bash -s -- \
  "$home_dir" "$log" "$feather_appimage" "$unsigned_tx" <<'BASH'
set -euo pipefail

home_dir="$1"
log="$2"
feather_appimage="$3"
unsigned_tx="$4"

export HOME="$home_dir"
export QT_X11_NO_MITSHM=1

"$feather_appimage" --quiet >"$log" 2>&1 &
feather_pid=$!

cleanup() {
  kill "$feather_pid" 2>/dev/null || true
  wait "$feather_pid" 2>/dev/null || true
}
trap cleanup EXIT

find_window() {
  local pattern="$1"
  local id name
  for id in $(xdotool search --onlyvisible --name '.*' 2>/dev/null || true); do
    name="$(xdotool getwindowname "$id" 2>/dev/null || true)"
    case "$name" in
      *"$pattern"*) echo "$id"; return 0 ;;
    esac
  done
  return 1
}

wait_window() {
  local pattern="$1"
  local id=""
  for _ in $(seq 1 120); do
    id="$(find_window "$pattern" || true)"
    if [[ -n "$id" ]]; then
      echo "$id"
      return 0
    fi
    sleep 0.25
  done
  echo "Timed out waiting for window matching: $pattern" >&2
  xdotool search --onlyvisible --name '.*' getwindowname %@ 2>/dev/null || true
  return 1
}

welcome="$(wait_window 'Welcome to Feather Wallet')"
xdotool key Return
sleep 0.5
xdotool key Return

main="$(wait_window 'cold_full')"
echo "Opened Feather wallet window: $(xdotool getwindowname "$main" 2>/dev/null)"

# Open Tools > Key image sync. Coordinates are relative to the deterministic
# 1280x900 Xvfb run and Feather 2.8.1's default main-window layout.
xdotool mousemove --window "$main" 168 12 click 1
sleep 0.4
xdotool key Down Down Down Return

wizard="$(wait_window 'Offline transaction signing')"
echo "Opened Feather offline signing wizard"

# Switch Method from "Animated QR codes" to "File transfer".
xdotool mousemove --window "$wizard" 136 66 click 1
sleep 0.2
xdotool key Down Return
sleep 0.8

# Import from file.
xdotool mousemove --window "$wizard" 63 412 click 1
file_dialog="$(wait_window 'Import outputs or unsigned transactions file')"
echo "Opened Feather unsigned-tx file dialog"

xdotool key ctrl+l
sleep 0.1
xdotool type --delay 0 "$unsigned_tx"
xdotool key Return

transaction_dialog="$(wait_window 'Transaction')"
echo "Imported unsigned_monero_tx into Feather dialog: $(xdotool getwindowname "$transaction_dialog" 2>/dev/null)"
BASH

if grep -E 'Failed to import|Unable to|Failed to decrypt' "$log" >/dev/null; then
  echo "Feather reported an import failure:" >&2
  grep -E 'Failed to import|Unable to|Failed to decrypt' "$log" >&2
  exit 1
fi
