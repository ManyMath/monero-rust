"""Reorg and congestion checks on a private, offline fork of a synced node's chain.

    scripts/stagenet_fork.py status

MONERO_FORK_RPC, MONERO_FORK_START, MONERO_FORK_PIDFILE and MONERO_SOURCE_RPC
name the fork node, its start script, its pid file and the read-only source
node. MONERO_FORK_TOOLS maps labels to crate tools: "A=/path/a,B=/path/b".
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagenet_lib as lib  # noqa: E402

RUN = lib.RUN
EVID = os.path.join(RUN, "evidence")
RINGDB = os.path.join(RUN, "ringdb")
LOOKAHEAD = {"lookahead_account": 49, "lookahead_subaddress": 199}
WALLET = os.environ.get("MONERO_FORK_WALLET", "hemlock")
RESTORE_HEIGHT = int(os.environ.get("MONERO_FORK_RESTORE_HEIGHT", "0"))


def env(name):
    value = os.environ.get(name)
    if not value:
        raise SystemExit("set %s" % name)
    return value.rstrip("/")


def fork_rpc():
    return env("MONERO_FORK_RPC")


def source_rpc():
    return env("MONERO_SOURCE_RPC")


def addr_tool():
    return next(iter(tools()))


def tools():
    out = {}
    for item in env("MONERO_FORK_TOOLS").split(","):
        label, path = item.split("=", 1)
        out[label] = path
    return out


def log(*args):
    line = time.strftime("%H:%M:%S ") + " ".join(str(a) for a in args)
    print(line, flush=True)
    os.makedirs(EVID, exist_ok=True)
    with open(os.path.join(EVID, "fork-log.txt"), "a") as f:
        f.write(line + "\n")


def save(name, data):
    path = os.path.join(EVID, name + ".json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(data, f, indent=1, sort_keys=True)
    return path


def load(name):
    with open(os.path.join(EVID, name + ".json")) as f:
        return json.load(f)


def jrpc(url, method, params=None, timeout=120):
    body = json.dumps({"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}}).encode()
    with urllib.request.urlopen(urllib.request.Request(url + "/json_rpc", body), timeout=timeout) as r:
        reply = json.load(r)
    if "error" in reply:
        raise RuntimeError("%s: %s" % (method, reply["error"]))
    return reply["result"]


def other(url, path, params=None, timeout=120):
    body = json.dumps(params or {}).encode()
    with urllib.request.urlopen(urllib.request.Request(url + "/" + path, body), timeout=timeout) as r:
        return json.load(r)


def info(url):
    return jrpc(url, "get_info")


def header_at(url, height):
    return jrpc(url, "get_block_header_by_height", {"height": height})["block_header"]


def connections(url):
    return jrpc(url, "get_connections").get("connections", []) or []


def pool_ids(url):
    return sorted(other(url, "get_transaction_pool_hashes").get("tx_hashes", []) or [])


def fork_pid():
    try:
        with open(env("MONERO_FORK_PIDFILE")) as f:
            return int(f.read().strip())
    except (OSError, ValueError):
        return None


def alive(pid):
    if not pid:
        return False
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    with open("/proc/%d/cmdline" % pid, "rb") as f:
        return b"monerod" in f.read()


def node_summary(url):
    i = info(url)
    keys = ("height", "top_block_hash", "cumulative_difficulty", "difficulty", "offline", "synchronized",
            "outgoing_connections_count", "incoming_connections_count", "alt_blocks_count", "tx_pool_size",
            "block_weight_limit", "block_weight_median", "busy_syncing", "target_height")
    return {k: i.get(k) for k in keys}


def stop_fork():
    pid = fork_pid()
    log("stopping fork node pid", pid)
    try:
        other(fork_rpc(), "stop_daemon")
    except Exception as exc:  # noqa: BLE001
        log("stop_daemon:", repr(exc))
    for _ in range(600):
        if not alive(pid):
            log("fork node stopped")
            return
        time.sleep(0.5)
    raise SystemExit("fork node pid %s did not stop" % pid)


def start_fork(extra):
    assert not alive(fork_pid()), "fork node is already running"
    cmd = [env("MONERO_FORK_START")] + list(extra)
    log("starting fork node:", " ".join(cmd[1:]) or "(normal settings)")
    subprocess.check_call(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(1200):
        try:
            s = node_summary(fork_rpc())
            log("fork node up, pid", fork_pid(), json.dumps(s))
            return s
        except Exception:  # noqa: BLE001
            time.sleep(0.5)
    raise SystemExit("fork node did not answer RPC")


def listening_ports(pid):
    out = subprocess.run(["ss", "-ltnpH"], capture_output=True, text=True).stdout
    return sorted({line.split()[3] for line in out.splitlines() if "pid=%d," % pid in line})


def assert_private():
    s = node_summary(fork_rpc())
    conns = connections(fork_rpc())
    ports = listening_ports(fork_pid())
    log("private check: offline", s["offline"], "connections", len(conns), "listening", ports)
    assert s["offline"] is True, "fork node is not offline"
    assert not conns, "fork node has connections"
    allowed = set(os.environ.get("MONERO_FORK_RPC_PORTS", fork_rpc().rsplit(":", 1)[1]).split(","))
    assert all(p.rsplit(":", 1)[1] in allowed for p in ports), "fork node listens on %s" % ports
    return {"summary": s, "connections": conns, "listening": ports}


def wait_synced(timeout=1800):
    started = time.time()
    while time.time() - started < timeout:
        try:
            f, m = info(fork_rpc()), info(source_rpc())
            if f["height"] == m["height"] and f["top_block_hash"] == m["top_block_hash"]:
                return f["height"], f["top_block_hash"]
        except Exception:  # noqa: BLE001
            pass
        time.sleep(5)
    raise SystemExit("fork node did not reach the source tip")


def go_private(tag):
    height, top = wait_synced()
    base = {"fork_height": height, "fork_top": top, "source": node_summary(source_rpc()),
            "fork_before": node_summary(fork_rpc()), "fork_connections_before": connections(fork_rpc())}
    log("going private at height", height, top)
    stop_fork()
    start_fork(["--offline", "--fixed-difficulty", "1"])
    base["private_check"] = assert_private()
    base["foreign_pool_flushed"] = pool_ids(fork_rpc())
    if base["foreign_pool_flushed"]:
        jrpc(fork_rpc(), "flush_txpool", {"txids": base["foreign_pool_flushed"]})
        log("flushed public transactions from the offline pool:", base["foreign_pool_flushed"])
    save("%s/private-start" % tag, base)
    return base


def mine(count, address, tag=None):
    assert node_summary(fork_rpc())["offline"] is True, "refusing to mine on a node that is online"
    mined = []
    for _ in range(count):
        t = jrpc(fork_rpc(), "get_block_template", {"wallet_address": address, "reserve_size": 0})
        r = jrpc(fork_rpc(), "submit_block", [t["blocktemplate_blob"]])
        assert r.get("status") == "OK", r
        mined.append({"height": t["height"], "hash": r.get("block_id"), "difficulty": t["difficulty"]})
    s = node_summary(fork_rpc())
    log("mined", count, "blocks, fork height now", s["height"], "pool", s["tx_pool_size"])
    if tag:
        path = os.path.join(EVID, tag, "mined.jsonl")
        with open(path, "a") as f:
            for m in mined:
                f.write(json.dumps(m) + "\n")
    return mined


def block_weights(url, start, end):
    hs = jrpc(url, "getblockheadersrange", {"start_height": start, "end_height": end - 1})["headers"]
    return [(h["height"], h["block_weight"], h["num_txes"]) for h in hs]


def feed_real_blocks(fork_height, tag, max_blocks=50):
    stop_fork()
    start_fork(["--offline"])
    assert_private()
    before = node_summary(fork_rpc())
    fed = []
    h = fork_height
    src_tip = info(source_rpc())["height"]
    while h < src_tip and len(fed) < max_blocks:
        blk = jrpc(source_rpc(), "get_block", {"height": h})
        txs = blk.get("tx_hashes", []) or []
        added = []
        if txs:
            got = other(source_rpc(), "get_transactions", {"txs_hashes": txs, "decode_as_json": False})
            for t in got.get("txs", []):
                r = other(fork_rpc(), "send_raw_transaction", {"tx_as_hex": t["as_hex"], "do_not_relay": True})
                added.append({"tx": t["tx_hash"], "status": r.get("status"), "reason": r.get("reason"),
                              "double_spend": r.get("double_spend")})
        r = jrpc(fork_rpc(), "submit_block", [blk["blob"]])
        s = node_summary(fork_rpc())
        fed.append({"height": h, "hash": blk["block_header"]["hash"], "txs": added, "submit": r,
                    "fork_height_after": s["height"], "fork_top_after": s["top_block_hash"]})
        log("fed real block", h, blk["block_header"]["hash"][:16], "txs", len(txs), "->", s["height"],
            s["top_block_hash"][:16])
        if s["top_block_hash"] == blk["block_header"]["hash"]:
            break
        h += 1
    after = node_summary(fork_rpc())
    on_real = header_at(source_rpc(), after["height"] - 1)["hash"] == after["top_block_hash"]
    data = {"before": before, "fed": fed, "after": after, "on_real_chain": on_real,
            "pool_after": pool_ids(fork_rpc())}
    save("%s/switch" % tag, data)
    log("switch done: on real chain", on_real, "pool", len(data["pool_after"]))
    assert on_real, "fork node did not switch to the real chain"
    return data


def flush_pool(keep=()):
    ids = pool_ids(fork_rpc())
    drop = [t for t in ids if t not in keep]
    if drop:
        jrpc(fork_rpc(), "flush_txpool", {"txids": drop})
    left = pool_ids(fork_rpc())
    log("flushed", len(drop), "pool now", left)
    return left


def go_public(tag, allow=()):
    s = node_summary(fork_rpc())
    top_on_source = header_at(source_rpc(), s["height"] - 1)["hash"]
    pool = pool_ids(fork_rpc())
    checks = {"fork": s, "source_hash_at_fork_top": top_on_source, "pool": pool, "allow": list(allow)}
    assert top_on_source == s["top_block_hash"], "fork node's tip is not on the real chain"
    assert sorted(pool) == sorted(allow), "pool holds %s, allowed %s" % (pool, list(allow))
    if allow:
        got = other(fork_rpc(), "get_transactions", {"txs_hashes": list(allow), "decode_as_json": True})
        source_pool = pool_ids(source_rpc())
        images = []
        for t in got["txs"]:
            assert t["tx_hash"] not in source_pool
            for vin in json.loads(t["as_json"])["vin"]:
                images.append(vin["key"]["k_image"])
        spent = other(source_rpc(), "is_key_image_spent", {"key_images": images})
        checks["allowed_key_images"] = images
        checks["source_spent_status"] = spent.get("spent_status")
        assert all(x == 0 for x in spent["spent_status"]), "an allowed input is spent or pooled on the source"
    save("%s/public-checks" % tag, checks)
    stop_fork()
    start_fork([])
    for _ in range(120):
        conns = connections(fork_rpc())
        if conns:
            break
        time.sleep(1)
    peers = sorted({"%s:%s" % (c.get("host"), c.get("port")) for c in conns})
    log("online, connections", peers)
    height, top = wait_synced()
    relayed = None
    if allow:
        relayed = jrpc(fork_rpc(), "relay_tx", {"txids": list(allow)})
        log("relay_tx", relayed)
    after = {"connections": conns, "synced": [height, top], "relay": relayed,
             "source": node_summary(source_rpc()), "fork": node_summary(fork_rpc())}
    save("%s/public-after" % tag, after)
    log("fork node synced at", height, "source alt blocks", after["source"]["alt_blocks_count"])
    return after


def w2_open(label, daemon=None):
    w = lib.WalletRpc(os.path.join(RUN, "w2", label), RINGDB,
                      daemon=(daemon or fork_rpc()).replace("http://", ""), log_level=1)
    w.call("open_wallet", {"filename": "w", "password": ""})
    w.call("auto_refresh", {"enable": False})
    return w


def w2_view(w):
    out = {"height": w.call("get_height")["height"]}
    accounts = w.call("get_accounts")
    out["accounts"] = accounts
    out["balance"] = w.call("get_balance", {"account_index": 0, "all_accounts": True})
    out["transfers"] = w.call("get_transfers", {"in": True, "out": True, "pending": True, "failed": True,
                                                "pool": True, "all_accounts": True})
    incoming = {}
    for account in accounts.get("subaddress_accounts", []):
        index = account["account_index"]
        incoming[str(index)] = w.call("incoming_transfers", {"transfer_type": "all", "account_index": index,
                                                             "verbose": True}).get("transfers", [])
    out["incoming"] = incoming
    return out


def w2_refresh(label, name, daemon=None):
    w = w2_open(label, daemon)
    try:
        started = time.time()
        r = w.raw("refresh", timeout=3600)
        view = w2_view(w)
        view["refresh"] = r
        view["refresh_seconds"] = time.time() - started
        save(name, view)
        log("wallet2", label, "refresh", json.dumps(r)[:300], "height", view["height"],
            "balance", view["balance"]["balance"])
        return view
    finally:
        w.stop()


def tool(label, request, name):
    request = dict(request)
    request.setdefault("network", lib.NETWORK)
    request.setdefault("rpc", fork_rpc())
    os.makedirs(os.path.join(RUN, "req"), exist_ok=True)
    req_path = os.path.join(RUN, "req", name.replace("/", "_") + ".json")
    out_path = os.path.join(RUN, "mr", name.replace("/", "_") + ".json")
    with open(req_path, "w") as f:
        json.dump(request, f, indent=1)
    if os.path.exists(out_path):
        os.remove(out_path)
    with open(os.path.join(RUN, "logs", name.replace("/", "_") + ".log"), "ab") as logf:
        code = subprocess.call([tools()[label], req_path, out_path], stdout=logf, stderr=logf)
    if not os.path.exists(out_path):
        return {"tool_exit": code, "error": "no response (exit %d), see logs/%s.log" % (code, name.replace("/", "_"))}
    with open(out_path) as f:
        out = json.load(f)
    out["tool_exit"] = code
    return out


def state_path(label, lineage):
    return os.path.join(RUN, "mr", "%s-%s-state.json" % (label, lineage))


def ring_db_path(label, lineage):
    return os.path.join(RUN, "mr", "%s-%s-rings.json" % (label, lineage))


def crate_scan(label, lineage, name, **extra):
    req = dict({"op": "scan", "seed_file": lib.seed_file(WALLET), "state": state_path(label, lineage),
                "ring_db": ring_db_path(label, lineage), "restore_height": RESTORE_HEIGHT}, **LOOKAHEAD)
    req.update(extra)
    r = tool(label, req, name)
    save(name, r)
    log("crate", label, lineage, "scan ->", {k: r.get(k) for k in ("height", "error", "reorgs", "balance")})
    return r


def norm_crate(r):
    outs = {}
    for o in r.get("outputs", []):
        outs[(o["tx_hash"], o["key"])] = {
            "amount": o["amount"], "height": o["block_height"],
            "sub": list(o["subaddress_index"] or [0, 0]), "global_index": o["global_index"],
            "key_image": o["key_image"], "spent": o["spent"], "spent_height": o.get("spent_height"),
            "spent_by": o.get("spent_by_tx")}
    return outs


def norm_w2(view):
    outs = {}
    for _, transfers in view["incoming"].items():
        for t in transfers:
            outs[(t["tx_hash"], t["pubkey"])] = {
                "amount": t["amount"], "height": t["block_height"],
                "sub": [t["subaddr_index"]["major"], t["subaddr_index"]["minor"]],
                "global_index": t["global_index"], "key_image": t["key_image"], "spent": t["spent"]}
    return outs


def diff_maps(a, b, fields=None):
    only_a = sorted(set(a) - set(b))
    only_b = sorted(set(b) - set(a))
    diffs = []
    for k in sorted(set(a) & set(b)):
        x, y = a[k], b[k]
        keys = fields or sorted(set(x) | set(y))
        d = {f: [x.get(f), y.get(f)] for f in keys if x.get(f) != y.get(f)}
        if d:
            diffs.append({"key": list(k), "diffs": d})
    return {"only_first": [list(k) for k in only_a], "only_second": [list(k) for k in only_b],
            "field_diffs": diffs, "compared": len(set(a) & set(b)), "equal": not (only_a or only_b or diffs)}


def w2_transfers_norm(view):
    rows = []
    for kind, items in view["transfers"].items():
        for t in items:
            rows.append((kind, t["txid"], t["height"], t["amount"], t["fee"], t["subaddr_index"]["major"],
                         t["subaddr_index"]["minor"]))
    return sorted(set(rows))


def compare_w2(main, ref):
    rows_main, rows_ref = set(w2_transfers_norm(main)), set(w2_transfers_norm(ref))
    return {"outputs": diff_maps(norm_w2(main), norm_w2(ref)),
            "balance": [main["balance"]["balance"], ref["balance"]["balance"],
                        main["balance"]["unlocked_balance"], ref["balance"]["unlocked_balance"]],
            "balance_equal": (main["balance"]["balance"] == ref["balance"]["balance"]
                              and main["balance"]["unlocked_balance"] == ref["balance"]["unlocked_balance"]),
            "transfers_only_main": sorted(rows_main - rows_ref),
            "transfers_only_ref": sorted(rows_ref - rows_main),
            "height": [main["height"], ref["height"]]}


def compare_crate(main, ref):
    m, r = norm_crate(main), norm_crate(ref)
    return {"outputs": diff_maps(m, r), "balance": [main.get("balance"), ref.get("balance")],
            "balance_equal": main.get("balance") == ref.get("balance"),
            "height": [main.get("height"), ref.get("height")]}


def compare_crate_w2(crate, view):
    c = norm_crate(crate)
    w = norm_w2(view)
    return diff_maps(c, w, ["amount", "height", "sub", "global_index", "key_image", "spent"])


def tx_rings(tx_hex):
    tx = lib.decode_tx(tx_hex)
    return {vin["key_image"]: vin["indexes"] for vin in tx["inputs"] if vin["type"] == "key"}


def fork_tx_hex(txid, url=None):
    got = other(url or fork_rpc(), "get_transactions", {"txs_hashes": [txid], "decode_as_json": False})
    txs = got.get("txs", [])
    return txs[0]["as_hex"] if txs else None


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("status")
    p = sub.add_parser("go-private")
    p.add_argument("tag")
    p = sub.add_parser("mine")
    p.add_argument("count", type=int)
    p.add_argument("address")
    p.add_argument("--tag")
    p = sub.add_parser("switch")
    p.add_argument("fork_height", type=int)
    p.add_argument("tag")
    p = sub.add_parser("flush")
    p.add_argument("--keep", nargs="*", default=[])
    p = sub.add_parser("go-public")
    p.add_argument("tag")
    p.add_argument("--allow", nargs="*", default=[])
    args = ap.parse_args()
    if args.cmd == "status":
        print(json.dumps({"fork": node_summary(fork_rpc()), "fork_pid": fork_pid(),
                          "fork_connections": connections(fork_rpc()), "fork_pool": pool_ids(fork_rpc()),
                          "source": node_summary(source_rpc())}, indent=1))
    elif args.cmd == "go-private":
        go_private(args.tag)
    elif args.cmd == "mine":
        mine(args.count, args.address, args.tag)
    elif args.cmd == "switch":
        feed_real_blocks(args.fork_height, args.tag)
    elif args.cmd == "flush":
        flush_pool(args.keep)
    elif args.cmd == "go-public":
        go_public(args.tag, args.allow)


if __name__ == "__main__":
    main()
