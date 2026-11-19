"""One reorg cycle on the private fork of stagenet_fork.py: per implementation, a
"main" copy that sees the private blocks must end equal to a "ref" copy that never does.

    stagenet_fork_cycle.py scan TAG PHASE [--main-only] | private TAG PLAN | compare TAG PHASE | rings TAG

PLAN: {"spends": [{"impl": "w2"|"A"|"B", "input": "<tx>:<index>", "to": [major, minor],
"public": false}], "miner": "<address>"}.
"""
import argparse
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagenet_fork as fk  # noqa: E402
import stagenet_lib as lib  # noqa: E402

RUN = lib.RUN
CRATES = ("A", "B")


def addresses():
    path = os.path.join(RUN, "mr", fk.WALLET + "-addr.json")
    if not os.path.exists(path):
        subs = [[a, i] for a in range(3) for i in range(12)]
        fk.tool(fk.addr_tool(), {"op": "addr", "seed_file": lib.seed_file(fk.WALLET), "subaddresses": subs}, fk.WALLET + "-addr")
    with open(path) as f:
        r = json.load(f)
    table = {(0, 0): r["primary"]}
    for a, i, address in r["subaddresses"]:
        table[(a, i)] = address
    return table


def scan_lineage(impl, lineage, name, **extra):
    if impl == "w2":
        label = fk.WALLET if lineage == "main" else fk.WALLET + "-" + lineage
        return fk.w2_refresh(label, name)
    return fk.crate_scan(impl, lineage, name, **extra)


def scan_all(tag, phase, lineages=("main", "ref"), extra=None):
    out = {}
    for lineage in lineages:
        for impl in ("w2",) + CRATES:
            kw = dict((extra or {}).get(impl, {}))
            if impl == "A":
                kw.setdefault("probe_tip", True)
            out["%s-%s" % (impl, lineage)] = scan_lineage(impl, lineage, "%s/%s-%s-%s" % (tag, phase, impl, lineage),
                                                          **kw)
    return out


def output_by_key(scan, key):
    tx, index = key.split(":")
    for o in scan["outputs"]:
        if o["tx_hash"] == tx and o["output_index"] == int(index):
            return o
    raise KeyError(key)


def private_spends(tag, plan):
    assert fk.node_summary(fk.fork_rpc())["offline"] is True
    subs = addresses()
    ref_scan = fk.load("%s/pre-B-main" % tag)
    made = []
    for spend in plan["spends"]:
        out = output_by_key(ref_scan, spend["input"])
        to = subs[tuple(spend["to"])]
        entry = dict(spend, key_image=out["key_image"], amount=out["amount"], address=to)
        if spend["impl"] == "w2":
            w = fk.w2_open(fk.WALLET)
            try:
                w.call("refresh")
                r = w.call("sweep_single", {"key_image": out["key_image"], "address": to, "get_tx_hex": True,
                                            "get_tx_key": True})
            finally:
                w.stop()
            entry.update(tx=r["tx_hash"], fee=r["fee"], tx_hex=r["tx_blob"])
        else:
            impl = spend["impl"]
            st = fk.state_path(impl, "main")
            tx_out = os.path.join(RUN, "mr", "%s_%s-%s-tx.json" % (tag, impl, out["key_image"][:8]))
            ring_db = fk.ring_db_path(impl, "main")
            b = fk.tool(impl, {"op": "build", "kind": "sweep", "seed_file": lib.seed_file(fk.WALLET), "state": st,
                               "ring_db": ring_db, "inputs": [spend["input"]], "recipients": [[to, 0]],
                               "record": True, "tx_out": tx_out, "priority": "low"}, "%s/build-%s" % (tag, impl))
            assert b.get("recorded"), b
            r = fk.tool(impl, {"op": "relay", "state": st, "ring_db": ring_db, "tx": tx_out},
                        "%s/relay-%s" % (tag, impl))
            assert r.get("relay") == "OK", r
            build = b["builds"][0]
            entry.update(tx=build["tx_id"], fee=build["fee"], tx_hex=build["tx_blob"])
        entry["rings"] = fk.tx_rings(entry["tx_hex"])
        fk.log("private spend", spend["impl"], entry["tx"], "input", spend["input"][:16], "to", spend["to"])
        made.append(entry)
    pool = fk.pool_ids(fk.fork_rpc())
    assert sorted(pool) == sorted(e["tx"] for e in made), pool
    fk.save("%s/private-spends" % tag, {"spends": made, "pool": pool})
    return made


def compare(tag, phase):
    get = lambda impl, lineage: fk.load("%s/%s-%s-%s" % (tag, phase, impl, lineage))  # noqa: E731
    report = {"w2": fk.compare_w2(get("w2", "main"), get("w2", "ref"))}
    for impl in CRATES:
        report[impl] = fk.compare_crate(get(impl, "main"), get(impl, "ref"))
        report["%s-vs-w2-main" % impl] = fk.compare_crate_w2(get(impl, "main"), get("w2", "main"))
        report["%s-ref-vs-w2-ref" % impl] = fk.compare_crate_w2(get(impl, "ref"), get("w2", "ref"))
    fk.save("%s/compare-%s" % (tag, phase), report)
    summary = {}
    for k, v in report.items():
        if "outputs" in v:
            summary[k] = {"outputs_equal": v["outputs"]["equal"], "balance_equal": v["balance_equal"],
                          "height": v["height"]}
            if "transfers_only_main" in v:
                summary[k]["transfers_only_main"] = len(v["transfers_only_main"])
                summary[k]["transfers_only_ref"] = len(v["transfers_only_ref"])
        else:
            summary[k] = {"equal": v["equal"], "compared": v["compared"]}
    fk.log("compare", tag, phase, json.dumps(summary))
    return report


def check_private_effects(tag, phase):
    spends = fk.load("%s/private-spends" % tag)["spends"]
    out = {}
    for lineage in ("main", "ref"):
        for impl in ("w2",) + CRATES:
            scan = fk.load("%s/%s-%s-%s" % (tag, phase, impl, lineage))
            outs = fk.norm_w2(scan) if impl == "w2" else fk.norm_crate(scan)
            rows = []
            for s in spends:
                received = [dict(v, key=list(k)) for k, v in outs.items() if k[0] == s["tx"]]
                inputs = [dict(v, key=list(k)) for k, v in outs.items() if v["key_image"] == s["key_image"]]
                rows.append({"tx": s["tx"], "impl": s["impl"], "public": s.get("public", False),
                             "received": received, "input": inputs})
            out["%s-%s" % (impl, lineage)] = rows
    fk.save("%s/effects-%s" % (tag, phase), out)
    return out


def rings_after(tag, phase="after"):
    spends = fk.load("%s/private-spends" % tag)["spends"]
    subs = addresses()
    result = []
    for s in spends:
        if s.get("public"):
            continue
        row = {"tx": s["tx"], "built_by": s["impl"], "key_image": s["key_image"],
               "private_ring": s["rings"][s["key_image"]]}
        w = fk.w2_open(fk.WALLET)
        try:
            row["w2_refresh"] = w.raw("refresh").get("error")
            got = w.raw("get_ring", {"key_image": s["key_image"]})
            row["w2_get_ring"] = got.get("result", {}).get("offsets") if "result" in got else got.get("error")
            r = w.raw("sweep_single", {"key_image": s["key_image"], "address": subs[(0, 8)], "do_not_relay": True,
                                       "get_tx_hex": True})
            if "result" in r:
                row["w2_rebuild"] = fk.tx_rings(r["result"]["tx_blob"]).get(s["key_image"])
            else:
                row["w2_rebuild_error"] = r["error"]
        finally:
            w.stop()
        for impl in CRATES:
            b = fk.tool(impl, {"op": "build", "kind": "sweep", "seed_file": lib.seed_file(fk.WALLET),
                               "state": fk.state_path(impl, "main"), "ring_db": fk.ring_db_path(impl, "main"),
                               "inputs": [s["input"]],
                               "recipients": [[subs[(0, 8)], 0]], "priority": "low", "ignore_pending": True},
                        "%s/rings-%s-%s" % (tag, impl, s["key_image"][:8]))
            if b.get("builds"):
                rr = {x["key_image"]: x["indexes"] for x in b["builds"][0]["rings"]}
                row["%s_rebuild" % impl] = rr.get(s["key_image"])
            else:
                row["%s_rebuild_error" % impl] = b.get("prepare_error") or b.get("build_errors") or b.get("error")
        for k in ("w2_rebuild", "A_rebuild", "B_rebuild"):
            if k in row:
                row[k.replace("rebuild", "same_ring")] = row[k] == row["private_ring"]
        w2r = row.get("w2_get_ring")
        if isinstance(w2r, list):
            row["w2_ringdb_same"] = w2r == row["private_ring"]
        fk.log("ring", s["impl"], s["tx"][:12], {k: v for k, v in row.items() if k.endswith("same_ring")
                                                  or k.endswith("_error") or k == "w2_ringdb_same"})
        result.append(row)
    fk.save("%s/rings-%s" % (tag, phase), result)
    return result


def copy_lineage(src, dst):
    for impl in CRATES:
        shutil.copyfile(fk.state_path(impl, src), fk.state_path(impl, dst))
        if os.path.exists(fk.ring_db_path(impl, src)):
            shutil.copyfile(fk.ring_db_path(impl, src), fk.ring_db_path(impl, dst))
    s = os.path.join(RUN, "w2", fk.WALLET if src == "main" else fk.WALLET + "-" + src)
    d = os.path.join(RUN, "w2", fk.WALLET if dst == "main" else fk.WALLET + "-" + dst)
    os.makedirs(d, exist_ok=True)
    for name in ("w", "w.keys"):
        shutil.copyfile(os.path.join(s, name), os.path.join(d, name))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("scan")
    p.add_argument("tag")
    p.add_argument("phase")
    p.add_argument("--main-only", action="store_true")
    p = sub.add_parser("private")
    p.add_argument("tag")
    p.add_argument("plan")
    p = sub.add_parser("compare")
    p.add_argument("tag")
    p.add_argument("phase")
    p = sub.add_parser("rings")
    p.add_argument("tag")
    args = ap.parse_args()
    if args.cmd == "scan":
        scan_all(args.tag, args.phase, ("main",) if args.main_only else ("main", "ref"))
    elif args.cmd == "private":
        with open(args.plan) as f:
            private_spends(args.tag, json.load(f))
    elif args.cmd == "compare":
        compare(args.tag, args.phase)
        check_private_effects(args.tag, args.phase)
    elif args.cmd == "rings":
        rings_after(args.tag)


if __name__ == "__main__":
    main()


def run_cycle(tag, blocks, plan, public=None, private_scan_extra=None, after_extra=None, stop_after_scan=False):
    os.makedirs(os.path.join(RUN, "evidence", tag), exist_ok=True)
    fk.save("%s/plan" % tag, plan)
    scan_all(tag, "pre")
    pre = fk.load("%s/pre-B-main" % tag)
    images = [output_by_key(pre, s["input"])["key_image"] for s in plan["spends"]]
    status = fk.other(fk.source_rpc(), "is_key_image_spent", {"key_images": images})["spent_status"]
    fk.log("inputs on the source node", list(zip([i[:8] for i in images], status)))
    assert all(x == 0 for x in status), "a planned input is spent or pooled on the real chain"
    start = fk.go_private(tag)
    fork_height = start["fork_height"]
    made = private_spends(tag, plan)
    fk.mine(blocks, plan["miner"], tag)
    scan_all(tag, "private", ("main",), private_scan_extra)
    if stop_after_scan:
        return made
    finish_cycle(tag, fork_height, made, public, after_extra)
    return made


def finish_cycle(tag, fork_height, made, public=None, after_extra=None):
    import time
    while fk.info(fk.source_rpc())["height"] <= fork_height:
        time.sleep(15)
    fk.feed_real_blocks(fork_height, tag)
    fk.flush_pool([public] if public else [])
    fk.go_public(tag, [public] if public else [])
    if public:
        while True:
            got = fk.other(fk.source_rpc(), "get_transactions", {"txs_hashes": [public]})
            t = (got.get("txs") or [{}])[0]
            if t.get("block_height") and not t.get("in_pool"):
                fk.log("public tx", public, "mined at", t["block_height"])
                break
            time.sleep(20)
        fk.wait_synced()
    scan_all(tag, "after", ("main", "ref"), after_extra)
    compare(tag, "after")
    check_private_effects(tag, "after")
    rings_after(tag)
