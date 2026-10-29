#!/usr/bin/env python3
"""When does wallet2 fix a ring? The baseline for tests/ring_reuse_*.rs.

    eval "$(scripts/regtest-node.sh start)"
    scripts/wallet2-ring-reuse.py <daemon_rpc_port> <monero-wallet-rpc> <work_dir>
"""
import json, os, socket, subprocess, sys, time, urllib.request

DAEMON_PORT, WALLET_RPC, WORK = int(sys.argv[1]), sys.argv[2], sys.argv[3]
MINER_SEED = ("velvet lymph giddy number token physics poetry unquoted nibs useful sabotage limits "
              "benches lifestyle eden nitrogen anvil fewest avoid batch vials washing fences goat unquoted")
XMR = 10**12

def post(port, path, body):
    req = urllib.request.Request(f"http://127.0.0.1:{port}/{path}", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.load(r)

def rpc(port, method, params=None):
    out = post(port, "json_rpc", {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}})
    if "error" in out:
        raise RuntimeError(f"{method}: {out['error']}")
    return out["result"]

def varint(b, i):
    v, shift = 0, 0
    while True:
        byte = b[i]; i += 1
        v |= (byte & 0x7f) << shift
        if not byte & 0x80:
            return v, i
        shift += 7

def rings(tx_hex):
    b = bytes.fromhex(tx_hex)
    _, i = varint(b, 0)          # version
    _, i = varint(b, i)          # unlock time
    count, i = varint(b, i)
    out = {}
    for _ in range(count):
        assert b[i] == 2, "txin_to_key"
        i += 1
        _, i = varint(b, i)      # amount
        n, i = varint(b, i)
        absolute, total = [], 0
        for _ in range(n):
            off, i = varint(b, i)
            total += off
            absolute.append(total)
        out[b[i:i + 32].hex()] = absolute
        i += 32
    return out

def free_port():
    s = socket.socket(); s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close(); return p

def height():
    return post(DAEMON_PORT, "get_height", {})["height"]

def pool():
    return post(DAEMON_PORT, "get_transaction_pool_hashes", {}).get("tx_hashes", [])

os.makedirs(WORK, exist_ok=True)
port = free_port()
log = open(os.path.join(WORK, "wallet-rpc.stdout"), "w")
proc = subprocess.Popen([WALLET_RPC, "--daemon-address", f"127.0.0.1:{DAEMON_PORT}",
                         "--trusted-daemon", "--allow-mismatched-daemon-version", "--wallet-dir", os.path.join(WORK, "wallets"),
                         "--shared-ringdb-dir", os.path.join(WORK, "ringdb"),
                         "--disable-rpc-login", "--rpc-bind-ip", "127.0.0.1", "--rpc-bind-port", str(port),
                         "--log-file", os.path.join(WORK, "wallet-rpc.log"), "--log-level", "1"],
                        stdout=log, stderr=subprocess.STDOUT)
print("wallet-rpc pid", proc.pid, "port", port, flush=True)
try:
    for _ in range(60):
        try:
            rpc(port, "get_version"); break
        except Exception:
            time.sleep(1)
    tag = str(int(time.time()))
    rpc(port, "restore_deterministic_wallet", {"filename": "miner-" + tag, "seed": MINER_SEED,
                                               "password": "", "restore_height": 0})
    miner_address = rpc(port, "get_address", {"account_index": 0})["address"]
    def mine(n):
        rpc(DAEMON_PORT, "generateblocks", {"amount_of_blocks": n, "wallet_address": miner_address})
    if height() < 750:
        mine(750 - height())
    rpc(port, "refresh")
    rpc(port, "close_wallet")
    rpc(port, "create_wallet", {"filename": "spender-" + tag, "password": "", "language": "English"})
    spender_address = rpc(port, "get_address", {"account_index": 0})["address"]
    rpc(port, "close_wallet")

    rpc(port, "open_wallet", {"filename": "miner-" + tag, "password": ""})
    rpc(port, "refresh")
    funding = rpc(port, "transfer", {"destinations": [{"amount": 5 * XMR, "address": spender_address}],
                                      "get_tx_hex": True})
    rpc(port, "close_wallet")
    received_at = height()
    mine(12)
    rpc(port, "open_wallet", {"filename": "spender-" + tag, "password": ""})
    rpc(port, "refresh")
    assert rpc(port, "get_balance")["unlocked_balance"] == 5 * XMR

    def build(amount, relay):
        t = rpc(port, "transfer", {"destinations": [{"amount": amount, "address": miner_address}],
                                   "get_tx_hex": True, "do_not_relay": not relay})
        r = rings(t["tx_blob"])
        assert len(r) == 1
        (key_image, ring), = r.items()
        return t["tx_hash"], key_image, ring

    result = {"wallet_rpc": rpc(port, "get_version")}

    _, ki_a1, ring_a1 = build(1 * XMR, relay=False)
    _, ki_a2, ring_a2 = build(2 * XMR, relay=False)
    assert ki_a1 == ki_a2
    result["A_unrelayed_rebuild"] = {"key_image": ki_a1, "first": ring_a1, "second": ring_a2,
                                     "same_ring": ring_a1 == ring_a2}

    txid_b1, ki_b1, ring_b1 = build(1 * XMR, relay=True)
    assert txid_b1 in pool()
    rpc(DAEMON_PORT, "flush_txpool", {"txids": [txid_b1]})
    assert txid_b1 not in pool()
    post(DAEMON_PORT, "pop_blocks", {"nblocks": height() - received_at})
    assert height() == received_at
    if funding["tx_hash"] not in pool():
        post(DAEMON_PORT, "send_raw_transaction", {"tx_as_hex": funding["tx_blob"]})
    mine(14)
    assert pool() == []
    respend = None
    for attempt in range(20):
        rpc(port, "refresh")
        try:
            respend = build(1 * XMR, relay=True)
            break
        except RuntimeError as e:
            result.setdefault("B_respend_errors", []).append(str(e))
            time.sleep(3)
    if respend is None:
        rpc(port, "rescan_spent")
        respend = build(1 * XMR, relay=True)
        result["B_needed_rescan_spent"] = True
    txid_b2, ki_b2, ring_b2 = respend
    result["B_reorg_of_receiving_block"] = {"key_image_same": ki_b1 == ki_b2, "first": ring_b1,
                                            "second": ring_b2, "same_ring": ring_b1 == ring_b2,
                                            "respend_in_pool": txid_b2 in pool()}
    mine(1)
    rpc(port, "close_wallet")
    print(json.dumps(result, indent=1))
finally:
    try:
        rpc(port, "stop_wallet")
    except Exception:
        pass
    try:
        proc.wait(timeout=20)
    except Exception:
        proc.terminate()
