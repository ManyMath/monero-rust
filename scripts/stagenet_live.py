"""Live re-verification subcommands of stagenet_compat.py.

MONERO_STAGENET_MAX_RELAYS (default 25) caps the relays in evidence/relayed.json.
"""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagenet_lib as lib  # noqa: E402
import stagenet_send as send  # noqa: E402

RUN = lib.RUN
EVID = os.path.join(RUN, "evidence")
XMR = 10 ** 12


def ledger():
    return send.load_evidence("relayed", [])


def budget_left():
    return send.MAX_RELAYS - len(ledger())


def cmd_preflight(args):
    out = {"run_dir": RUN, "tool": lib.TOOL, "tool_built": os.path.exists(lib.TOOL),
           "wallet_rpc": lib.WALLET_RPC, "max_relays": send.MAX_RELAYS, "relays_used": len(ledger())}
    info = lib.daemon_json("get_info")
    out["node"] = {k: info.get(k) for k in ("height", "target_height", "synchronized", "nettype", "restricted",
                                            "tx_pool_size", "version", "busy_syncing", "offline")}
    out["seeds"] = {label: os.path.exists(lib.seed_file(label)) for label in args.labels}
    problems = []
    if not out["tool_built"]:
        problems.append("stagenet_tool not built")
    if not info.get("synchronized") or info.get("busy_syncing"):
        problems.append("node not synchronized")
    if info.get("restricted"):
        problems.append("node RPC is restricted; the harness needs the unrestricted port")
    if info.get("nettype") != lib.NETWORK:
        problems.append("node is on %s, not %s" % (info.get("nettype"), lib.NETWORK))
    problems += ["no seed file for %s" % label for label, ok in out["seeds"].items() if not ok]
    out["problems"] = problems
    send.evidence("preflight", out)
    print(json.dumps(out, indent=1))
    if problems:
        raise SystemExit("preflight failed")


def w2_funds(view):
    by_sub = {}
    for transfers in view["incoming"].values():
        for t in transfers:
            if t["spent"]:
                continue
            key = "%d,%d" % (t["subaddr_index"]["major"], t["subaddr_index"]["minor"])
            row = by_sub.setdefault(key, {"outputs": 0, "amount": 0, "unlocked_outputs": 0,
                                          "pre_ringct_outputs": 0, "pre_ringct_amount": 0})
            row["outputs"] += 1
            row["amount"] += t["amount"]
            row["unlocked_outputs"] += 1 if t.get("unlocked") else 0
    balance = view["balance"]
    return {"height": view["height"], "balance": balance.get("balance"),
            "unlocked_balance": balance.get("unlocked_balance"),
            "per_account": {str(a["account_index"]): [a["balance"], a["unlocked_balance"]]
                            for a in view["accounts"].get("subaddress_accounts", [])},
            "unspent_by_subaddress": dict(sorted(by_sub.items()))}


def mr_funds(scan):
    by_sub = {}
    per_account = {}
    for o in scan["outputs"]:
        if o["spent"]:
            continue
        sub = o["subaddress_index"] or [0, 0]
        key = "%d,%d" % tuple(sub)
        row = by_sub.setdefault(key, {"outputs": 0, "amount": 0, "unlocked_outputs": 0,
                                      "pre_ringct_outputs": 0, "pre_ringct_amount": 0})
        unlocked = o["spendable"] and not o.get("pending_spend")
        row["outputs"] += 1
        row["amount"] += o["amount"]
        row["unlocked_outputs"] += 1 if unlocked else 0
        if o.get("pre_ringct"):
            row["pre_ringct_outputs"] += 1
            row["pre_ringct_amount"] += o["amount"]
        acc = per_account.setdefault(str(sub[0]), [0, 0])
        acc[0] += o["amount"]
        acc[1] += o["amount"] if unlocked else 0
    bal = scan["balance"]
    return {"height": scan["height"], "balance_struct": bal,
            "balance": bal["confirmed"] + bal["unconfirmed"] + bal.get("pre_ringct", 0),
            "unlocked_balance": sum(v[1] for v in per_account.values()),
            "pre_ringct": bal.get("pre_ringct"), "per_account": per_account,
            "unspent_by_subaddress": dict(sorted(by_sub.items())),
            "unspent_outputs": [{k: o.get(k) for k in ("tx_hash", "output_index", "amount", "subaddress_index",
                                                       "block_height", "global_index", "spendable",
                                                       "pending_spend", "pre_ringct")}
                                for o in scan["outputs"] if not o["spent"]]}


def cmd_funds(args):
    import stagenet_compat as compat
    label = args.label
    for attempt in range(4):
        scan = send.mr_scan(label, "funds-%s-%s-scan" % (args.tag, label))
        view = compat.export_wallet2(label, "funds-" + args.tag)
        if scan["height"] == view["height"]:
            break
        print("heights differ (crate %d, wallet2 %d), again" % (scan["height"], view["height"]), flush=True)
    with open(compat.scan_response_path(label), "w") as f:
        json.dump(scan, f)
    diff = compat.scan_diff(view, scan)
    w2, mr = w2_funds(view), mr_funds(scan)
    flagged = {(o["tx_hash"], o["key"]) for o in scan["outputs"] if o.get("pre_ringct") and not o["spent"]}
    w2_unspent = {(t["tx_hash"], t["pubkey"]): t for v in view["incoming"].values() for t in v if not t["spent"]}
    w2["pre_ringct"] = sum(w2_unspent[k]["amount"] for k in flagged if k in w2_unspent)
    w2["pre_ringct_outputs_found"] = sum(1 for k in flagged if k in w2_unspent)
    subs = sorted(set(w2["unspent_by_subaddress"]) | set(mr["unspent_by_subaddress"]),
                  key=lambda k: tuple(int(x) for x in k.split(",")))
    agree = {
        "height": w2["height"] == mr["height"],
        "pre_ringct": w2["pre_ringct"] == mr["pre_ringct"] and w2["pre_ringct_outputs_found"] == len(flagged),
        "balance": w2["balance"] == mr["balance"],
        "unlocked_balance": w2["unlocked_balance"] == mr["unlocked_balance"],
        "per_account": {k: v for k, v in w2["per_account"].items() if v != [0, 0]} ==
                       {k: v for k, v in mr["per_account"].items() if v != [0, 0]},
        "unspent_by_subaddress": all(
            (w2["unspent_by_subaddress"].get(k, {}).get("outputs"), w2["unspent_by_subaddress"].get(k, {}).get("amount"))
            == (mr["unspent_by_subaddress"].get(k, {}).get("outputs"),
                mr["unspent_by_subaddress"].get(k, {}).get("amount")) for k in subs),
        "outputs": not diff["only_wallet2"] and not diff["only_monero_rust"] and not diff["field_diffs"],
    }
    out = {"label": label, "tag": args.tag, "wallet2": w2, "monero_rust": mr, "agree": agree,
           "all_agree": all(agree.values()),
           "scan_diff": {k: diff[k] for k in ("w2_outputs", "mr_outputs", "w2_unspent", "mr_unspent",
                                              "w2_outgoing_txs", "mr_outgoing_txs", "balances")},
           "differences": {"only_wallet2": diff["only_wallet2"], "only_monero_rust": diff["only_monero_rust"],
                           "field_diffs": diff["field_diffs"], "outgoing_diffs": diff["outgoing_diffs"]}}
    path = send.evidence("funds-%s-%s" % (label, args.tag), out)
    brief = {k: out[k] for k in ("agree", "all_agree")}
    brief["wallet2"] = {k: w2[k] for k in ("height", "balance", "unlocked_balance", "per_account",
                                           "unspent_by_subaddress")}
    brief["monero_rust"] = {k: mr[k] for k in ("height", "balance", "unlocked_balance", "pre_ringct",
                                               "balance_struct", "per_account", "unspent_by_subaddress")}
    print(json.dumps(brief, indent=1))
    print("evidence:", path)


def addresses(label, pairs):
    r = lib.tool({"op": "addr", "seed_file": lib.seed_file(label), "subaddresses": pairs},
                 "addr-%s-%d" % (label, int(time.time())))
    return r["primary"], {(a, i): address for a, i, address in r["subaddresses"]}


def cmd_prep(args):
    label = args.label
    dests = []
    for spec in args.dest:
        major, minor, amount = spec.split(",")
        dests.append((int(major), int(minor), int(round(float(amount) * XMR))))
    _, subs = addresses(label, [[m, n] for m, n, _ in dests])
    destinations = [{"address": subs[(m, n)], "amount": v} for m, n, v in dests]
    tiers = send.fee_tiers()
    if args.relay and budget_left() < 1:
        raise SystemExit("relay budget used")
    w = send.open_wallet(label)
    out = {"label": label, "account": args.account, "destinations": [[m, n, v] for m, n, v in dests]}
    try:
        w.call("refresh")
        out["balance_before"] = w.call("get_balance", {"account_index": args.account})
        r = w.raw("transfer", {"destinations": destinations, "account_index": args.account, "priority": 0,
                               "do_not_relay": True, "get_tx_hex": True, "get_tx_metadata": True,
                               "get_tx_key": True})
        if "error" in r:
            out["error"] = r["error"]
            send.evidence("prep-%s" % args.tag, out)
            raise SystemExit("wallet2 did not build: %s" % r["error"])
        t = r["result"]
        tx = lib.decode_tx(t["tx_blob"])
        checks = {"version_2": tx["version"] == 2, "unlock_time_0": tx["unlock_time"] == 0,
                  "rct_type_bulletproof_plus": tx.get("rct_type") == 6,
                  "outputs": len(tx["outputs"]) == len(dests) + 1,
                  "ring_size_16": all(len(i["offsets"]) == 16 for i in tx["inputs"]),
                  "fee_sane": 0 < tx["fee"] <= send.MAX_SANE_FEE and tx["fee"] == t["fee"],
                  "fee_matches_a_daemon_tier": bool(send.fee_tier_of(tx, tiers)),
                  "no_trailing_bytes": tx.get("trailing_bytes") == 0,
                  "amount": t["amount"] == sum(v for _, _, v in dests)}
        kis = [i["key_image"] for i in tx["inputs"]]
        used = {k for e in ledger() for k in e.get("key_images", [])}
        checks["inputs_not_in_an_earlier_relay"] = not (set(kis) & used)
        out.update(tx_id=t["tx_hash"], fee=t["fee"], amount=t["amount"], tx_hex=t["tx_blob"], checks=checks,
                   observer=lib.observer_view(tx), all_checks_pass=all(checks.values()))
        if args.relay and out["all_checks_pass"]:
            out["guard"] = send.guard(kis)
            entries = ledger()
            entry = {"name": "prep-" + args.tag, "tx_id": t["tx_hash"], "by": "wallet2", "key_images": kis,
                     "guard": out["guard"], "started": time.time(), "result": "pending"}
            entries.append(entry)
            send.evidence("relayed", entries)
            relayed = w.raw("relay_tx", {"hex": t["tx_metadata"]})
            entry["result"] = "OK" if "result" in relayed else json.dumps(relayed.get("error"))
            time.sleep(1)
            entry["status_after"] = send.tx_status(t["tx_hash"])
            entry["finished"] = time.time()
            send.evidence("relayed", entries)
            out["relay"] = entry
    finally:
        w.stop()
    path = send.evidence("prep-%s" % args.tag, out)
    print(json.dumps({k: out.get(k) for k in ("tx_id", "fee", "amount", "checks", "all_checks_pass", "relay")},
                     indent=1))
    print("evidence:", path)


def cmd_wait(args):
    deadline = time.time() + args.timeout
    last = 0
    while True:
        status = send.tx_status(args.txid)
        height = lib.daemon_height()
        mined = status.get("block_height") if status.get("known") and not status.get("in_pool") else None
        done = mined is not None and height >= mined + args.blocks
        if done or time.time() - last >= 60:
            print(time.strftime("%H:%M:%S"), "height", height, "tx", status, "need",
                  None if mined is None else mined + args.blocks, flush=True)
            last = time.time()
        if done:
            return
        if time.time() > deadline:
            raise SystemExit("timed out")
        time.sleep(10)


def cmd_live_plan(args):
    sender, rcpt_label, fresh_label = args.sender, args.recipient, args.fresh
    rcpt = send.load_evidence("wallet-" + rcpt_label)
    fresh = send.load_evidence("wallet-" + fresh_label)
    assert rcpt and fresh, "create the recipient wallets with new-wallet first"
    sender_primary, sender_subs = addresses(sender, [[0, 1]])
    owners = {
        rcpt["primary"]: {"wallet": rcpt_label, "major": 0, "minor": 0},
        rcpt["sub_0_1"]: {"wallet": rcpt_label, "major": 0, "minor": 1},
        rcpt["sub_0_2"]: {"wallet": rcpt_label, "major": 0, "minor": 2},
        rcpt["sub_1_0"]: {"wallet": rcpt_label, "major": 1, "minor": 0},
        rcpt["integrated"]: {"wallet": rcpt_label, "major": 0, "minor": 0},
        fresh["primary"]: {"wallet": fresh_label, "major": 0, "minor": 0},
        fresh["sub_0_1"]: {"wallet": fresh_label, "major": 0, "minor": 1},
        sender_primary: {"wallet": sender, "major": 0, "minor": 0},
        sender_subs[(0, 1)]: {"wallet": sender, "major": 0, "minor": 1},
    }
    c = XMR // 100
    plan = send.load_evidence("shapes", {})
    plan.update({
        "_address_owners": owners,
        "_relay_budget": send.MAX_RELAYS,
        "probe1": {"wallet": sender, "kind": "send", "account": 0, "recipients": [[rcpt["primary"], c]],
                   "intent": "never relayed: fees and request capture"},
        "acct1": {"wallet": sender, "kind": "send", "account": 1, "recipients": [[rcpt["primary"], c]],
                  "intent": "account 1, standard recipient, free selection: one input from one subaddress; "
                            "change to (1,0)"},
        "standard": {"wallet": sender, "kind": "send", "account": 0, "recipients": [[rcpt["primary"], 3 * c]],
                     "intent": "standard recipient, free selection in account 0"},
        "subaddr": {"wallet": sender, "kind": "send", "account": 0, "recipients": [[rcpt["sub_0_1"], 2 * c]],
                    "intent": "one subaddress recipient"},
        "multiout": {"wallet": sender, "kind": "send", "account": 0,
                     "recipients": [[rcpt["primary"], c], [rcpt["sub_0_2"], c],
                                    [fresh["primary"], 2 * c], [fresh["sub_0_1"], c // 2]],
                     "intent": "four destinations plus change; funds the fresh wallet at two subaddresses (the "
                               "sweep takes one, as wallet2's sweep_all does)"},
        "integrated": {"wallet": sender, "kind": "send", "account": 0,
                       "recipients": [[rcpt["integrated"], c]], "payment_id": rcpt["integrated_payment_id"],
                       "intent": "integrated address recipient"},
        "submix": {"wallet": sender, "kind": "send", "account": 0,
                   "recipients": [[rcpt["sub_0_2"], c], [rcpt["primary"], c]],
                   "intent": "subaddress plus standard recipients (second round)"},
        "sweep": {"wallet": fresh_label, "kind": "sweep", "account": 0, "recipients": [[sender_primary, 0]],
                  "intent": "zero-change sweep of the fresh wallet back to the sender, after multiout unlocks"},
        "refusal": {"wallet": sender, "kind": "send", "account": 0,
                    "recipients": [[rcpt["integrated"], c], [fresh["primary"], c]],
                    "intent": "never relayed: integrated address with a second destination must be refused"},
    })
    plan["_order"] = ["prep (wallet2)", "wait 10", "exact", "rebuild", "acct1", "standard", "subaddr",
                      "integrated", "multiout", "wait 10 after the last of them", "submix", "sweep",
                      "wait 10", "sweep-back rcpt 0 (wallet2)"]
    path = send.evidence("shapes", plan)
    print(json.dumps({k: v for k, v in plan.items() if k != "_address_owners"}, indent=1))
    print("evidence:", path)


def save_shape(name, shape):
    plan = send.shapes()
    plan[name] = shape
    send.evidence("shapes", plan)


def scanned_outputs(label, name):
    return send.mr_scan(label, name)["outputs"]


def cmd_exact(args):
    plan = send.shapes()
    label = args.wallet
    to = args.to or plan["standard"]["recipients"][0][0]
    outputs = scanned_outputs(label, "scan-%s-pre-%s-probe" % (label, args.name))
    mine = [o for o in outputs if "%s:%d" % (o["tx_hash"], o["output_index"]) == args.input]
    assert len(mine) == 1 and not mine[0]["spent"] and mine[0]["spendable"] and not mine[0]["pending_spend"], \
        "input must be an unspent, unlocked output of the wallet"
    source = mine[0]
    account = (source["subaddress_index"] or [0, 0])[0]
    probe = send.mr_build(label, "build-%s-probe" % args.name, "send", [[to, source["amount"] // 2]], account,
                          [args.input], args.priority, record=True,
                          tx_out=os.path.join(EVID, "tx-%s-probe.json" % args.name))
    assert probe.get("builds"), "probe did not build: %s %s" % (probe.get("prepare_error"), probe.get("build_errors"))
    fee = probe["builds"][0]["fee"]
    amount = source["amount"] - fee
    shape = {"wallet": label, "kind": "send", "account": account, "recipients": [[to, amount]],
             "inputs": [args.input], "priority": args.priority, "w2_priority": send.PRIORITIES.index(args.priority),
             "w2_subaddr_indices": [(source["subaddress_index"] or [0, 0])[1]],
             "expect_zero_change": True, "probe_fee": fee,
             "intent": "input equals amount plus fee: zero change, paid with a dummy output"}
    save_shape(args.name, shape)
    out = {"name": args.name, "input": source, "probe_fee": fee, "amount": amount, "shape": shape}
    send.evidence("exact-%s" % args.name, out)
    print(json.dumps(out, indent=1))


def relatedness(a, b):
    if a["tx_hash"] == b["tx_hash"]:
        return 100
    d = abs(a["block_height"] - b["block_height"])
    return 90 if d == 0 else 80 if d == 1 else 20 if d <= 9 else 0


def cmd_rebuild(args):
    plan = send.shapes()
    label = args.wallet
    to = args.to or plan["standard"]["recipients"][0][0]
    outputs = scanned_outputs(label, "scan-%s-pre-%s-find" % (label, args.name))
    est = lib.tool({"op": "estimate", "priority": args.priority, "shapes": [[1, 2], [2, 2]]},
                   "estimate-%s" % args.name)
    e12, e22 = est["shapes"][0]["wallet2_fee"], est["shapes"][1]["wallet2_fee"]
    cands = [o for o in outputs if not o["spent"] and o["spendable"] and not o["pending_spend"]
             and not o.get("pre_ringct") and (o["subaddress_index"] or [0, 0])[0] == args.account]
    by_sub = {}
    for o in cands:
        by_sub.setdefault(tuple(o["subaddress_index"] or [0, 0]), []).append(o)
    order = sorted(by_sub, key=lambda s: -sum(o["amount"] for o in by_sub[s]))
    out = {"name": args.name, "account": args.account, "estimate": est, "subaddress_order": order,
           "candidates": len(cands)}
    reasons = []
    if not order:
        reasons.append("no unlocked outputs in the account")
    else:
        top = order[0]
        if len(by_sub[top]) != 1:
            reasons.append("the largest subaddress %s holds %d outputs, not one" % (top, len(by_sub[top])))
        if len(cands) < 2:
            reasons.append("no second output in the account to add")
        if len(order) > 1 and sum(o["amount"] for o in by_sub[order[1]]) == sum(o["amount"] for o in by_sub[top]):
            reasons.append("two subaddresses tie for the largest balance")
        x = by_sub[top][0]
        amount = x["amount"] - e12
        preferred = amount + e22
        if any(o["amount"] >= preferred for o in cands):
            reasons.append("an output alone covers amount plus the two-input estimate")
        for sub, group in by_sub.items():
            for i in range(len(group)):
                for j in range(i + 1, len(group)):
                    if relatedness(group[i], group[j]) < 100 and group[i]["amount"] + group[j]["amount"] >= preferred:
                        reasons.append("a pair at %s covers amount plus the two-input estimate" % (sub,))
        out.update(x="%s:%d" % (x["tx_hash"], x["output_index"]), x_amount=x["amount"], amount=amount,
                   wallet2_estimate_1_2=e12, wallet2_estimate_2_2=e22)
    out["reasons_it_cannot_be_triggered"] = sorted(set(reasons))
    out["triggerable"] = not reasons
    if out["triggerable"]:
        shape = {"wallet": label, "kind": "send", "account": args.account, "recipients": [[to, out["amount"]]],
                 "priority": args.priority, "w2_priority": send.PRIORITIES.index(args.priority),
                 "expect_rebuild": out["x"],
                 "intent": "selection stops at wallet2's estimate with one input; the real fee is not covered, "
                           "so an input is added and the transaction rebuilt, as wallet2 does"}
        save_shape(args.name, shape)
        out["shape"] = shape
    send.evidence("rebuild-%s" % args.name, out)
    print(json.dumps({k: v for k, v in out.items() if k != "estimate"}, indent=1))


def cmd_refusal(args):
    plan = send.shapes()
    shape = plan[args.shape]
    label = shape["wallet"]
    log = os.path.join(EVID, "proxy-refusal-%s.jsonl" % args.tag)
    if os.path.exists(log):
        os.remove(log)
    proxy = lib.LoggingProxy(lib.rpc_url(), log, "refusal")
    try:
        built = send.mr_build(label, "build-refusal-%s" % args.tag, "send", shape["recipients"], shape["account"],
                              None, rpc=proxy.url, fresh=True)
    finally:
        proxy.stop()
    calls = [lib.call_name(e) for e in lib.read_log(log)]
    out = {"shape": shape, "monero_rust": {"prepare_error": built.get("prepare_error"),
                                           "build_errors": built.get("build_errors"),
                                           "built": len(built.get("builds", [])), "requests": calls}}
    w = send.open_wallet(label)
    try:
        w.call("refresh")
        out["wallet2"] = send.w2_transfer(w, shape)
        out["wallet2"].pop("tx_hex", None)
    finally:
        w.stop(store=False)
    errors = " ".join(built.get("build_errors") or []) + " " + str(built.get("prepare_error") or "")
    out["checks"] = {"crate_refused": not built.get("builds"),
                     "crate_names_the_integrated_address": "integrated address" in errors,
                     "no_decoy_request": not any(c in ("get_outs.bin", "get_output_distribution.bin") for c in calls),
                     "wallet2_refused": "error" in out["wallet2"]}
    out["all_checks_pass"] = all(out["checks"].values())
    path = send.evidence("refusal-%s" % args.tag, out)
    print(json.dumps(out, indent=1))
    print("evidence:", path)


def cmd_budget(args):
    entries = ledger()
    for e in entries:
        print(e.get("name"), e.get("tx_id"), e.get("by"), e.get("result"))
    print("used %d of %d, left %d" % (len(entries), send.MAX_RELAYS, budget_left()))


def register(sub):
    s = sub.add_parser("preflight", help="node, binaries, seeds, relay budget (read-only)")
    s.add_argument("labels", nargs="*", default=["hemlock"])
    s.set_defaults(fn=cmd_preflight)
    s = sub.add_parser("funds", help="funds of one wallet, crate and wallet2 at one height")
    s.add_argument("label")
    s.add_argument("--tag", default="now")
    s.set_defaults(fn=cmd_funds)
    s = sub.add_parser("prep", help="wallet2 pays the wallet's own subaddresses (build; --relay to relay)")
    s.add_argument("label")
    s.add_argument("--account", type=int, default=0)
    s.add_argument("--dest", action="append", required=True, help="MAJOR,MINOR,AMOUNT_XMR; repeatable")
    s.add_argument("--tag", default="1")
    s.add_argument("--relay", action="store_true")
    s.set_defaults(fn=cmd_prep)
    s = sub.add_parser("wait", help="until a transaction has BLOCKS blocks on top")
    s.add_argument("txid")
    s.add_argument("blocks", type=int)
    s.add_argument("--timeout", type=int, default=7200)
    s.set_defaults(fn=cmd_wait)
    s = sub.add_parser("live-plan", help="evidence/shapes.json for the live rerun")
    s.add_argument("--sender", default="hemlock")
    s.add_argument("--recipient", default="rcpt")
    s.add_argument("--fresh", default="mrfresh")
    s.set_defaults(fn=cmd_live_plan)
    s = sub.add_parser("exact", help="shape NAME paying one pinned input less its fee")
    s.add_argument("name")
    s.add_argument("--input", required=True)
    s.add_argument("--wallet", default="hemlock")
    s.add_argument("--to", default=None)
    s.add_argument("--priority", default="low")
    s.set_defaults(fn=cmd_exact)
    s = sub.add_parser("rebuild", help="shape NAME whose prepared build adds an input for its real fee")
    s.add_argument("name")
    s.add_argument("--account", type=int, required=True)
    s.add_argument("--wallet", default="hemlock")
    s.add_argument("--to", default=None)
    s.add_argument("--priority", default="low")
    s.set_defaults(fn=cmd_rebuild)
    s = sub.add_parser("refusal", help="integrated address with a second destination; no relay")
    s.add_argument("--shape", default="refusal")
    s.add_argument("--tag", default="1")
    s.set_defaults(fn=cmd_refusal)
    s = sub.add_parser("budget", help="relays in the ledger and what is left")
    s.set_defaults(fn=cmd_budget)
