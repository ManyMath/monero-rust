"""Fee selection under real congestion on the private fork of stagenet_fork.py.

    stagenet_fork_fees.py fanout TAG COUNT PER_OUTPUT
    stagenet_fork_fees.py measure TAG STAGE [--w2 ...] [--lineage LINEAGE] [--no-refresh]
"""
import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagenet_fork as fk  # noqa: E402
import stagenet_fork_cycle as cy  # noqa: E402
import stagenet_lib as lib  # noqa: E402

RUN = lib.RUN
PRIORITIES = ["automatic", "low", "normal", "elevated", "priority"]
PAYMENT = 10_000_000_000  # 0.01 XMR
SOURCE_ACCOUNT = 1


def fan_addresses():
    path = os.path.join(RUN, "mr", fk.WALLET + "-fan-addr.json")
    if not os.path.exists(path):
        subs = [[0, i] for i in range(20, 36)]
        fk.tool(fk.addr_tool(), {"op": "addr", "seed_file": lib.seed_file(fk.WALLET), "subaddresses": subs},
                fk.WALLET + "-fan-addr")
    with open(path) as f:
        r = json.load(f)
    return [s[2] for s in r["subaddresses"]]


def fanout(tag, count, per_output=None, outputs=15, priority=1):
    assert fk.node_summary(fk.fork_rpc())["offline"] is True
    addrs = fan_addresses()
    w = fk.w2_open(fk.WALLET)
    made, errors = [], []
    try:
        w.call("refresh")
        unlocked = w.call("get_balance", {"account_index": 0, "all_accounts": True})["unlocked_balance"]
        amount = per_output or max(1_000_000_000, unlocked // (count * (outputs + 2) * 4))
        started = time.time()
        for n in range(count):
            dests = [{"address": addrs[(n * outputs + k) % len(addrs)], "amount": amount} for k in range(outputs)]
            r = w.raw("transfer", {"destinations": dests, "priority": priority, "account_index": 0,
                                   "subaddr_indices": [], "get_tx_hex": False}, timeout=600)
            if "result" in r:
                made.append({"tx": r["result"]["tx_hash"], "fee": r["result"]["fee"],
                             "weight": r["result"].get("weight")})
            else:
                errors.append(r["error"])
                if len(errors) > 5:
                    break
        fk.log("fanout", tag, "made", len(made), "errors", len(errors), "amount", amount,
               "%.0fs" % (time.time() - started), errors[:2])
    finally:
        w.stop()
    fk.save("%s/fanout-%d" % (tag, int(time.time())), {"made": made, "errors": errors})
    return made


def pool_weight():
    pool = fk.other(fk.fork_rpc(), "get_transaction_pool")
    txs = pool.get("transactions", []) or []
    return len(txs), sum(t["weight"] for t in txs), sorted({t["fee"] * 1.0 / t["weight"] for t in txs})[:1]


def window(height=None):
    h = height or fk.info(fk.fork_rpc())["height"]
    rows = fk.block_weights(fk.fork_rpc(), h - 10, h)
    return {"height": h, "weights": [r[1] for r in rows], "sum": sum(r[1] for r in rows),
            "txs": [r[2] for r in rows]}


def measure(tag, stage, w2_label=None, lineage="main", refresh=True):
    w2_label = w2_label or fk.WALLET
    out = {"stage": stage, "node": fk.node_summary(fk.fork_rpc()), "pool": pool_weight(), "window": window(),
           "backlog_raw": None}
    try:
        out["fee_estimate"] = fk.jrpc(fk.fork_rpc(), "get_fee_estimate", {"grace_blocks": 10})
    except Exception as exc:  # noqa: BLE001
        out["fee_estimate"] = repr(exc)
    dest = cy.addresses()[(2, 1)]
    evdir = os.path.join(RUN, "evidence", tag)
    os.makedirs(evdir, exist_ok=True)

    proxy = lib.LoggingProxy(fk.fork_rpc(), os.path.join(evdir, "proxy-%s-w2-%s.jsonl" % (stage, w2_label)),
                             "w2-" + stage)
    if not refresh:
        proxy.refuse = {"getblocks.bin", "gethashes.bin", "get_blocks.bin", "get_hashes.bin"}
    try:
        w = fk.w2_open(w2_label, daemon=proxy.url)
        try:
            if refresh:
                proxy.phase = "refresh"
                w.call("refresh")
            rows = {"height": w.call("get_height")["height"]}
            proxy.phase = "default_priority"
            r = w.raw("get_default_fee_priority")
            rows["default_priority"] = r.get("result", {}).get("priority", r.get("error"))
            for p in range(5):
                proxy.phase = "transfer-%d" % p
                r = w.raw("transfer", {"destinations": [{"address": dest, "amount": PAYMENT}], "priority": p,
                                       "account_index": SOURCE_ACCOUNT, "do_not_relay": True,
                                       "get_tx_hex": True}, timeout=600)
                if "result" in r:
                    res = r["result"]
                    tx = lib.decode_tx(res["tx_blob"])
                    rows["p%d" % p] = {"fee": res["fee"], "weight": res["weight"], "size": len(res["tx_blob"]) // 2,
                                       "inputs": len(tx["inputs"]), "outputs": len(tx["outputs"]),
                                       "rate": res["fee"] / res["weight"]}
                else:
                    rows["p%d" % p] = {"error": r["error"]}
            rows["height_after"] = w.call("get_height")["height"]
        finally:
            w.stop(store=refresh)
        out["w2"] = rows
    finally:
        proxy.stop()

    for impl in cy.CRATES:
        proxy = lib.LoggingProxy(fk.fork_rpc(), os.path.join(evdir, "proxy-%s-%s-%s.jsonl" % (stage, impl, lineage)),
                                 "%s-%s" % (impl, stage))
        try:
            st = fk.state_path(impl, lineage)
            rows = {}
            if refresh:
                proxy.phase = "scan"
                kw = {"probe_tip": True} if impl == "A" else {}
                s = fk.tool(impl, dict({"op": "scan", "seed_file": lib.seed_file(fk.WALLET), "state": st,
                                        "ring_db": fk.ring_db_path(impl, lineage),
                                        "restore_height": fk.RESTORE_HEIGHT, "rpc": proxy.url}, **fk.LOOKAHEAD, **kw),
                            "%s/%s-scan-%s-%s" % (tag, stage, impl, lineage))
                rows["scan_error"] = s.get("error")
            with open(st) as f:
                height = json.load(f)["next_height"]
            rows["height"] = height
            proxy.phase = "fee"
            rows["fee"] = fk.tool(impl, {"op": "fee", "wallet_height": height, "rpc": proxy.url},
                                  "%s/%s-fee-%s-%s" % (tag, stage, impl, lineage)).get("priorities")
            for name in PRIORITIES:
                proxy.phase = "build-" + name
                b = fk.tool(impl, {"op": "build", "kind": "send", "seed_file": lib.seed_file(fk.WALLET), "state": st,
                                   "recipients": [[dest, PAYMENT]], "priority": name, "rpc": proxy.url,
                                   "source_account": SOURCE_ACCOUNT},
                            "%s/%s-build-%s-%s-%s" % (tag, stage, impl, lineage, name))
                if b.get("builds"):
                    x = b["builds"][0]
                    tx = lib.decode_tx(x["tx_blob"])
                    rows[name] = {"fee": x["fee"], "size": x["blob_bytes"], "inputs": len(tx["inputs"]),
                                  "outputs": len(tx["outputs"]), "rate_per_byte": x["fee"] / x["blob_bytes"]}
                else:
                    rows[name] = {"error": b.get("prepare_error") or b.get("build_errors") or b.get("error")}
        finally:
            proxy.stop()
        out[impl] = rows
    fk.save("%s/measure-%s" % (tag, stage), out)
    brief = {"w2_default": out["w2"].get("default_priority"),
             "w2_rates": [round(out["w2"].get("p%d" % p, {}).get("rate", 0)) for p in range(5)]}
    for impl in cy.CRATES:
        brief[impl + "_auto"] = out[impl]["automatic"].get("fee") if "fee" in out[impl]["automatic"] else out[impl]["automatic"]
        brief[impl + "_rates"] = [(row["checked"].get("Ok", row["checked"]) if isinstance(row["checked"], dict) else row["checked"])
                                  for row in out[impl]["fee"] or []]
    fk.log("measure", tag, stage, "pool", out["pool"][:2], "window", out["window"]["sum"], json.dumps(brief)[:900])
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("fanout")
    p.add_argument("tag")
    p.add_argument("count", type=int)
    p.add_argument("per_output", type=int)
    p = sub.add_parser("measure")
    p.add_argument("tag")
    p.add_argument("stage")
    p.add_argument("--w2", default=None)
    p.add_argument("--lineage", default="main")
    p.add_argument("--no-refresh", action="store_true")
    args = ap.parse_args()
    if args.cmd == "fanout":
        fanout(args.tag, args.count, args.per_output)
    else:
        measure(args.tag, args.stage, args.w2, args.lineage, not args.no_refresh)


if __name__ == "__main__":
    main()
