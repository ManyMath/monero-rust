#!/bin/sh
# Manage a private regtest monerod for integration tests.
#
# Choose available ports and keep the chain in a separate data directory.
# `start` prints an assignment for MONERO_REGTEST_RPC, which the tests use
# to find this daemon.
#
#   eval "$(scripts/regtest-node.sh start)"   # start and export MONERO_REGTEST_RPC
#   cargo test -p monero-rust --test regtest_e2e_test
#   scripts/regtest-node.sh stop
#
# `status` prints the URL if RPC is available, otherwise exits nonzero.
# State lives in $MONERO_REGTEST_DIR, defaulting to monero-rust-regtest under
# TMPDIR (or /tmp). `reset` stops the node and deletes its chain data.
set -eu

STATE_DIR="${MONERO_REGTEST_DIR:-${TMPDIR:-/tmp}/monero-rust-regtest}"
PORT_FILE="$STATE_DIR/rpc-port"
PID_FILE="$STATE_DIR/monerod.pid"
LOG_FILE="$STATE_DIR/monerod.log"
MONEROD="${MONEROD:-monerod}"

free_port() {
    # Ask the kernel to select an available port.
    python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

node_url() {
    [ -f "$PORT_FILE" ] || return 1
    echo "http://127.0.0.1:$(cat "$PORT_FILE")"
}

is_running() {
    [ -f "$PID_FILE" ] || return 1
    kill -0 "$(cat "$PID_FILE")" 2>/dev/null
}

wait_for_rpc() {
    url="$1"
    i=0
    while [ "$i" -lt 60 ]; do
        if curl -s -m 2 "$url/get_info" | grep -q '"status"'; then
            return 0
        fi
        i=$((i + 1))
        sleep 1
    done
    echo "monerod did not answer on $url within 60s; see $LOG_FILE" >&2
    return 1
}

cmd_start() {
    if is_running; then
        url="$(node_url)"
        echo "export MONERO_REGTEST_RPC=$url"
        return 0
    fi

    command -v "$MONEROD" >/dev/null 2>&1 || {
        echo "monerod not found on PATH; set MONEROD to its location" >&2
        return 1
    }

    mkdir -p "$STATE_DIR/data"
    rpc_port="$(free_port)"
    p2p_port="$(free_port)"
    zmq_port="$(free_port)"

    # Keep the daemon offline and set difficulty to 1 for fast local mining,
    # as in the cold-signing vector generator.
    "$MONEROD" --regtest --fixed-difficulty 1 \
        --rpc-bind-ip 127.0.0.1 --rpc-bind-port "$rpc_port" \
        --p2p-bind-port "$p2p_port" --zmq-rpc-bind-port "$zmq_port" \
        --data-dir "$STATE_DIR/data" \
        --non-interactive --offline --disable-dns-checkpoints \
        --check-updates disabled --rpc-ssl disabled \
        --no-igd --hide-my-port --log-level 1 \
        >"$LOG_FILE" 2>&1 &

    echo $! >"$PID_FILE"
    echo "$rpc_port" >"$PORT_FILE"

    url="http://127.0.0.1:$rpc_port"
    wait_for_rpc "$url" || { cmd_stop >/dev/null 2>&1 || true; return 1; }
    echo "export MONERO_REGTEST_RPC=$url"
}

cmd_stop() {
    if is_running; then
        kill "$(cat "$PID_FILE")" 2>/dev/null || true
        i=0
        while is_running && [ "$i" -lt 30 ]; do
            i=$((i + 1))
            sleep 1
        done
    fi
    rm -f "$PID_FILE" "$PORT_FILE"
    echo "stopped" >&2
}

cmd_status() {
    if is_running && url="$(node_url)"; then
        echo "$url"
        return 0
    fi
    echo "no regtest node running (state: $STATE_DIR)" >&2
    return 1
}

cmd_reset() {
    cmd_stop >/dev/null 2>&1 || true
    rm -rf "$STATE_DIR/data"
    echo "chain discarded" >&2
}

case "${1:-}" in
    start) cmd_start ;;
    stop) cmd_stop ;;
    status) cmd_status ;;
    reset) cmd_reset ;;
    *)
        echo "usage: $0 start|stop|status|reset" >&2
        exit 2
        ;;
esac
