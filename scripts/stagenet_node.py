#!/usr/bin/env python3
"""Compare monero-rust with wallet2 against restricted and pruned remote nodes.

    MONERO_NODE_FULL_RPC=<url> scripts/stagenet_node.py scan full LABEL

Endpoints full, full-r, pruned and pruned-r come from MONERO_NODE_FULL_RPC,
MONERO_NODE_FULL_RESTRICTED_RPC, MONERO_NODE_PRUNED_RPC and
MONERO_NODE_PRUNED_RESTRICTED_RPC. `relay` and `w2-relay` first ask
`--guard-endpoint` (default `full`) about the input's key image, so point it
at a node you run.
"""
import argparse
import http.client
import json
import os
import struct
import subprocess
import sys
import threading
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagenet_lib as lib  # noqa: E402
import stagenet_compat as compat  # noqa: E402

RUN = lib.RUN
EVID = os.path.join(RUN, "evidence")
ENDPOINT_ENV = {
    "full": "MONERO_NODE_FULL_RPC",
    "full-r": "MONERO_NODE_FULL_RESTRICTED_RPC",
    "pruned": "MONERO_NODE_PRUNED_RPC",
    "pruned-r": "MONERO_NODE_PRUNED_RESTRICTED_RPC",
}
WALLET_CLI = os.environ.get("MONERO_WALLET_CLI") or os.path.join(
    os.path.dirname(lib.WALLET_RPC), "monero-wallet-cli")
LOOKAHEAD = {"lookahead_account": 49, "lookahead_subaddress": 199}
EPEE_SIGNATURE = bytes.fromhex("0111010101010201")


def endpoint_url(name):
    var = ENDPOINT_ENV.get(name)
    if var is None:
        raise SystemExit("unknown endpoint %s (one of %s)" % (name, ", ".join(ENDPOINT_ENV)))
    url = os.environ.get(var)
    if not url:
        raise SystemExit("set %s to the RPC URL of the %s endpoint" % (var, name))
    return url.rstrip("/")


def evidence(name, data):
    os.makedirs(EVID, exist_ok=True)
    path = os.path.join(EVID, "node-" + name + ".json")
    n = 1
    while os.path.exists(path) and not name.startswith("diff-") and name != "summary":
        n += 1
        path = os.path.join(EVID, "node-%s.%d.json" % (name, n))
    with open(path, "w") as f:
        json.dump(data, f, indent=1, sort_keys=True)
    return path


def reply_status(path, resp):
    if not resp:
        return "<empty body>"
    try:
        if resp[:8] == EPEE_SIGNATURE:
            status = lib.epee_decode(resp).get("status")
            return status.decode(errors="replace") if isinstance(status, bytes) else status
        doc = json.loads(resp.decode("utf-8", "replace"), strict=False)
    except Exception as exc:  # noqa: BLE001
        return "<undecodable: %s>" % type(exc).__name__
    if isinstance(doc, dict) and "error" in doc:
        return "error %s: %s" % (doc["error"].get("code"), doc["error"].get("message"))
    if isinstance(doc, dict) and isinstance(doc.get("result"), dict):
        doc = doc["result"]
    return doc.get("status") if isinstance(doc, dict) else None


class CaptureProxy(lib.LoggingProxy):
    def record(self, handler, body, status, resp, seconds):
        path = handler.path.lstrip("/")
        entry = {"label": self.label, "phase": self.phase, "path": path, "http_status": status,
                 "status": reply_status(path, resp), "req_bytes": len(body), "resp_bytes": len(resp),
                 "ms": round(seconds * 1000, 1), "user_agent": handler.headers.get("User-Agent"),
                 "headers": sorted(k.lower() for k in handler.headers.keys())}
        if body[:8] == EPEE_SIGNATURE:
            entry["params"] = lib.summarize_bin_request(path, body)
            if path == "getblocks.bin" and resp and status == 200:
                try:
                    r = lib.epee_decode(resp)
                    blocks = r.get("blocks", []) or []
                    entry["resp"] = {"start_height": r.get("start_height"), "blocks": len(blocks),
                                     "current_height": r.get("current_height"),
                                     "pruned_flags": sorted({bool(b.get("pruned")) for b in blocks
                                                             if isinstance(b, dict)}),
                                     "pool_info_extent": r.get("pool_info_extent"),
                                     "added_pool_txs": len(r.get("added_pool_txs", []) or []),
                                     "remaining_added_pool_txids": len(
                                         r.get("remaining_added_pool_txids", b"") or b"") // 32}
                except Exception as exc:  # noqa: BLE001
                    entry["resp"] = {"undecoded": repr(exc)}
        else:
            req = lib.summarize_json_request(body)
            if path == "json_rpc" and isinstance(req, dict):
                entry["method"] = req.get("method")
                entry["params"] = req.get("params")
            else:
                entry["params"] = req
            if path in ("send_raw_transaction", "sendrawtransaction"):
                try:
                    entry["resp"] = json.loads(resp)
                except Exception:  # noqa: BLE001
                    pass
        with self.lock:
            self.seq += 1
            entry["seq"] = self.seq
            entry["t"] = round(time.time(), 3)
            with open(self.log_path, "a") as f:
                f.write(json.dumps(entry, default=repr) + "\n")


class Capture:
    def __init__(self, endpoint, run_name):
        self.endpoint = endpoint
        os.makedirs(EVID, exist_ok=True)
        self.log = os.path.join(EVID, "node-proxy-%s.jsonl" % run_name)
        n = 1
        while os.path.exists(self.log):
            n += 1
            self.log = os.path.join(EVID, "node-proxy-%s.%d.jsonl" % (run_name, n))
        self.proxy = CaptureProxy(endpoint_url(endpoint), self.log, run_name)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.proxy.stop()

    @property
    def url(self):
        return self.proxy.url

    @property
    def address(self):
        return self.proxy.url.replace("http://", "")

    def phase(self, name):
        self.proxy.phase = name

    def entries(self):
        return lib.read_log(self.log)


def sequence(entries):
    out = []
    for e in entries:
        name = e.get("method") or e["path"]
        key = (e["phase"], name, e["http_status"], e["status"])
        if out and tuple(out[-1][:4]) == key:
            out[-1][4] += 1
        else:
            out.append(list(key) + [1])
    return [{"phase": p, "call": c, "http": h, "status": s, "count": n} for p, c, h, s, n in out]


def refused(entries):
    bad = []
    for e in entries:
        if e["http_status"] != 200 or e["status"] not in ("OK", None):
            bad.append({k: e.get(k) for k in ("phase", "path", "method", "http_status", "status", "params")})
    return bad


def tool_path(args):
    return args.tool or lib.TOOL


def run_tool(args, request, name, rpc):
    saved = lib.TOOL
    lib.TOOL = tool_path(args)
    try:
        return lib.tool(request, name, rpc=rpc)
    finally:
        lib.TOOL = saved


def state_file(label, tag):
    return lib.state_file(label, tag)


def cmd_probe(args):
    with open(args.set) as f:
        probes = json.load(f)["probes"]
    name = "probe-%s-%s" % (args.tag, args.endpoint)
    with Capture(args.endpoint, name) as cap:
        r = run_tool(args, {"op": "rpc-probe", "probes": probes}, name, cap.url)
        entries = cap.entries()
    out = {"endpoint": args.endpoint, "tool": tool_path(args), "results": r["probes"],
           "requests": sequence(entries), "refused": refused(entries)}
    evidence(name, out)
    for p in r["probes"]:
        print(json.dumps(p)[:300])


def cmd_scan(args):
    name = "scan-%s-%s-%s" % (args.tag, args.endpoint, "-".join(args.labels))
    base = dict({"op": "scan", "restore_height": args.restore_height, "prune": args.prune}, **LOOKAHEAD)
    if args.stop_height is not None:
        base["stop_height"] = args.stop_height
    if args.fresh:
        for label in args.labels:
            path = state_file(label, args.tag)
            if os.path.exists(path):
                os.remove(path)
    started = time.time()
    with Capture(args.endpoint, name) as cap:
        cap.phase("scan")
        if len(args.labels) == 1:
            label = args.labels[0]
            r = run_tool(args, dict(base, seed_file=lib.seed_file(label), state=state_file(label, args.tag)),
                         name, cap.url)
            responses = {label: r}
        else:
            wallets = [{"label": label, "seed_file": lib.seed_file(label), "state": state_file(label, args.tag),
                        "response": os.path.join(RUN, "mr", "%s-%s.json" % (name, label))}
                       for label in args.labels]
            r = run_tool(args, dict(base, wallets=wallets), name, cap.url)
            responses = {}
            for w in wallets:
                with open(w["response"]) as f:
                    responses[w["label"]] = json.load(f)
        entries = cap.entries()
    summary = {}
    for label, resp in responses.items():
        summary[label] = {k: resp.get(k) for k in ("error", "start_height", "height", "blocks", "seconds",
                                                    "new_outputs", "newly_spent", "balance", "reorgs",
                                                    "conflicts", "balance_at_error", "outputs_at_error")}
        summary[label]["outputs"] = len(resp.get("outputs", []) or [])
        with open(os.path.join(RUN, "mr", "%s-%s-final.json" % (name, label)), "w") as f:
            json.dump(resp, f)
    traffic = {}
    for e in entries:
        t = traffic.setdefault(e.get("method") or e["path"], {"requests": 0, "resp_bytes": 0})
        t["requests"] += 1
        t["resp_bytes"] += e["resp_bytes"]
    out = {"endpoint": args.endpoint, "tool": tool_path(args), "prune": args.prune, "wallets": summary,
           "wall_seconds": time.time() - started, "traffic": traffic, "refused": refused(entries)[:50],
           "requests_head": sequence(entries)[:40], "requests_tail": sequence(entries)[-20:]}
    evidence(name, out)
    print(json.dumps({k: out[k] for k in ("wallets", "wall_seconds", "traffic")}, indent=1)[:4000])
    if out["refused"]:
        print("refused:", json.dumps(out["refused"][:5])[:1500])


def cmd_mempool(args):
    name = "mempool-%s-%s-%s" % (args.tag, args.endpoint, args.label)
    with Capture(args.endpoint, name) as cap:
        cap.phase("mempool")
        r = run_tool(args, dict({"op": "mempool", "seed_file": lib.seed_file(args.label)}, **LOOKAHEAD),
                     name, cap.url)
        entries = cap.entries()
    out = {"endpoint": args.endpoint, "tool": tool_path(args), "result": r, "requests": sequence(entries),
           "refused": refused(entries)}
    evidence(name, out)
    print(json.dumps(out, indent=1)[:3000])


def cmd_fee(args):
    name = "fee-%s-%s" % (args.tag, args.endpoint)
    with Capture(args.endpoint, name) as cap:
        cap.phase("fee")
        req = {"op": "fee"}
        if args.wallet_height is not None:
            req["wallet_height"] = args.wallet_height
        r = run_tool(args, req, name, cap.url)
        entries = cap.entries()
    out = {"endpoint": args.endpoint, "tool": tool_path(args), "result": r, "requests": sequence(entries),
           "refused": refused(entries)}
    evidence(name, out)
    print(json.dumps(out, indent=1)[:3000])


class Wallet2(lib.WalletRpc):
    def __init__(self, wallet_dir, ringdb_dir, daemon, trust, log_level=0):
        assert trust in ("trusted", "untrusted")
        self.trust = trust
        os.makedirs(wallet_dir, exist_ok=True)
        os.makedirs(ringdb_dir, exist_ok=True)
        assert os.path.abspath(ringdb_dir).startswith(os.path.abspath(RUN)), "ringdb must be in the run dir"
        self.port = lib.free_port()
        self.stdout = open(os.path.join(wallet_dir, "wallet-rpc.stdout"), "ab")
        self.proc = subprocess.Popen(
            [lib.WALLET_RPC, "--" + lib.NETWORK, "--daemon-address", daemon, "--" + trust + "-daemon",
             "--shared-ringdb-dir", ringdb_dir,
             "--disable-rpc-login", "--rpc-bind-ip", "127.0.0.1", "--rpc-bind-port", str(self.port),
             "--wallet-dir", wallet_dir, "--log-file", os.path.join(wallet_dir, "wallet-rpc.log"),
             "--log-level", str(log_level), "--non-interactive"],
            stdout=self.stdout, stderr=self.stdout, stdin=subprocess.DEVNULL)
        with open(os.path.join(wallet_dir, "wallet-rpc.pid"), "w") as f:
            f.write(str(self.proc.pid))
        self.url = "http://127.0.0.1:%d/json_rpc" % self.port
        for _ in range(240):
            try:
                self.call("get_version", timeout=5)
                break
            except Exception:  # noqa: BLE001
                if self.proc.poll() is not None:
                    raise RuntimeError("wallet-rpc exited early")
                time.sleep(0.25)
        else:
            self.kill()
            raise RuntimeError("wallet-rpc did not come up")


def w2_dir(label, tag):
    return os.path.join(RUN, "w2", "%s-%s" % (label, tag))


def ringdb_dir(tag):
    return os.path.join(RUN, "ringdb", tag)


def cmd_w2_refresh(args):
    tag = "%s-%s-%s" % (args.tag, args.trust, args.endpoint)
    name = "w2refresh-%s-%s" % (tag, args.label)
    wdir = w2_dir(args.label, tag)
    if os.path.exists(os.path.join(wdir, "w.keys")) and not args.reuse:
        raise SystemExit("%s exists; pass --reuse to refresh it again" % wdir)
    started = time.time()
    with Capture(args.endpoint, name) as cap:
        w = Wallet2(wdir, ringdb_dir(tag), cap.address, args.trust)
        try:
            cap.phase("open")
            if os.path.exists(os.path.join(wdir, "w.keys")):
                w.call("open_wallet", {"filename": "w", "password": ""})
            else:
                w.call("restore_deterministic_wallet",
                       {"filename": "w", "seed": compat.read_seed(args.label),
                        "restore_height": args.restore_height, "password": "", "autosave_current": False})
            w.call("auto_refresh", {"enable": False})
            cap.phase("refresh")
            t = time.time()
            refresh = w.raw("refresh", timeout=7200)
            refresh_seconds = time.time() - t
            cap.phase("export")
            view = compat.w2_view(w)
            view["refresh"] = refresh
            view["refresh_seconds"] = refresh_seconds
            cap.phase("rescan_spent")
            view["rescan_spent"] = w.raw("rescan_spent", timeout=600)
        finally:
            cap.phase("stop")
            w.stop()
        entries = cap.entries()
    view["endpoint"] = args.endpoint
    view["trust"] = args.trust
    view["wall_seconds"] = time.time() - started
    compat.evidence("w2-export-%s-%s" % (args.label, tag), view)
    traffic = {}
    for e in entries:
        t = traffic.setdefault(e.get("method") or e["path"], {"requests": 0, "resp_bytes": 0})
        t["requests"] += 1
        t["resp_bytes"] += e["resp_bytes"]
    out = {"endpoint": args.endpoint, "trust": args.trust, "refresh": view["refresh"],
           "rescan_spent": view["rescan_spent"], "height": view["height"], "balance": view["balance"],
           "outputs": sum(len(v) for v in view["incoming"].values()), "refresh_seconds": refresh_seconds,
           "traffic": traffic, "refused": refused(entries)[:50], "requests_head": sequence(entries)[:40],
           "requests_tail": sequence(entries)[-20:]}
    evidence(name, out)
    print(json.dumps({k: out[k] for k in ("refresh", "rescan_spent", "height", "outputs", "refresh_seconds",
                                          "traffic")}, indent=1)[:3000])
    print("balance", out["balance"].get("balance"), "unlocked", out["balance"].get("unlocked_balance"))
    if out["refused"]:
        print("refused:", json.dumps(out["refused"][:8])[:2000])


def cmd_w2_fee(args):
    tag = "%s-%s-%s" % (args.tag, args.trust, args.endpoint)
    name = "w2fee-%s-%s" % (tag, args.label)
    wdir = w2_dir(args.label, args.wallet_tag or tag)
    with Capture(args.endpoint, name) as cap:
        cap.phase("fee")
        p = subprocess.run(
            [WALLET_CLI, "--" + lib.NETWORK, "--daemon-address", cap.address, "--" + args.trust + "-daemon",
             "--shared-ringdb-dir", ringdb_dir(tag), "--wallet-file", os.path.join(wdir, "w"),
             "--password", "", "--log-file", os.path.join(wdir, "wallet-cli.log"), "--log-level", "0",
             "--no-dns", "fee"],
            stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=600)
        entries = cap.entries()
    out = {"endpoint": args.endpoint, "trust": args.trust, "exit": p.returncode,
           "stdout": p.stdout[-4000:], "stderr": p.stderr[-2000:], "requests": sequence(entries),
           "refused": refused(entries)}
    evidence(name, out)
    print(p.stdout[-2500:])
    if out["refused"]:
        print("refused:", json.dumps(out["refused"])[:1500])


def cmd_diff(args):
    with open(os.path.join(EVID, args.w2 + ".json")) as f:
        w2 = json.load(f)
    with open(args.mr) as f:
        mr = json.load(f)
    report = compat.scan_diff(w2, mr)
    path = evidence("diff-" + args.name, report)
    keep = {k: v for k, v in report.items() if not isinstance(v, list) or len(v) < 20}
    print(json.dumps(keep, indent=1)[:4000])
    print(path)


LEDGER = os.path.join(EVID, "relayed.json")


def ledger():
    if os.path.exists(LEDGER):
        with open(LEDGER) as f:
            return json.load(f)
    return []


def ledger_add(entry):
    entries = ledger()
    entries.append(entry)
    tmp = LEDGER + ".tmp"
    with open(tmp, "w") as f:
        json.dump(entries, f, indent=1)
        f.flush()
        os.fsync(f.fileno())
    os.rename(tmp, LEDGER)


def relays_in_ledger():
    return len({e["tx_id"] for e in ledger()})


def spent_key_images_in_ledger():
    return {ki for e in ledger() for ki in e.get("key_images", [])}


def check_blob(tx_hex, expect_inputs=1, expect_outputs=2):
    """Refuse a relay whose input is spent or in the pool; asks `endpoint` directly."""
    tx = lib.decode_tx(tx_hex)
    checks = {
        "version 2": tx.get("version") == 2,
        "unlock_time 0": tx.get("unlock_time") == 0,
        "rct type 6": tx.get("rct_type") == 6,
        "inputs": len(tx.get("inputs", [])) == expect_inputs,
        "outputs": len(tx.get("outputs", [])) == expect_outputs,
        "ring size 16": all(len(i.get("indexes", [])) == 16 for i in tx.get("inputs", [])),
        "fee sane": 0 < tx.get("fee", 0) < 10 ** 9,
        "no trailing bytes": tx.get("trailing_bytes") == 0,
    }
    rings = [{"key_image": i["key_image"], "indexes": i["indexes"]} for i in tx.get("inputs", [])]
    return tx, checks, rings


def http_json(url, path, body, timeout=60):
    req = urllib.request.Request(url + "/" + path, json.dumps(body).encode())
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def guard(key_images, endpoint):
    url = endpoint_url(endpoint)
    status = http_json(url, "is_key_image_spent", {"key_images": key_images})["spent_status"]
    pool = {k["id_hash"] for k in http_json(url, "get_transaction_pool", {}).get("spent_key_images", [])}
    found = {k: {"spent_status": s, "in_pool": k in pool} for k, s in zip(key_images, status)}
    bad = [k for k, v in found.items() if v["spent_status"] != 0 or v["in_pool"]]
    if bad:
        raise SystemExit("input already spent or in the pool: %s" % found)
    return found


def cmd_build(args):
    name = "build-%s-%s-%s-%s" % (args.tag, args.endpoint, args.label, args.name)
    tx_out = os.path.join(RUN, "mr", name + "-tx.json")
    with Capture(args.endpoint, name) as cap:
        cap.phase("build")
        r = run_tool(args, {"op": "build", "seed_file": lib.seed_file(args.label),
                            "state": state_file(args.label, args.tag), "kind": "send",
                            "recipients": [[args.address, args.amount]], "inputs": [args.input],
                            "record": True, "tx_out": tx_out, "priority": args.priority}, name, cap.url)
        entries = cap.entries()
    out = {"endpoint": args.endpoint, "tool": tool_path(args), "build": r, "requests": sequence(entries),
           "refused": refused(entries), "get_outs": [e["params"] for e in entries if e["path"] == "get_outs.bin"]}
    if r.get("builds"):
        b = r["builds"][0]
        tx, checks, rings = check_blob(b["tx_blob"])
        with open(state_file(args.label, args.tag)) as f:
            state = json.load(f)
        own = {o["key_image"]: o for o in state["outputs"]}
        for ring in rings:
            stored = own.get(ring["key_image"])
            checks["input is ours"] = stored is not None
            checks["input unspent"] = bool(stored) and not stored.get("spent")
            checks["input not pending"] = ring["key_image"] not in state.get("pending_spends", {})
            checks["ring saved before relay equals blob"] = bool(stored) and bool(stored.get("ring")) and \
                _ring_abs(stored["ring"]) == ring["indexes"]
            checks["input never relayed"] = ring["key_image"] not in spent_key_images_in_ledger()
        checks["candidates include the real output"] = all(
            any(set(ring["indexes"]) <= set(g.get("outputs_indexes", [])) for g in out["get_outs"])
            for ring in rings) if out["get_outs"] else None
        out.update(tx_id=b["tx_id"], fee=b["fee"], checks=checks, rings=rings,
                   observer=lib.observer_view(tx) if hasattr(lib, "observer_view") else None)
    evidence(name, out)
    print(json.dumps({k: out.get(k) for k in ("tx_id", "fee", "checks", "refused")}, indent=1))
    if not r.get("builds"):
        print(json.dumps(r)[:2000])


def _ring_abs(ring):
    absolute, acc = [], 0
    for off in ring["offsets"]:
        acc += off
        absolute.append(acc)
    return absolute


def cmd_relay(args):
    name = "build-%s-%s-%s-%s" % (args.tag, args.build_endpoint or args.endpoint, args.label, args.name)
    with open(os.path.join(EVID, "node-" + name + ".json")) as f:
        built = json.load(f)
    failed = [k for k, v in built["checks"].items() if v is False]
    if failed:
        raise SystemExit("checks failed: %s" % failed)
    if relays_in_ledger() >= args.max_relays:
        raise SystemExit("relay budget of %d used" % args.max_relays)
    key_images = [r["key_image"] for r in built["rings"]]
    if set(key_images) & spent_key_images_in_ledger():
        raise SystemExit("an input was relayed before")
    guarded = guard(key_images, args.guard_endpoint)
    entry = {"name": args.name, "impl": args.tag, "endpoint": args.endpoint, "tx_id": built["tx_id"],
             "guard": guarded,
             "key_images": key_images, "rings": built["rings"], "fee": built["fee"], "t": time.time(),
             "state": "about to relay"}
    ledger_add(entry)
    rname = "relay-%s-%s-%s-%s" % (args.tag, args.endpoint, args.label, args.name)
    with Capture(args.endpoint, rname) as cap:
        cap.phase("relay")
        r = run_tool(args, {"op": "relay", "state": state_file(args.label, args.tag),
                            "tx": os.path.join(RUN, "mr", name + "-tx.json")}, rname, cap.url)
        entries = cap.entries()
    out = {"endpoint": args.endpoint, "relay": r, "requests": sequence(entries), "refused": refused(entries),
           "wire": [e for e in entries if e["path"] in ("send_raw_transaction", "sendrawtransaction")]}
    evidence(rname, out)
    ledger_add(dict(entry, state="relay answered", answer=r.get("relay"), t=time.time()))
    print(json.dumps(out, indent=1)[:3000])


def cmd_w2_relay(args):
    tag = "%s-%s-%s" % (args.tag, args.trust, args.wallet_endpoint or args.endpoint)
    wdir = w2_dir(args.label, args.wallet_tag or tag)
    name = "w2relay-%s-%s-%s-%s" % (args.tag, args.trust, args.endpoint, args.name)
    if relays_in_ledger() >= args.max_relays:
        raise SystemExit("relay budget of %d used" % args.max_relays)
    with Capture(args.endpoint, name) as cap:
        w = Wallet2(wdir, ringdb_dir(args.wallet_tag or tag), cap.address, args.trust)
        out = {"endpoint": args.endpoint, "trust": args.trust}
        try:
            w.call("open_wallet", {"filename": "w", "password": ""})
            w.call("auto_refresh", {"enable": False})
            cap.phase("refresh")
            out["refresh"] = w.raw("refresh", timeout=3600)
            unspent = w.call("incoming_transfers", {"transfer_type": "available", "account_index": 0,
                                                    "subaddr_indices": [args.subaddr], "verbose": True})
            outs = unspent.get("transfers", [])
            out["source_outputs"] = outs
            if len(outs) != 1 or not outs[0].get("unlocked"):
                raise SystemExit("subaddress (0,%d) must hold exactly one unlocked output, has %s"
                                 % (args.subaddr, [(o["amount"], o.get("unlocked")) for o in outs]))
            if outs[0]["key_image"] in spent_key_images_in_ledger():
                raise SystemExit("that output was relayed before")
            cap.phase("transfer")
            t = w.call("transfer", {"destinations": [{"address": args.address, "amount": args.amount}],
                                    "account_index": 0, "subaddr_indices": [args.subaddr],
                                    "priority": args.priority, "do_not_relay": True, "get_tx_hex": True,
                                    "get_tx_metadata": True})
            tx, checks, rings = check_blob(t["tx_blob"])
            checks["input is the chosen output"] = [r["key_image"] for r in rings] == [outs[0]["key_image"]]
            checks["input never relayed"] = outs[0]["key_image"] not in spent_key_images_in_ledger()
            checks["ring includes the real global index"] = outs[0]["global_index"] in rings[0]["indexes"]
            entries = cap.entries()
            gets = [e["params"] for e in entries if e["path"] == "get_outs.bin"]
            checks["candidates include the real output"] = all(
                any(set(r["indexes"]) <= set(g.get("outputs_indexes", [])) for g in gets) for r in rings) \
                if gets else None
            out.update(tx_id=t["tx_hash"], fee=t["fee"], checks=checks, rings=rings, get_outs=gets)
            failed = [k for k, v in checks.items() if v is False]
            if failed:
                raise SystemExit("checks failed: %s" % failed)
            guarded = guard([r["key_image"] for r in rings], args.guard_endpoint)
            entry = {"name": args.name, "impl": "w2-" + args.trust, "endpoint": args.endpoint, "guard": guarded,
                     "tx_id": t["tx_hash"], "key_images": [r["key_image"] for r in rings], "rings": rings,
                     "fee": t["fee"], "t": time.time(), "state": "about to relay"}
            ledger_add(entry)
            cap.phase("relay")
            relayed = w.raw("relay_tx", {"hex": t["tx_metadata"]}, timeout=600)
            out["relay"] = relayed
            ledger_add(dict(entry, state="relay answered", answer=relayed, t=time.time()))
        finally:
            cap.phase("stop")
            w.stop()
        entries = cap.entries()
    out.update(requests=sequence(entries), refused=refused(entries),
               wire=[e for e in entries if e["path"] in ("send_raw_transaction", "sendrawtransaction")])
    evidence(name, out)
    print(json.dumps({k: out.get(k) for k in ("tx_id", "fee", "checks", "relay", "refused")}, indent=1)[:3000])


def cmd_pool_watch(args):
    names = [n for n in ENDPOINT_ENV if os.environ.get(ENDPOINT_ENV[n])]
    rows = []
    end = time.time() + args.seconds
    while time.time() < end:
        row = {"t": round(time.time(), 2)}
        for name in names:
            try:
                hashes = http_json(endpoint_url(name), "get_transaction_pool_hashes", {}, timeout=5).get("tx_hashes")
                row[name] = args.txid in (hashes or [])
            except Exception as exc:  # noqa: BLE001
                row[name] = repr(exc)[:60]
        rows.append(row)
        if all(row[n] is True for n in names):
            break
        time.sleep(args.interval)
    first = {n: next((r["t"] for r in rows if r.get(n) is True), None) for n in names}
    out = {"txid": args.txid, "start": rows[0]["t"], "first_visible": first, "polls": rows}
    evidence("pool-watch-" + args.txid[:16], out)
    print(json.dumps({"start": out["start"], "first_visible": first}))


def getblocks_raw(url, start, prune):
    def varint(n):
        if n < 64:
            return bytes([n << 2])
        if n < 16384:
            return struct.pack("<H", (n << 2) | 1)
        if n < 1 << 30:
            return struct.pack("<I", (n << 2) | 2)
        return struct.pack("<Q", (n << 2) | 3)

    def entry(name, typ, value):
        out = bytes([len(name)]) + name.encode() + bytes([typ])
        if typ == 5:
            return out + struct.pack("<Q", value)
        if typ == 11:
            return out + bytes([1 if value else 0])
        return out + varint(len(value)) + value

    fields = [entry("block_ids", 10, b""), entry("start_height", 5, start), entry("prune", 11, prune),
              entry("no_miner_tx", 11, False)]
    body = EPEE_SIGNATURE + b"\x01" + varint(len(fields)) + b"".join(fields)
    host, port = url.replace("http://", "").split(":")
    conn = http.client.HTTPConnection(host, int(port), timeout=600)
    conn.request("POST", "/getblocks.bin", body=body)
    r = conn.getresponse()
    return r.status, r.read()


def cmd_getblocks_compare(args):
    out = {"start": args.start, "prune": args.prune, "reference": args.reference, "endpoint": args.endpoint}
    replies = {}
    for name in (args.reference, args.endpoint):
        status, body = getblocks_raw(endpoint_url(name), args.start, args.prune)
        replies[name] = lib.epee_decode(body) if status == 200 and body[:8] == EPEE_SIGNATURE else None
        out[name] = {"http_status": status, "bytes": len(body),
                     "blocks": len(replies[name].get("blocks", []) or []) if replies[name] else None}
    if replies[args.reference] and replies[args.endpoint]:
        diffs, compared = [], 0
        pairs = zip(replies[args.reference]["blocks"], replies[args.endpoint]["blocks"])
        for i, (x, y) in enumerate(pairs):
            xt, yt = x.get("txs", []) or [], y.get("txs", []) or []
            if x.get("block") != y.get("block") or len(xt) != len(yt):
                diffs.append({"height": args.start + i, "what": "block"})
                continue
            for j, (u, v) in enumerate(zip(xt, yt)):
                compared += 1
                ub = u.get("blob") if isinstance(u, dict) else u
                vb = v.get("blob") if isinstance(v, dict) else v
                if ub != vb:
                    diffs.append({"height": args.start + i, "tx": j, "reference_bytes": len(ub), "bytes": len(vb)})
        out.update(transactions_compared=compared, differing=len(diffs), first_differences=diffs[:10])
    evidence("getblocks-compare-%s-%d-%s" % (args.endpoint, args.start, "pruned" if args.prune else "full"), out)
    print(json.dumps(out)[:1500])


def cmd_summary(args):
    rows = {}
    for fname in sorted(os.listdir(EVID)):
        if fname.startswith("node-proxy-") and fname.endswith(".jsonl"):
            entries = lib.read_log(os.path.join(EVID, fname))
            rows[fname[len("node-proxy-"):-len(".jsonl")]] = {
                "requests": len(entries),
                "calls": sorted({(e.get("method") or e["path"]) for e in entries}),
                "refused": refused(entries)[:10],
            }
    evidence("summary", rows)
    for k, v in rows.items():
        print(k, v["requests"], v["calls"], "refused:", len(v["refused"]))


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)

    def common(s, endpoint=True):
        if endpoint:
            s.add_argument("endpoint", choices=sorted(ENDPOINT_ENV))
        s.add_argument("--tool", help="stagenet_tool build to run (default MONERO_STAGENET_TOOL)")
        s.add_argument("--tag", default="mr", help="names this code state's state files and evidence")

    s = sub.add_parser("probe")
    common(s)
    s.add_argument("--set", required=True, help="JSON file with a `probes` list for rpc-probe")
    s = sub.add_parser("scan")
    common(s)
    s.add_argument("labels", nargs="+")
    s.add_argument("--restore-height", type=int, default=0)
    s.add_argument("--stop-height", type=int)
    s.add_argument("--prune", action="store_true")
    s.add_argument("--fresh", action="store_true", help="start from the restore height, dropping the state file")
    s = sub.add_parser("mempool")
    common(s)
    s.add_argument("label")
    s = sub.add_parser("fee")
    common(s)
    s.add_argument("--wallet-height", type=int)
    for cmd in ("w2-refresh", "w2-fee"):
        s = sub.add_parser(cmd)
        common(s)
        s.add_argument("label")
        s.add_argument("--trust", choices=("trusted", "untrusted"), default="untrusted")
        s.add_argument("--restore-height", type=int, default=0)
        s.add_argument("--reuse", action="store_true")
        s.add_argument("--wallet-tag", help="use the wallet2 wallet of another run")
    s = sub.add_parser("diff")
    s.add_argument("w2", help="evidence name of a wallet2 export, without .json")
    s.add_argument("mr", help="crate scan response file")
    s.add_argument("name")
    s = sub.add_parser("build")
    common(s)
    s.add_argument("label")
    s.add_argument("name")
    s.add_argument("--input", required=True, help="output key (txhash:index) of the one input")
    s.add_argument("--address", required=True)
    s.add_argument("--amount", type=int, required=True)
    s.add_argument("--priority", default="automatic")
    s = sub.add_parser("relay")
    common(s)
    s.add_argument("label")
    s.add_argument("name")
    s.add_argument("--build-endpoint")
    s.add_argument("--max-relays", type=int, default=4)
    s.add_argument("--guard-endpoint", default="full", help="unrestricted endpoint asked whether the input is spent")
    s = sub.add_parser("w2-relay")
    common(s)
    s.add_argument("label")
    s.add_argument("name")
    s.add_argument("--trust", choices=("trusted", "untrusted"), default="untrusted")
    s.add_argument("--subaddr", type=int, required=True)
    s.add_argument("--address", required=True)
    s.add_argument("--amount", type=int, required=True)
    s.add_argument("--priority", type=int, default=0)
    s.add_argument("--wallet-tag")
    s.add_argument("--wallet-endpoint")
    s.add_argument("--max-relays", type=int, default=4)
    s.add_argument("--guard-endpoint", default="full", help="unrestricted endpoint asked whether the input is spent")
    s = sub.add_parser("pool-watch")
    s.add_argument("txid")
    s.add_argument("--seconds", type=float, default=600)
    s.add_argument("--interval", type=float, default=0.5)
    s = sub.add_parser("getblocks-compare")
    s.add_argument("endpoint", choices=sorted(ENDPOINT_ENV))
    s.add_argument("start", type=int)
    s.add_argument("--reference", default="full", choices=sorted(ENDPOINT_ENV))
    s.add_argument("--prune", action="store_true")
    sub.add_parser("summary")
    args = p.parse_args()
    lib.ensure_dirs()
    globals()["cmd_" + args.cmd.replace("-", "_")](args)


if __name__ == "__main__":
    main()
