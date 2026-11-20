#!/usr/bin/env python3
"""Compare monero-rust with wallet2 on a real network (stagenet by default).

    cargo build --release --example stagenet_tool
    export MONERO_STAGENET_RPC=<synced node> MONERO_STAGENET_DIR=<run dir> MONERO_WALLET_RPC=<path>
    scripts/stagenet_compat.py scan LABEL

Each wallet's seed goes in $MONERO_STAGENET_DIR/seeds/LABEL.txt.
"""
import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagenet_lib as lib  # noqa: E402

RUN = lib.RUN
EVID = os.path.join(RUN, "evidence")
RINGDB = os.path.join(RUN, "ringdb")
# Registers exactly wallet2's 50 accounts by 200 subaddresses.
LOOKAHEAD = {"lookahead_account": 49, "lookahead_subaddress": 199}


def evidence(name, data):
    os.makedirs(EVID, exist_ok=True)
    path = os.path.join(EVID, name + ".json")
    with open(path, "w") as f:
        json.dump(data, f, indent=1, sort_keys=True)
    return path


def load_evidence(name):
    with open(os.path.join(EVID, name + ".json")) as f:
        return json.load(f)


def open_wallet(label, daemon=None, ringdb=RINGDB, auto_refresh=False, log_level=0):
    w = lib.WalletRpc(os.path.join(RUN, "w2", label), ringdb, daemon=daemon, log_level=log_level)
    w.call("open_wallet", {"filename": "w", "password": ""})
    w.call("auto_refresh", {"enable": auto_refresh})
    return w


def read_seed(label):
    with open(lib.seed_file(label)) as f:
        return f.read().strip()


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



def export_wallet2(label, tag=""):
    w = open_wallet(label)
    try:
        started = time.time()
        refreshed = w.call("refresh")
        view = w2_view(w)
        view["refresh"] = refreshed
        view["refresh_seconds"] = time.time() - started
        view["daemon_height"] = lib.daemon_height()
        evidence("w2-export-" + label + (("-" + tag) if tag else ""), view)
        return view
    finally:
        w.stop()


def cmd_w2_export(args):
    view = export_wallet2(args.label, args.tag)
    print(args.label, "wallet2 height", view["height"], "refresh", view["refresh"],
          "outputs", sum(len(v) for v in view["incoming"].values()))


def cmd_w2_restore(args):
    for label in args.labels:
        wdir = os.path.join(RUN, "w2", label)
        assert not os.path.exists(os.path.join(wdir, "w.keys")), "wallet2 wallet for %s exists" % label
        w = lib.WalletRpc(wdir, RINGDB)
        try:
            r = w.call("restore_deterministic_wallet",
                       {"filename": "w", "seed": read_seed(label), "restore_height": args.restore_height,
                        "password": "", "autosave_current": False})
            w.call("auto_refresh", {"enable": False})
            started = time.time()
            refreshed = w.call("refresh")
            print(label, r["address"], "refresh", refreshed, "%.0fs" % (time.time() - started), flush=True)
        finally:
            w.stop()


def cmd_w2_viewonly(args):
    w = open_wallet(args.label)
    try:
        address = w.call("get_address", {"account_index": 0})["address"]
        view_key = w.call("query_key", {"key_type": "view_key"})["key"]
    finally:
        w.stop(store=False)
    spend_public, _ = lib.address_keys(address)
    seed_path = lib.viewonly_seed_file(args.label)
    if not os.path.exists(seed_path):
        fd = os.open(seed_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write("viewonly:%s:%s\n" % (view_key, spend_public))
    name = args.label + "-vo"
    wdir = os.path.join(RUN, "w2", name)
    if os.path.exists(os.path.join(wdir, "w.keys")):
        print(name, "exists")
        return
    w = lib.WalletRpc(wdir, RINGDB)
    try:
        r = w.call("generate_from_keys", {"filename": "w", "address": address, "viewkey": view_key,
                                          "restore_height": args.restore_height, "password": "",
                                          "autosave_current": False})
        w.call("auto_refresh", {"enable": False})
        started = time.time()
        refreshed = w.call("refresh", timeout=7200)
        info = {"name": name, "address": address, "restore_height": args.restore_height, "created": r,
                "refresh": refreshed, "refresh_seconds": time.time() - started,
                "height": w.call("get_height")["height"]}
        evidence("wallet-" + name, info)
        print(json.dumps(info), flush=True)
    finally:
        w.stop()


def scan_response_path(label, tag=""):
    return os.path.join(RUN, "mr", (tag + "-" if tag else "") + label + "-scan.json")


def cmd_scan(args):
    tag = args.state_tag
    base = {"op": "scan", "restore_height": args.restore_height,
            "lookahead_account": args.lookahead_account, "lookahead_subaddress": args.lookahead_subaddress}
    if args.stop_height is not None:
        base["stop_height"] = args.stop_height
    name = "scan-" + (tag + "-" if tag else "") + "-".join(args.labels)
    if len(args.labels) == 1:
        label = args.labels[0]
        r = lib.tool(dict(base, seed_file=lib.seed_file(label), state=lib.state_file(label, tag)), name)
        with open(scan_response_path(label, tag), "w") as f:
            json.dump(r, f)
        print(label, json.dumps({k: r[k] for k in ("start_height", "height", "blocks", "seconds", "new_outputs",
                                                   "newly_spent", "balance", "reorgs")}))
        return
    wallets = [{"label": label, "seed_file": lib.seed_file(label), "state": lib.state_file(label, tag),
                "response": scan_response_path(label, tag)} for label in args.labels]
    print(json.dumps(lib.tool(dict(base, wallets=wallets), name), indent=1))


def cmd_scan_diff(args):
    tag = ("-" + args.tag) if args.tag else ""
    w2 = load_evidence("w2-export-" + args.label + tag)
    path = os.path.join(RUN, "mr", args.mr) if args.mr else scan_response_path(args.label, args.state_tag)
    with open(path) as f:
        mr = json.load(f)
    report = scan_diff(w2, mr)
    report["label"] = args.label
    path = evidence("scan-diff-" + args.label + tag + (("-" + args.name) if args.name else ""), report)
    summary = {k: v for k, v in report.items() if not isinstance(v, list) or len(v) <= 5}
    for key, value in report.items():
        if isinstance(value, list) and len(value) > 5:
            summary[key + "_count"] = len(value)
    print(json.dumps(summary, indent=1, sort_keys=True))
    print("evidence:", path)


def scan_diff(w2, mr):
    w2_out = {}
    w2_dupes = []
    for account, transfers in w2["incoming"].items():
        for t in transfers:
            key = (t["tx_hash"], t["pubkey"])
            if key in w2_out:
                w2_dupes.append(list(key))
            w2_out[key] = t
    mr_out = {}
    mr_dupes = []
    for o in mr["outputs"]:
        key = (o["tx_hash"], o["key"])
        if key in mr_out:
            mr_dupes.append(list(key))
        mr_out[key] = o

    only_w2 = [dict(w2_out[k], _key=list(k)) for k in sorted(set(w2_out) - set(mr_out))]
    only_mr = [dict(mr_out[k], _key=list(k)) for k in sorted(set(mr_out) - set(w2_out))]
    field_diffs = []
    checked = 0
    for key in sorted(set(w2_out) & set(mr_out)):
        a, b = w2_out[key], mr_out[key]
        checked += 1
        sub = b["subaddress_index"] or [0, 0]
        diffs = {}
        if a["amount"] != b["amount"]:
            diffs["amount"] = [a["amount"], b["amount"]]
        if a["block_height"] != b["block_height"]:
            diffs["block_height"] = [a["block_height"], b["block_height"]]
        if [a["subaddr_index"]["major"], a["subaddr_index"]["minor"]] != list(sub):
            diffs["subaddress"] = [a["subaddr_index"], sub]
        if a["global_index"] != b["global_index"]:
            diffs["global_index"] = [a["global_index"], b["global_index"]]
        if a["key_image"] != b["key_image"]:
            diffs["key_image"] = [a["key_image"], b["key_image"]]
        if a["spent"] != b["spent"]:
            diffs["spent"] = [a["spent"], b["spent"]]
        if not a["spent"] and not b["spent"] and a["unlocked"] != b["spendable"]:
            diffs["unlocked"] = [a["unlocked"], b["spendable"]]
        if diffs:
            field_diffs.append({"tx_hash": key[0], "pubkey": key[1], "diffs": diffs})

    w2_bal, w2_unl = {}, {}
    for sub in w2["balance"].get("per_subaddress", []):
        major = sub["account_index"]
        w2_bal[major] = w2_bal.get(major, 0) + sub["balance"]
        w2_unl[major] = w2_unl.get(major, 0) + sub["unlocked_balance"]
    mr_bal, mr_unl = {}, {}
    for o in mr["outputs"]:
        if o["spent"]:
            continue
        major = (o["subaddress_index"] or [0, 0])[0]
        mr_bal[major] = mr_bal.get(major, 0) + o["amount"]
        if o["spendable"] and not o.get("pending_spend"):
            mr_unl[major] = mr_unl.get(major, 0) + o["amount"]
    balance_rows = []
    for major in sorted(set(w2_bal) | set(mr_bal)):
        row = {"account": major, "w2_balance": w2_bal.get(major, 0), "mr_balance": mr_bal.get(major, 0),
               "w2_unlocked": w2_unl.get(major, 0), "mr_unlocked": mr_unl.get(major, 0)}
        row["match"] = row["w2_balance"] == row["mr_balance"] and row["w2_unlocked"] == row["mr_unlocked"]
        balance_rows.append(row)

    mr_balance = mr.get("balance") or {}
    mr_total = sum(mr_balance.get(k, 0) for k in ("confirmed", "unconfirmed"))
    total_row = {"w2_balance": w2["balance"].get("balance"), "w2_unlocked": w2["balance"].get("unlocked_balance"),
                 "mr_confirmed": mr_balance.get("confirmed"), "mr_unconfirmed": mr_balance.get("unconfirmed"),
                 "mr_pending_spend": mr_balance.get("pending_spend"),
                 "mr_confirmed_unconfirmed": mr_total}
    total_row["match"] = total_row["w2_balance"] == mr_total
    balance_rows.append(dict(total_row, account="all"))

    mr_spends = {}
    for o in mr["outputs"]:
        if o["spent"] and o.get("spent_by_tx"):
            s = mr_spends.setdefault(o["spent_by_tx"], {"inputs": 0, "height": o["spent_height"], "accounts": set()})
            s["inputs"] += o["amount"]
            s["accounts"].add((o["subaddress_index"] or [0, 0])[0])
    received_in_tx = {}
    for o in mr["outputs"]:
        major = (o["subaddress_index"] or [0, 0])[0]
        received_in_tx.setdefault(o["tx_hash"], {}).setdefault(major, 0)
        received_in_tx[o["tx_hash"]][major] += o["amount"]
    w2_spends = {}
    for t in w2["transfers"].get("out", []):
        s = w2_spends.setdefault(t["txid"], {"amount": 0, "fee": t["fee"], "height": t["height"], "accounts": set()})
        s["amount"] += t["amount"]
        s["accounts"].add(t["subaddr_index"]["major"])
    out_only_w2 = sorted(set(w2_spends) - set(mr_spends))
    out_only_mr = sorted(set(mr_spends) - set(w2_spends))
    out_diffs = []
    for txid in sorted(set(w2_spends) & set(mr_spends)):
        a, b = w2_spends[txid], mr_spends[txid]
        diffs = {}
        if a["height"] != b["height"]:
            diffs["height"] = [a["height"], b["height"]]
        change = sum(v for major, v in received_in_tx.get(txid, {}).items() if major in b["accounts"])
        if a["amount"] + a["fee"] != b["inputs"] - change:
            diffs["amount_plus_fee"] = {"w2": a["amount"] + a["fee"], "mr_inputs_minus_change": b["inputs"] - change,
                                        "mr_inputs": b["inputs"], "mr_change": change}
        if a["accounts"] != b["accounts"]:
            diffs["accounts"] = [sorted(a["accounts"]), sorted(b["accounts"])]
        if diffs:
            out_diffs.append({"txid": txid, "diffs": diffs})

    spent_without_tx = [o["key_image"] for o in mr["outputs"] if o["spent"] and not o.get("spent_by_tx")]
    return {
        "w2_height": w2["height"], "mr_height": mr["height"],
        "w2_outputs": len(w2_out), "mr_outputs": len(mr_out), "outputs_compared": checked,
        "w2_unspent": sum(1 for t in w2_out.values() if not t["spent"]),
        "mr_unspent": sum(1 for o in mr_out.values() if not o["spent"]),
        "w2_duplicate_keys": w2_dupes, "mr_duplicate_keys": mr_dupes,
        "only_wallet2": only_w2, "only_monero_rust": only_mr, "field_diffs": field_diffs,
        "balances": balance_rows,
        "w2_in_transfers": len(w2["transfers"].get("in", [])),
        "w2_out_transfers": len(w2["transfers"].get("out", [])),
        "w2_outgoing_txs": len(w2_spends), "mr_outgoing_txs": len(mr_spends),
        "outgoing_only_wallet2": out_only_w2, "outgoing_only_monero_rust": out_only_mr,
        "outgoing_diffs": out_diffs,
        "mr_spent_without_tx_hash": spent_without_tx,
        "mr_scan": {k: mr.get(k) for k in ("start_height", "height", "blocks", "transactions", "seconds",
                                           "fetch_seconds", "process_seconds", "blocks_per_second",
                                           "conflicts", "reorgs", "balance")},
    }



def cmd_final_check(args):
    out = {}
    for label in args.labels:
        tags = ["" if t == "live" else t for t in args.state_tags]
        tags = [t for t in tags if os.path.exists(lib.state_file(label, t))]
        assert tags, "no crate state file for %s" % label
        for _ in range(4):
            scans = {}
            for t in tags:
                request = {"op": "scan", "seed_file": lib.seed_file(label), "state": lib.state_file(label, t)}
                request.update(LOOKAHEAD)
                scans[t] = lib.tool(request, "%s-%s-%s" % (args.tag, label, t or "live"))
            view = export_wallet2(label, args.tag)
            if all(scan["height"] == view["height"] for scan in scans.values()):
                break
        row = {"w2_height": view["height"]}
        for t, scan in scans.items():
            with open(scan_response_path(label, t), "w") as f:
                json.dump(scan, f)
            diff = scan_diff(view, scan)
            diff["label"] = label
            evidence("scan-diff-%s-%s-%s" % (label, args.tag, t or "live"), diff)
            row[t or "live"] = {
                "height": scan["height"], "outputs": [diff["w2_outputs"], diff["mr_outputs"]],
                "only_w2": len(diff["only_wallet2"]), "only_mr": len(diff["only_monero_rust"]),
                "field_diffs": len(diff["field_diffs"]),
                "outgoing": [diff["w2_outgoing_txs"], diff["mr_outgoing_txs"],
                             len(diff["outgoing_diffs"]) + len(diff["outgoing_only_wallet2"])
                             + len(diff["outgoing_only_monero_rust"])],
                "balances": diff["balances"]}
        out[label] = row
        print(label, json.dumps(row, indent=1), flush=True)
    evidence("final-check-" + args.tag, out)


def cmd_lookahead_check(args):
    view = load_evidence("w2-export-" + args.label + (("-" + args.tag) if args.tag else ""))
    rows = []
    for account, subaddress, name in ((0, 20, "default"), (49, 199, "wallet2")):
        state = os.path.join(RUN, "mr", "la-%s-%s-state.json" % (args.label, name))
        if os.path.exists(state):
            os.remove(state)
        r = lib.tool({"op": "scan", "seed_file": lib.seed_file(args.label), "state": state,
                      "restore_height": args.start, "stop_height": args.stop,
                      "lookahead_account": account, "lookahead_subaddress": subaddress},
                     "la-%s-%s-%d" % (args.label, name, args.start))
        found = {(o["tx_hash"], o["key"]) for o in r["outputs"]}
        theirs = [t for transfers in view["incoming"].values() for t in transfers
                  if args.start <= t["block_height"] < r["height"]]
        missing = [t for t in theirs if (t["tx_hash"], t["pubkey"]) not in found]
        rows.append({"wallet": args.label, "range": [args.start, r["height"]], "lookahead": [account, subaddress],
                     "wallet2_outputs": len(theirs), "monero_rust_outputs": len(r["outputs"]),
                     "missing": len(missing), "missing_amount": sum(t["amount"] for t in missing),
                     "missing_subaddresses": sorted({(t["subaddr_index"]["major"], t["subaddr_index"]["minor"])
                                                     for t in missing})})
        print(json.dumps(rows[-1]), flush=True)
    evidence("lookahead-%s-%d" % (args.label, args.start), rows)


def cmd_new_wallet(args):
    lib.ensure_dirs()
    wdir = os.path.join(RUN, "w2", args.name)
    seed_path = lib.seed_file(args.name)
    assert not os.path.exists(os.path.join(wdir, "w.keys")), "wallet exists"
    assert not os.path.exists(seed_path), "seed file exists"
    info = {"name": args.name}
    if args.crate_seed:
        made = lib.tool({"op": "seed", "seed_file": seed_path}, "seed-" + args.name)
        os.chmod(seed_path, 0o600)
        info["seed_generated_by"] = "monero-rust generate_seed(Classic)"
        info["crate_primary"] = made["primary"]
    else:
        w = lib.WalletRpc(wdir, RINGDB)
        try:
            w.call("create_wallet", {"filename": "created", "password": "", "language": "English"})
            w.call("auto_refresh", {"enable": False})
            seed = w.call("query_key", {"key_type": "mnemonic"})["key"]
        finally:
            w.stop(store=False)
        fd = os.open(seed_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(seed + "\n")
        for leftover in ("created", "created.keys", "created.address.txt"):
            if os.path.exists(os.path.join(wdir, leftover)):
                os.remove(os.path.join(wdir, leftover))
        info["seed_generated_by"] = "wallet2"
    info["restore_height"] = lib.daemon_height() - 5
    w = lib.WalletRpc(wdir, RINGDB)
    try:
        r = w.call("restore_deterministic_wallet",
                   {"filename": "w", "seed": read_seed(args.name), "restore_height": info["restore_height"],
                    "password": "", "autosave_current": False})
        w.call("auto_refresh", {"enable": False})
        info["primary"] = r["address"]
        if args.crate_seed:
            assert info["primary"] == info["crate_primary"], "wallet2 and the crate derive different addresses"
        info["sub_0_1"] = w.call("create_address", {"account_index": 0})["address"]
        info["sub_0_2"] = w.call("create_address", {"account_index": 0})["address"]
        info["sub_1_0"] = w.call("create_account")["address"]
        integrated = w.call("make_integrated_address", {})
        info["integrated"] = integrated["integrated_address"]
        info["integrated_payment_id"] = integrated["payment_id"]
        info["refresh"] = w.call("refresh")
    finally:
        w.stop()
    evidence("wallet-" + args.name, info)
    print(json.dumps(info, indent=1))


def cmd_proxy(args):
    import signal
    lib.ensure_dirs()
    upstream = lib.rpc_url()
    proxy = lib.LoggingProxy(upstream, lib.proxy_log(args.label), args.label, light=args.light)
    with open(os.path.join(RUN, "proxy-%s.port" % args.label), "w") as f:
        f.write(str(proxy.port))
    print("proxy", args.label, proxy.url, "->", upstream, flush=True)
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    try:
        while True:
            time.sleep(3600)
    finally:
        proxy.stop()


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("w2-export", help="refresh a wallet2 wallet and export its view")
    s.add_argument("label")
    s.add_argument("--tag", default="")
    s.set_defaults(fn=cmd_w2_export)
    s = sub.add_parser("w2-restore", help="create wallet2 wallets from seed files")
    s.add_argument("labels", nargs="+")
    s.add_argument("--restore-height", type=int, default=0)
    s.set_defaults(fn=cmd_w2_restore)
    s = sub.add_parser("w2-viewonly", help="wallet2 view-only wallet LABEL-vo, and the crate's view-only keys")
    s.add_argument("label")
    s.add_argument("--restore-height", type=int, default=0)
    s.set_defaults(fn=cmd_w2_viewonly)
    s = sub.add_parser("scan", help="scan one or more wallets with the crate")
    s.add_argument("labels", nargs="+")
    s.add_argument("--state-tag", default="", help="use a separate state file mr/TAG-LABEL-state.json")
    s.add_argument("--restore-height", type=int, default=0, help="only used when the state file is new")
    s.add_argument("--stop-height", type=int, default=None)
    s.add_argument("--lookahead-account", type=int, default=LOOKAHEAD["lookahead_account"])
    s.add_argument("--lookahead-subaddress", type=int, default=LOOKAHEAD["lookahead_subaddress"])
    s.set_defaults(fn=cmd_scan)
    s = sub.add_parser("scan-diff", help="diff a crate scan against a wallet2 export")
    s.add_argument("label")
    s.add_argument("--mr", default=None, help="crate scan file under mr/, instead of the one --state-tag names")
    s.add_argument("--state-tag", default="")
    s.add_argument("--tag", default="", help="tag of the wallet2 export")
    s.add_argument("--name", default="")
    s.set_defaults(fn=cmd_scan_diff)
    s = sub.add_parser("final-check", help="bring crate states and wallet2 to one height and diff them")
    s.add_argument("labels", nargs="+")
    s.add_argument("--state-tags", nargs="+", default=["live"], help="`live` is the untagged state file")
    s.add_argument("--tag", default="final")
    s.set_defaults(fn=cmd_final_check)
    s = sub.add_parser("lookahead-check", help="default lookahead against wallet2's window on a block range")
    s.add_argument("label")
    s.add_argument("start", type=int)
    s.add_argument("stop", type=int)
    s.add_argument("--tag", default="", help="tag of the wallet2 export to compare with")
    s.set_defaults(fn=cmd_lookahead_check)
    s = sub.add_parser("new-wallet", help="fresh wallet2 recipient wallet")
    s.add_argument("name")
    s.add_argument("--crate-seed", action="store_true", help="let the crate generate the seed")
    s.set_defaults(fn=cmd_new_wallet)
    s = sub.add_parser("proxy", help="run one logging proxy in front of the node")
    s.add_argument("label")
    s.add_argument("--light", action="store_true", help="log sizes only (metering), no reply decoding")
    s.set_defaults(fn=cmd_proxy)
    import stagenet_send
    stagenet_send.register(sub)
    import stagenet_viewonly
    stagenet_viewonly.register(sub)
    import stagenet_live
    stagenet_live.register(sub)
    args = p.parse_args()
    lib.ensure_dirs()
    args.fn(args)


if __name__ == "__main__":
    main()
