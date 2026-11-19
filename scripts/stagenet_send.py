"""Send-side subcommands of stagenet_compat.py."""
import json
import os
import shutil
import time

import stagenet_lib as lib

RUN = lib.RUN
EVID = os.path.join(RUN, "evidence")
RINGDB = os.path.join(RUN, "ringdb")
LOOKAHEAD = {"lookahead_account": 49, "lookahead_subaddress": 199}
PRIORITIES = ["automatic", "low", "normal", "elevated", "priority"]
MAX_RELAYS = int(os.environ.get("MONERO_STAGENET_MAX_RELAYS", "25"))
MAX_SANE_FEE = 1_000_000_000  # 0.001 XMR


def evidence(name, data):
    os.makedirs(EVID, exist_ok=True)
    path = os.path.join(EVID, name + ".json")
    with open(path, "w") as f:
        json.dump(data, f, indent=1, sort_keys=True)
    return path


def load_evidence(name, default=None):
    path = os.path.join(EVID, name + ".json")
    if not os.path.exists(path):
        return default
    with open(path) as f:
        return json.load(f)


def open_wallet(label, daemon=None, ringdb=RINGDB, log_level=0):
    w = lib.WalletRpc(os.path.join(RUN, "w2", label), ringdb, daemon=daemon, log_level=log_level)
    w.call("open_wallet", {"filename": "w", "password": ""})
    w.call("auto_refresh", {"enable": False})
    return w


def mr_scan(label, name, rpc=None):
    request = {"op": "scan", "seed_file": lib.seed_file(label), "state": lib.state_file(label),
               "ring_db": lib.ring_db_file(label)}
    request.update(LOOKAHEAD)
    return lib.tool(request, name, rpc=rpc)


def mr_build(label, name, kind, recipients, account=0, inputs=None, priority="automatic", record=False,
             fresh=False, repeat=1, rpc=None, ignore_pending=False, tx_out=None, keep_blobs=None, state=None):
    assert not (record and state), "rings are only recorded in the wallet's own state file"
    request = {"op": "build", "seed_file": lib.seed_file(label), "state": state or lib.state_file(label),
               "kind": kind,
               "ring_db": lib.ring_db_file(label) if state is None else None,
               "recipients": recipients, "source_account": account, "priority": priority, "record": record,
               "fresh_rings": fresh, "repeat": repeat, "ignore_pending": ignore_pending}
    if inputs is not None:
        request["inputs"] = inputs
    if tx_out:
        request["tx_out"] = tx_out
    if keep_blobs is not None:
        request["keep_blobs"] = keep_blobs
    return lib.tool(request, name, rpc=rpc)


def state_outputs(label):
    with open(lib.state_file(label)) as f:
        return json.load(f)


def fee_tiers():
    return lib.daemon_json("get_fee_estimate", {"grace_blocks": 10})


def fee_tier_of(tx, tiers):
    mask = tiers["quantization_mask"]
    matches = []
    for index, rate in enumerate(tiers["fees"]):
        expect = -(-(tx["weight"] * rate) // mask) * mask
        if expect == tx["fee"]:
            matches.append(index + 1)
    return matches


def w2_transfer(w, shape, priority=0, relay=False):
    if shape["kind"] == "sweep":
        params = {"address": shape["recipients"][0][0], "account_index": shape.get("account", 0),
                  "priority": priority, "do_not_relay": not relay, "get_tx_hex": True, "get_tx_keys": True}
        r = w.raw("sweep_all", params)
        if "error" in r:
            return {"error": r["error"]}
        r = r["result"]
        return {"tx_hash": r["tx_hash_list"][0], "tx_hex": r["tx_blob_list"][0], "fee": r["fee_list"][0],
                "amount": r["amount_list"][0], "weight": r.get("weight_list", [None])[0],
                "n_txs": len(r["tx_hash_list"])}
    params = {"destinations": [{"address": a, "amount": v} for a, v in shape["recipients"]],
              "account_index": shape.get("account", 0), "priority": priority, "do_not_relay": not relay,
              "get_tx_hex": True, "get_tx_key": True}
    if shape.get("w2_subaddr_indices") is not None:
        params["subaddr_indices"] = shape["w2_subaddr_indices"]
    r = w.raw("transfer", params)
    if "error" in r:
        return {"error": r["error"]}
    r = r["result"]
    return {"tx_hash": r["tx_hash"], "tx_hex": r["tx_blob"], "fee": r["fee"], "amount": r["amount"],
            "weight": r.get("weight")}


def shapes():
    plan = load_evidence("shapes")
    assert plan, "write evidence/shapes.json first"
    return plan


def check_built(shape, build, tx, state_before, state_after, tiers):
    checks = {}
    n_dest = len(shape["recipients"])
    expected_outputs = 2 if shape["kind"] == "sweep" else n_dest + 1
    checks["version_2"] = tx["version"] == 2
    checks["unlock_time_0"] = tx["unlock_time"] == 0
    checks["rct_type_bulletproof_plus"] = tx.get("rct_type") == 6
    checks["output_count"] = len(tx["outputs"]) == expected_outputs
    checks["ring_size_16"] = all(len(i["offsets"]) == 16 for i in tx["inputs"])
    checks["fee_sane"] = 0 < tx["fee"] <= MAX_SANE_FEE and tx["fee"] == build["fee"]
    checks["fee_matches_a_daemon_tier"] = bool(fee_tier_of(tx, tiers))
    checks["no_trailing_bytes"] = tx.get("trailing_bytes") == 0
    checks["view_tags"] = all(o["view_tag"] is not None for o in tx["outputs"])
    by_ki_before = {o["key_image"]: o for o in state_before["outputs"]}
    by_ki_after = {o["key_image"]: o for o in state_after["outputs"]}
    selected = set(build["_selected_inputs"])
    further = set(build.get("_further_inputs", []))
    kis = [i["key_image"] for i in tx["inputs"]]
    spent = {"%s:%d" % (by_ki_before[k]["tx_hash"], by_ki_before[k]["output_index"]) for k in kis
             if k in by_ki_before}
    checks["inputs_are_selected_outputs"] = (
        len(spent) == len(kis) and selected <= spent and spent <= selected | further)
    checks["inputs_unspent_and_not_pending"] = all(
        k in by_ki_after and not by_ki_after[k]["spent"] and k not in state_after["pending_spends"] for k in kis)
    # The state file on disk must already hold exactly the rings in the blob.
    ring_ok = True
    for i in tx["inputs"]:
        stored = by_ki_after.get(i["key_image"], {}).get("ring")
        if not stored or stored["offsets"] != i["offsets"]:
            ring_ok = False
        elif i["indexes"][stored["real_index"]] != by_ki_after[i["key_image"]]["global_index"]:
            ring_ok = False
    checks["rings_saved_in_state_file_match_blob"] = ring_ok
    ring_db = lib.ring_db_file(shape["wallet"])
    if os.path.exists(ring_db):
        with open(ring_db) as f:
            rings = json.load(f).get("rings", {})
        checks["rings_in_ring_database_file_match_blob"] = all(
            rings.get(i["key_image"], {}).get("offsets") == i["offsets"] for i in tx["inputs"])
    else:
        checks["rings_in_ring_database_file_match_blob"] = False
    subaddresses = {tuple(by_ki_before[k]["subaddress_index"] or [0, 0]) for k in kis if k in by_ki_before}
    if len(kis) <= len(selected):
        checks["inputs_from_one_subaddress"] = len(subaddresses) == 1
    ledger = load_evidence("relayed", [])
    used = {k for entry in ledger for k in entry.get("key_images", [])}
    checks["inputs_not_in_an_earlier_relay"] = not (set(kis) & used)
    if shape.get("expect_zero_change"):
        checks["zero_change_with_dummy_output"] = not build["change_outputs"] and len(tx["outputs"]) == 2
        checks["fee_as_probed"] = tx["fee"] == shape.get("probe_fee")
    if shape.get("expect_rebuild"):
        checks["rebuilt_with_a_further_input"] = (selected == {shape["expect_rebuild"]} and len(kis) == 2
                                                  and spent <= selected | further)
    total_in = sum(by_ki_before[k]["amount"] for k in kis if k in by_ki_before)
    paid = sum(v for _, v in shape["recipients"])
    change = sum(c["amount"] for c in build["change_outputs"])
    if shape["kind"] == "sweep":
        checks["amounts_balance"] = change == 0 and total_in > tx["fee"]
    else:
        checks["amounts_balance"] = total_in == paid + change + tx["fee"]
        checks["change_to_source_account_index_0"] = all(
            (c["subaddress_index"] or [0, 0]) == [shape.get("account", 0), 0] for c in build["change_outputs"])
    return checks


def cmd_shape_build(args):
    plan = shapes()
    shape = plan[args.name]
    label = shape["wallet"]
    assert load_evidence("shape-%s-build" % args.name) is None or args.force, "already built"
    tiers = fee_tiers()
    scan = mr_scan(label, "scan-%s-pre-%s" % (label, args.name))
    state_before = state_outputs(label)
    tx_out = os.path.join(EVID, "tx-%s.json" % args.name)
    if args.input:
        shape = dict(shape, inputs=args.input)
    built = mr_build(label, "build-" + args.name, shape["kind"], shape["recipients"], shape.get("account", 0),
                     shape.get("inputs"), shape.get("priority", "automatic"), record=True, tx_out=tx_out,
                     rpc=args.rpc)
    if built.get("prepare_error") or not built["builds"]:
        print("monero-rust did not build:", built.get("prepare_error"), built.get("build_errors"))
        evidence("shape-%s-build-failed" % args.name, built)
        return
    build = built["builds"][0]
    build["_selected_inputs"] = built["selected_inputs"]
    build["_further_inputs"] = built.get("further_inputs", [])
    state_after = state_outputs(label)
    tx = lib.decode_tx(build["tx_blob"])
    checks = check_built(shape, build, tx, state_before, state_after, tiers)
    record = {"name": args.name, "shape": shape, "wallet_height": built["wallet_height"], "scan_height": scan["height"],
              "daemon_fee_estimate": tiers, "tx_id": build["tx_id"], "tx_hex": build["tx_blob"],
              "selected_inputs": built["selected_inputs"], "further_inputs": built.get("further_inputs", []),
              "inputs": build.get("inputs", built["selected_inputs"]),
              "rebuilt_with_further_inputs": len(build.get("inputs", [])) > len(built["selected_inputs"]),
              "input_total": build.get("input_total", built["input_total"]),
              "estimated_fee": built["estimated_fee"], "fee": build["fee"],
              "change_outputs": build["change_outputs"], "rings": build["rings"],
              "decoded": tx, "observer": lib.observer_view(tx), "fee_tier": fee_tier_of(tx, tiers),
              "checks": checks, "all_checks_pass": all(checks.values()), "built_at": time.time()}

    if not args.no_w2:
        w = open_wallet(label)
        try:
            w.call("refresh")
            w2 = []
            for _ in range(args.w2_repeat):
                r = w2_transfer(w, shape, priority=shape.get("w2_priority", 0))
                if "error" in r:
                    w2.append(r)
                    continue
                wtx = lib.decode_tx(r["tx_hex"])
                r["decoded"] = wtx
                r["observer"] = lib.observer_view(wtx)
                r["fee_tier"] = fee_tier_of(wtx, tiers)
                w2.append(r)
            record["wallet2"] = w2
        finally:
            w.stop(store=False)
        good = [r for r in w2 if "observer" in r]
        if good:
            a, b = record["observer"], good[0]["observer"]
            skip = {"key_images", "tx_pubkey", "size", "weight", "fee", "fee_per_byte"}
            record["observer_diff"] = {k: {"monero_rust": a[k], "wallet2": b[k]}
                                       for k in a if k not in skip and a[k] != b[k]}
            record["observer_numbers"] = {k: {"monero_rust": a[k], "wallet2": b[k]}
                                          for k in ("size", "weight", "fee", "fee_per_byte", "n_inputs")}
    path = evidence("shape-%s-build" % args.name, record)
    print(json.dumps({k: record[k] for k in ("tx_id", "selected_inputs", "fee", "fee_tier", "checks",
                                             "all_checks_pass", "observer")}, indent=1))
    print("observer_diff:", json.dumps(record.get("observer_diff"), indent=1))
    print("numbers:", json.dumps(record.get("observer_numbers")))
    print("evidence:", path)


def tx_status(txid):
    t = lib.get_tx(txid)
    if t is None:
        return {"known": False}
    return {"known": True, "in_pool": t.get("in_pool"), "block_height": t.get("block_height"),
            "double_spend_seen": t.get("double_spend_seen"), "confirmations": t.get("confirmations")}


def cmd_shape_relay(args):
    record = load_evidence("shape-%s-build" % args.name)
    assert record and record["all_checks_pass"], "shape is not built or failed its checks"
    ledger = load_evidence("relayed", [])
    assert len(ledger) < MAX_RELAYS, "relay budget used"
    assert not any(e["name"] == args.name for e in ledger), "already relayed"
    kis = record["observer"]["key_images"]
    used = {k for entry in ledger for k in entry.get("key_images", [])}
    assert not (set(kis) & used), "an input was already relayed in another transaction"
    before = tx_status(record["tx_id"])
    assert not before["known"], "transaction is already known to the node: %s" % before
    label = record["shape"]["wallet"]
    guarded = guard(kis)
    # Write the ledger entry first so an ambiguous result still blocks a second relay.
    entry = {"name": args.name, "tx_id": record["tx_id"], "key_images": kis, "by": "monero-rust",
             "guard": guarded, "started": time.time(), "result": "pending"}
    ledger.append(entry)
    evidence("relayed", ledger)
    try:
        result = lib.tool({"op": "relay", "state": lib.state_file(label), "ring_db": lib.ring_db_file(label),
                           "guard": True, "tx": os.path.join(EVID, "tx-%s.json" % args.name)},
                          "relay-" + args.name, rpc=args.rpc)
        entry["result"] = result["relay"]
        entry["guard_in_tool"] = result.get("guard")
    except Exception as exc:  # noqa: BLE001
        entry["result"] = "tool failure: %r" % exc
    time.sleep(1)
    entry["status_after"] = tx_status(record["tx_id"])
    entry["in_pool_hashes"] = record["tx_id"] in lib.pool_hashes()
    entry["finished"] = time.time()
    evidence("relayed", ledger)
    print(json.dumps(entry, indent=1))


def guard(key_images):
    """Refuse unless every key image is unspent and not in the pool of the node
    MONERO_STAGENET_GUARD_RPC names (else the run's node), which learns them."""
    r = lib.tool({"op": "guard", "key_images": list(key_images)}, "guard-%d" % int(time.time() * 1000),
                 rpc=os.environ.get("MONERO_STAGENET_GUARD_RPC"))
    if r.get("error") or not r.get("clear"):
        raise SystemExit("not relayed: an input is spent, in the pool, or the node could not say: %s"
                         % json.dumps(r))
    return r


def cmd_shape_status(args):
    ledger = load_evidence("relayed", [])
    height = lib.daemon_height()
    for entry in ledger:
        if args.name and entry["name"] != args.name:
            continue
        status = tx_status(entry["tx_id"])
        entry["status_latest"] = status
        print(entry["name"], entry["tx_id"], entry.get("by"), entry["result"], status, "daemon height", height)
    evidence("relayed", ledger)


def cmd_shape_verify(args):
    record = load_evidence("shape-%s-build" % args.name)
    shape = record["shape"]
    txid = record["tx_id"]
    status = tx_status(txid)
    out = {"name": args.name, "tx_id": txid, "status": status, "checks": {}}
    checks = out["checks"]
    checks["mined"] = bool(status.get("known") and not status.get("in_pool") and status.get("block_height"))
    plan = shapes()
    owners = plan.get("_address_owners", {})

    expected = {}
    for address, amount in shape["recipients"]:
        if shape["kind"] == "sweep":
            amount = record["input_total"] - record["fee"]
        owner = owners[address]
        expected.setdefault(owner["wallet"], {}).setdefault((owner["major"], owner["minor"]), 0)
        expected[owner["wallet"]][(owner["major"], owner["minor"])] += amount
    out["recipients"] = {}
    for wallet, per_address in expected.items():
        w = open_wallet(wallet)
        try:
            w.call("refresh")
            got = {}
            entries = []
            for major in sorted({m for m, _ in per_address}):
                r = w.raw("get_transfer_by_txid", {"txid": txid, "account_index": major})
                for t in r.get("result", {}).get("transfers", []):
                    if t["type"] in ("in", "pool"):
                        key = (t["subaddr_index"]["major"], t["subaddr_index"]["minor"])
                        got[key] = got.get(key, 0) + t["amount"]
                        entries.append({k: t.get(k) for k in ("type", "amount", "subaddr_index", "payment_id",
                                                              "height", "unlock_time", "confirmations", "locked")})
            out["recipients"][wallet] = {"expected": {"%d,%d" % k: v for k, v in per_address.items()},
                                         "credited": {"%d,%d" % k: v for k, v in got.items()},
                                         "entries": entries,
                                         "balance": w.call("get_balance", {"account_index": 0, "all_accounts": True})}
            checks["recipient_%s_exact" % wallet] = got == per_address
            if shape.get("payment_id"):
                checks["recipient_%s_payment_id" % wallet] = all(
                    e["payment_id"] == shape["payment_id"] for e in entries) and bool(entries)
        finally:
            w.stop()

    label = shape["wallet"]
    scan = mr_scan(label, "scan-%s-post-%s" % (label, args.name))
    mr_by_ki = {o["key_image"]: o for o in scan["outputs"]}
    kis = record["observer"]["key_images"]
    mr_inputs = [mr_by_ki.get(k) for k in kis]
    checks["mr_inputs_marked_spent_at_block"] = all(
        o and o["spent"] and o["spent_height"] == status.get("block_height") and o["spent_by_tx"] == txid
        for o in mr_inputs)
    mr_change = [o for o in scan["outputs"] if o["tx_hash"] == txid]
    out["mr_change"] = [{k: o[k] for k in ("amount", "subaddress_index", "block_height", "spent", "global_index",
                                           "output_index")} for o in mr_change]
    self_paid = sum(v for a, v in shape["recipients"] if owners[a]["wallet"] == label)
    if shape["kind"] != "sweep":
        expect_change = record["input_total"] - sum(v for _, v in shape["recipients"]) - record["fee"]
        checks["mr_change_amount"] = sum(o["amount"] for o in mr_change) == expect_change + self_paid

    if os.path.exists(os.path.join(RUN, "w2", label, "w.keys")):
        w = open_wallet(label)
        try:
            w.call("refresh")
            view = {"height": w.call("get_height")["height"]}
            r = w.raw("get_transfer_by_txid", {"txid": txid, "account_index": shape.get("account", 0)})
            transfers = r.get("result", {}).get("transfers", [])
            view["transfers"] = [{k: t.get(k) for k in ("type", "amount", "fee", "height", "subaddr_index",
                                                        "destinations", "payment_id", "unlock_time")}
                                 for t in transfers]
            outs = [t for t in transfers if t["type"] == "out"]
            if shape["kind"] == "sweep":
                checks["w2_sender_amount"] = bool(outs) and outs[0]["amount"] == record["input_total"] - record["fee"]
            else:
                checks["w2_sender_amount"] = bool(outs) and outs[0]["amount"] == sum(
                    v for a, v in shape["recipients"] if owners[a]["wallet"] != label)
            checks["w2_sender_fee"] = bool(outs) and outs[0]["fee"] == record["fee"]
            checks["w2_sender_height"] = bool(outs) and outs[0]["height"] == status.get("block_height")
            incoming = []
            accounts = w.call("get_accounts")["subaddress_accounts"]
            for account in accounts:
                incoming += w.call("incoming_transfers", {"transfer_type": "all",
                                                          "account_index": account["account_index"],
                                                          "verbose": True}).get("transfers", [])
            w2_by_ki = {t["key_image"]: t for t in incoming}
            checks["w2_inputs_marked_spent"] = all(k in w2_by_ki and w2_by_ki[k]["spent"] for k in kis)
            w2_change = [t for t in incoming if t["tx_hash"] == txid]
            view["change"] = [{k: t[k] for k in ("amount", "subaddr_index", "global_index", "block_height", "spent")}
                              for t in w2_change]
            checks["change_agrees"] = sorted(
                (t["amount"], t["subaddr_index"]["major"], t["subaddr_index"]["minor"], t["global_index"])
                for t in w2_change) == sorted(
                (o["amount"], (o["subaddress_index"] or [0, 0])[0], (o["subaddress_index"] or [0, 0])[1],
                 o["global_index"]) for o in mr_change)
            w2_unspent = sorted((t["tx_hash"], t["pubkey"], t["amount"]) for t in incoming if not t["spent"])
            mr_unspent = sorted((o["tx_hash"], o["key"], o["amount"]) for o in scan["outputs"] if not o["spent"])
            checks["unspent_sets_agree"] = w2_unspent == mr_unspent
            balance = w.call("get_balance", {"account_index": 0, "all_accounts": True})
            w2_bal, w2_unl, mr_bal, mr_unl = {}, {}, {}, {}
            for sub in balance.get("per_subaddress", []):
                w2_bal[sub["account_index"]] = w2_bal.get(sub["account_index"], 0) + sub["balance"]
                w2_unl[sub["account_index"]] = w2_unl.get(sub["account_index"], 0) + sub["unlocked_balance"]
            for o in scan["outputs"]:
                if o["spent"]:
                    continue
                major = (o["subaddress_index"] or [0, 0])[0]
                mr_bal[major] = mr_bal.get(major, 0) + o["amount"]
                if o["spendable"] and not o["pending_spend"]:
                    mr_unl[major] = mr_unl.get(major, 0) + o["amount"]
            view["balances"] = {"w2_height": view["height"], "mr_height": scan["height"],
                                "w2": {str(k): [w2_bal[k], w2_unl.get(k, 0)] for k in sorted(w2_bal)},
                                "mr": {str(k): [mr_bal[k], mr_unl.get(k, 0)] for k in sorted(mr_bal)}}
            checks["balances_agree"] = {k: v for k, v in w2_bal.items() if v} == {k: v for k, v in mr_bal.items() if v}
            if view["height"] == scan["height"]:
                checks["unlocked_balances_agree"] = (
                    {k: v for k, v in w2_unl.items() if v} == {k: v for k, v in mr_unl.items() if v})
            out["w2_sender"] = view
        finally:
            w.stop()
    out["all_checks_pass"] = all(checks.values())
    path = evidence("shape-%s-verify" % args.name, out)
    print(json.dumps(out, indent=1))
    print("evidence:", path)


def cmd_ring_reuse(args):
    record = load_evidence("shape-%s-build" % args.name)
    shape = record["shape"]
    label = shape["wallet"]
    status = tx_status(record["tx_id"])
    other = [[args.address, args.amount]]
    built = mr_build(label, "ring-reuse-%s-%s" % (args.name, args.tag), "send", other, shape.get("account", 0),
                     record.get("inputs", record["selected_inputs"]), ignore_pending=True, repeat=args.repeat,
                     keep_blobs=True)
    out = {"name": args.name, "relayed_tx": record["tx_id"], "status_at_rebuild": status,
           "relayed_rings": record["rings"], "rebuilds": [], "prepare_error": built.get("prepare_error"),
           "build_errors": built.get("build_errors")}
    for b in built.get("builds", []):
        tx = lib.decode_tx(b["tx_blob"])
        same = [r["indexes"] for r in b["rings"]] == [r["indexes"] for r in record["rings"]] and \
            [i["indexes"] for i in tx["inputs"]] == [i["indexes"] for i in record["decoded"]["inputs"]]
        out["rebuilds"].append({"tx_id": b["tx_id"], "different_tx": b["tx_id"] != record["tx_id"],
                                "rings": b["rings"], "rings_identical": same})
    out["all_identical"] = bool(out["rebuilds"]) and all(r["rings_identical"] for r in out["rebuilds"])
    path = evidence("ring-reuse-%s-%s" % (args.name, args.tag), out)
    print(json.dumps({k: out[k] for k in ("status_at_rebuild", "prepare_error", "build_errors", "all_identical")}))
    for r in out["rebuilds"]:
        print(r["tx_id"], "different tx:", r["different_tx"], "rings identical:", r["rings_identical"])
    print("evidence:", path)


def cmd_fees(args):
    plan = shapes()
    shape = plan[args.shape]
    label = shape["wallet"]
    tiers = fee_tiers()
    info = lib.daemon_other("get_info")
    out = {"daemon_fee_estimate": tiers, "daemon_height": info["height"],
           "block_weight_limit": info.get("block_weight_limit"), "tx_pool_size": info.get("tx_pool_size"),
           "shape": shape, "monero_rust": {}, "wallet2": {}}
    mr_scan(label, "scan-%s-pre-fees" % label)
    out["monero_rust_rates"] = lib.tool({"op": "fee", "wallet_height": state_outputs(label)["next_height"]},
                                        "fee-rates")
    for priority in PRIORITIES:
        built = mr_build(label, "fees-%s" % priority, shape["kind"], shape["recipients"], shape.get("account", 0),
                         shape.get("inputs"), priority, fresh=True, keep_blobs=True)
        if built.get("prepare_error") or not built.get("builds"):
            out["monero_rust"][priority] = {"error": built.get("prepare_error") or built.get("build_errors")}
            continue
        tx = lib.decode_tx(built["builds"][0]["tx_blob"])
        out["monero_rust"][priority] = {"fee": tx["fee"], "weight": tx["weight"], "size": tx["size"],
                                        "fee_per_byte": tx["fee_per_byte"], "tier": fee_tier_of(tx, tiers),
                                        "n_inputs": len(tx["inputs"]), "n_outputs": len(tx["outputs"]),
                                        "estimated_fee": built["estimated_fee"],
                                        "mask_multiple": tx["fee"] % tiers["quantization_mask"] == 0}
    w = open_wallet(label)
    try:
        w.call("refresh")
        for priority in range(5):
            r = w2_transfer(w, shape, priority=priority)
            if "error" in r:
                out["wallet2"][str(priority)] = r
                continue
            tx = lib.decode_tx(r["tx_hex"])
            out["wallet2"][str(priority)] = {"fee": tx["fee"], "weight": tx["weight"], "size": tx["size"],
                                             "rpc_weight": r["weight"], "fee_per_byte": tx["fee_per_byte"],
                                             "tier": fee_tier_of(tx, tiers), "n_inputs": len(tx["inputs"]),
                                             "n_outputs": len(tx["outputs"]),
                                             "mask_multiple": tx["fee"] % tiers["quantization_mask"] == 0}
    finally:
        w.stop(store=False)
    path = evidence("fees" + (("-" + args.tag) if args.tag else ""), out)
    print(json.dumps({k: out[k] for k in ("daemon_fee_estimate", "monero_rust", "wallet2")}, indent=1))
    print("evidence:", path)


def cmd_decoys_mr(args):
    plan = shapes()
    shape = plan[args.shape]
    label = shape["wallet"]
    path = os.path.join(EVID, "decoys-mr-%s.jsonl" % args.tag)
    state = None
    if args.snapshot:
        state = lib.state_file(label, "sample")
        if not os.path.exists(state):
            shutil.copy2(lib.state_file(label), state)
    done = 0
    batch_no = 0
    while done < args.n:
        count = min(args.batch, args.n - done)
        name = "decoys-mr-%s-%04d" % (args.tag, batch_no)
        started = time.time()
        height_before = lib.daemon_height()
        built = mr_build(label, name, shape["kind"], shape["recipients"], shape.get("account", 0),
                         shape.get("inputs"), shape.get("priority", "automatic"), fresh=True, repeat=count,
                         keep_blobs=False, rpc=args.rpc, state=state)
        height_after = lib.daemon_height()
        with open(path, "a") as f:
            for b in built.get("builds", []):
                f.write(json.dumps({"t": time.time(), "tx_id": b["tx_id"], "wallet_height": built["wallet_height"],
                                    "daemon_height_before": height_before, "daemon_height_after": height_after,
                                    "rings": b["rings"]}) + "\n")
        done += count
        batch_no += 1
        print("monero-rust rings: %d builds, %d errors, %.1fs" % (len(built.get("builds", [])),
                                                                 len(built.get("build_errors", [])),
                                                                 time.time() - started), flush=True)
        if built.get("prepare_error") or built.get("build_errors"):
            print("errors:", built.get("prepare_error"), built.get("build_errors")[:3])
        for sub in ("req", "mr"):
            p = os.path.join(RUN, sub, name + ".json")
            if os.path.exists(p) and batch_no > 1:
                os.remove(p)


def cmd_decoys_w2(args):
    plan = shapes()
    shape = plan[args.shape]
    label = shape["wallet"]
    path = os.path.join(EVID, "decoys-w2-%s.jsonl" % args.tag)
    work = os.path.join(RUN, "w2", "%s-sample-%s" % (label, args.tag))
    if not os.path.exists(work):
        os.makedirs(work)
        for name in ("w", "w.keys"):
            shutil.copy2(os.path.join(RUN, "w2", label, name), os.path.join(work, name))
    ring_root = os.path.join(RUN, "ringdb-samples", args.tag)
    done = 0
    while done < args.n:
        ringdb = os.path.join(ring_root, "%05d" % done)
        started = time.time()
        w = lib.WalletRpc(work, ringdb, daemon=args.daemon)
        try:
            w.call("open_wallet", {"filename": "w", "password": ""})
            w.call("auto_refresh", {"enable": False})
            builds = []
            for _ in range(args.per_db):
                height_before = lib.daemon_height()
                r = w2_transfer(w, shape, priority=shape.get("w2_priority", 0))
                height_after = lib.daemon_height()
                if "error" in r:
                    builds.append(r)
                    continue
                tx = lib.decode_tx(r["tx_hex"])
                builds.append({"tx_id": r["tx_hash"], "daemon_height_before": height_before,
                               "daemon_height_after": height_after,
                               "rings": [{"key_image": i["key_image"], "indexes": i["indexes"]}
                                         for i in tx["inputs"]]})
        finally:
            w.kill()
        shutil.rmtree(ringdb, ignore_errors=True)
        with open(path, "a") as f:
            for position, b in enumerate(builds):
                b["t"] = time.time()
                b["ringdb"] = done
                b["build_in_db"] = position
                f.write(json.dumps(b) + "\n")
        done += 1
        if done % 10 == 0 or done == args.n:
            print("wallet2 rings: %d/%d, last %.1fs" % (done, args.n, time.time() - started), flush=True)


def phase(label, name):
    lib.set_phase(label, name)


def cmd_capture(args):
    plan = shapes()
    shape = plan[args.shape]
    label = shape["wallet"]
    mr_url, w2_url = lib.proxy_url("mr"), lib.proxy_url("w2")
    w2_daemon = w2_url.replace("http://", "")
    tag = args.tag
    out = {"tag": tag, "shape": shape, "daemon_height": lib.daemon_height()}

    phase("w2", tag + ":open")
    w = open_wallet(label, daemon=w2_daemon)
    try:
        out["w2_height_before"] = w.call("get_height")["height"]
        phase("w2", tag + ":refresh")
        out["w2_refresh"] = w.call("refresh")
        phase("w2", tag + ":refresh-again")
        out["w2_refresh_again"] = w.call("refresh")
        phase("w2", tag + ":build-default")
        r = w2_transfer(w, shape, priority=0)
        out["w2_build"] = {k: r.get(k) for k in ("tx_hash", "fee", "error")}
        phase("w2", tag + ":build-default-again")
        w2_transfer(w, shape, priority=0)
        for priority in (1, 2, 3, 4):
            phase("w2", tag + ":build-priority-%d" % priority)
            w2_transfer(w, shape, priority=priority)
        if args.idle_seconds:
            phase("w2", tag + ":auto-refresh")
            w.call("auto_refresh", {"enable": True, "period": 10})
            time.sleep(args.idle_seconds)
            w.call("auto_refresh", {"enable": False})
        phase("w2", tag + ":close")
    finally:
        w.stop(store=False)
    phase("w2", tag + ":idle")

    phase("mr", tag + ":refresh")
    scan = mr_scan(label, "capture-%s-scan" % tag, rpc=mr_url)
    out["mr_scan"] = {k: scan[k] for k in ("start_height", "height", "blocks", "batches")}
    phase("mr", tag + ":refresh-again")
    mr_scan(label, "capture-%s-scan-again" % tag, rpc=mr_url)
    phase("mr", tag + ":mempool")
    request = {"op": "mempool", "seed_file": lib.seed_file(label)}
    request.update(LOOKAHEAD)
    out["mr_mempool"] = lib.tool(request, "capture-%s-mempool" % tag, rpc=mr_url)
    phase("mr", tag + ":fee-rates")
    lib.tool({"op": "fee", "wallet_height": scan["height"]}, "capture-%s-fee" % tag, rpc=mr_url)
    phase("mr", tag + ":build-default")
    built = mr_build(label, "capture-%s-build" % tag, shape["kind"], shape["recipients"], shape.get("account", 0),
                     shape.get("inputs"), "automatic", fresh=True, rpc=mr_url)
    out["mr_build"] = {"inputs": built.get("selected_inputs"), "errors": built.get("build_errors"),
                       "prepare_error": built.get("prepare_error")}
    for priority in ("low", "normal", "elevated", "priority"):
        phase("mr", tag + ":build-priority-" + priority)
        mr_build(label, "capture-%s-build-%s" % (tag, priority), shape["kind"], shape["recipients"],
                 shape.get("account", 0), shape.get("inputs"), priority, fresh=True, rpc=mr_url)
    if args.plain_scan_blocks:
        phase("mr", tag + ":plain-scan")
        state = lib.state_file(label, "capture-plain")
        if os.path.exists(state):
            os.remove(state)
        stop = out["daemon_height"] - 1500
        request = {"op": "scan", "seed_file": lib.seed_file(label), "state": state,
                   "restore_height": stop - args.plain_scan_blocks, "stop_height": stop}
        request.update(LOOKAHEAD)
        plain = lib.tool(request, "capture-%s-plain-scan" % tag, rpc=mr_url)
        out["mr_plain_scan"] = {k: plain[k] for k in ("start_height", "height", "blocks", "batches")}
    phase("mr", tag + ":idle")
    evidence("capture-" + tag, out)
    print(json.dumps(out, indent=1))


def cmd_capture_summary(args):
    summary = {}
    flagged = []
    headers = {}
    for label in ("mr", "w2"):
        for e in lib.read_log(lib.proxy_log(label)):
            name = lib.call_name(e)
            params = e.get("params")
            brief = {}
            if isinstance(params, dict):
                for key, value in params.items():
                    if key == "outputs_indexes":
                        continue
                    if key == "client" and value in ("", None):
                        continue
                    brief[key] = value
            row = {"call": name, "http": e["http"], "params": brief, "req_bytes": e["req_bytes"],
                   "resp_bytes": e["resp_bytes"]}
            kind = "bin" if name.endswith(".bin") else ("json_rpc" if e["path"] == "json_rpc" else "other")
            headers.setdefault(label, {}).setdefault(kind, set()).add(
                (tuple(e.get("headers") or ()), e.get("user_agent")))
            if "resp" in e and name == "getblocks.bin":
                row["resp"] = e["resp"]
            summary.setdefault(label, {}).setdefault(e["phase"], []).append(row)
            if name in ("get_transactions", "gettransactions", "is_key_image_spent", "get_o_indexes.bin",
                        "get_outs", "get_transaction_pool_hashes.bin") or (
                    name == "get_outs.bin" and isinstance(params, dict) and params.get("outputs_count", 99) < 20):
                flagged.append({"label": label, "phase": e["phase"], "call": name, "params": brief})
    counts = {label: {ph: {} for ph in phases} for label, phases in summary.items()}
    for label, phases in summary.items():
        for ph, rows in phases.items():
            for row in rows:
                counts[label][ph][row["call"]] = counts[label][ph].get(row["call"], 0) + 1
    header_sets = {label: {kind: [{"headers": list(h), "user_agent": ua} for h, ua in sorted(v, key=str)]
                           for kind, v in kinds.items()} for label, kinds in headers.items()}
    path = evidence("capture-summary", {"sequences": summary, "counts": counts, "flagged": flagged,
                                        "request_headers": header_sets})
    print("request headers:", json.dumps(header_sets, indent=1))
    for label in ("mr", "w2"):
        for ph, rows in summary.get(label, {}).items():
            if args.phase and args.phase not in ph:
                continue
            print("==", label, ph)
            for row in rows:
                print("   %-34s %s  req=%d resp=%d %s" % (row["call"], json.dumps(row["params"])[:230],
                                                          row["req_bytes"], row["resp_bytes"],
                                                          json.dumps(row.get("resp")) if row.get("resp") else ""))
    print("flagged:", json.dumps(flagged, indent=1)[:4000])
    print("evidence:", path)


def cmd_plan(args):
    sender, other = args.sender, args.other
    rcpt = load_evidence("wallet-" + args.recipient)
    fresh = load_evidence("wallet-" + args.fresh)
    assert rcpt and fresh, "create the recipient wallets with new-wallet first"
    sender_address = lib.tool({"op": "addr", "seed_file": lib.seed_file(sender), "subaddresses": []},
                              "addr-" + sender)["primary"]
    other_address = lib.tool({"op": "addr", "seed_file": lib.seed_file(other), "subaddresses": []},
                             "addr-" + other)["primary"]
    state = state_outputs(sender)
    unspent = [o for o in state["outputs"] if not o["spent"]]
    balance = {}
    for o in unspent:
        account = (o["subaddress_index"] or [0, 0])[0]
        balance[account] = balance.get(account, 0) + o["amount"]
    owners = {
        rcpt["primary"]: {"wallet": args.recipient, "major": 0, "minor": 0},
        rcpt["sub_0_1"]: {"wallet": args.recipient, "major": 0, "minor": 1},
        rcpt["sub_0_2"]: {"wallet": args.recipient, "major": 0, "minor": 2},
        rcpt["sub_1_0"]: {"wallet": args.recipient, "major": 1, "minor": 0},
        rcpt["integrated"]: {"wallet": args.recipient, "major": 0, "minor": 0},
        fresh["primary"]: {"wallet": args.fresh, "major": 0, "minor": 0},
        fresh["sub_0_1"]: {"wallet": args.fresh, "major": 0, "minor": 1},
        other_address: {"wallet": other, "major": 0, "minor": 0},
        sender_address: {"wallet": sender, "major": 0, "minor": 0},
    }
    xmr = 10 ** 12
    plan = load_evidence("shapes", {})
    plan.update({
        "_address_owners": owners,
        "_sender_unspent": [{"key": "%s:%d" % (o["tx_hash"], o["output_index"]), "amount": o["amount"],
                             "subaddress": o["subaddress_index"], "height": o["block_height"],
                             "global_index": o["global_index"]} for o in unspent],
        "probe1": {"wallet": sender, "kind": "send", "account": 0, "recipients": [[rcpt["primary"], xmr // 100]],
                   "intent": "one small payment, used for fees and request capture only"},
        "acct1": {"wallet": sender, "kind": "send", "account": 1, "recipients": [[rcpt["primary"], 2 * xmr // 100]],
                  "intent": "spend from account 1, standard recipient; change returns to (1,0)"},
        "standard": {"wallet": sender, "kind": "send", "account": 0,
                     "recipients": [[rcpt["primary"], 5 * xmr // 100]], "intent": "standard recipient"},
        "subaddr": {"wallet": sender, "kind": "send", "account": 0,
                    "recipients": [[rcpt["sub_0_1"], 5 * xmr // 100]], "intent": "one subaddress recipient"},
        "multiout": {"wallet": sender, "kind": "send", "account": 0,
                     "recipients": [[rcpt["primary"], 2 * xmr // 100], [rcpt["sub_1_0"], 2 * xmr // 100],
                                    [fresh["primary"], 3 * xmr // 100], [fresh["sub_0_1"], 2 * xmr // 100]],
                     "intent": "four destinations plus change; also funds the fresh wallet twice"},
        "integrated": {"wallet": sender, "kind": "send", "account": 0,
                       "recipients": [[rcpt["integrated"], xmr // 100]],
                       "payment_id": rcpt["integrated_payment_id"], "intent": "integrated address recipient"},
        "submix": {"wallet": sender, "kind": "send", "account": 0,
                   "recipients": [[rcpt["sub_0_2"], 3 * xmr // 100], [rcpt["primary"], 4 * xmr // 100]],
                   "intent": "subaddress plus standard recipients"},
        "sweep": {"wallet": args.fresh, "kind": "sweep", "account": 0, "recipients": [[rcpt["primary"], 0]],
                  "intent": "zero-change sweep of the fresh wallet, after `multiout` has unlocked"},
    })
    if balance.get(0, 0) > xmr // 5:
        plan["decoys"] = {"wallet": sender, "kind": "send", "account": 0,
                          "recipients": [[other_address, balance[0] - xmr // 20]],
                          "intent": "ring sampling only"}
    path = evidence("shapes", plan)
    print(json.dumps({k: v for k, v in plan.items() if k != "_address_owners"}, indent=1))
    print("evidence:", path)


def cmd_sweep_back(args):
    ledger = load_evidence("relayed", [])
    tiers = fee_tiers()
    use_proxy = not args.direct
    daemon = lib.proxy_url("w2").replace("http://", "") if use_proxy else None
    if use_proxy:
        lib.set_phase("w2", "sweepback-%s:open" % args.name)
    w = open_wallet(args.wallet, daemon=daemon)
    out = {"name": args.name, "wallet": args.wallet, "account": args.account, "to": args.address, "txs": []}
    try:
        if use_proxy:
            lib.set_phase("w2", "sweepback-%s:refresh" % args.name)
        w.call("refresh")
        out["balance_before"] = w.call("get_balance", {"account_index": args.account})
        if use_proxy:
            lib.set_phase("w2", "sweepback-%s:build" % args.name)
        r = w.raw("sweep_all", {"address": args.address, "account_index": args.account, "subaddr_indices_all": True,
                                "priority": 0, "do_not_relay": True, "get_tx_hex": True, "get_tx_metadata": True})
        if "error" in r:
            out["error"] = r["error"]
            print(json.dumps(out, indent=1))
            evidence("sweep-back-" + args.name, out)
            return
        r = r["result"]
        for position, txid in enumerate(r["tx_hash_list"]):
            tx = lib.decode_tx(r["tx_blob_list"][position])
            checks = {"version_2": tx["version"] == 2, "unlock_time_0": tx["unlock_time"] == 0,
                      "two_outputs": len(tx["outputs"]) == 2,
                      "ring_size_16": all(len(i["offsets"]) == 16 for i in tx["inputs"]),
                      "fee_sane": 0 < tx["fee"] <= MAX_SANE_FEE and tx["fee"] == r["fee_list"][position],
                      "budget": len(ledger) < MAX_RELAYS}
            entry = {"tx_id": txid, "amount": r["amount_list"][position], "fee": r["fee_list"][position],
                     "tx_hex": r["tx_blob_list"][position], "observer": lib.observer_view(tx),
                     "fee_tier": fee_tier_of(tx, tiers), "checks": checks}
            out["txs"].append(entry)
            if not all(checks.values()):
                entry["relay"] = "not relayed: a check failed"
                continue
            entry["guard"] = guard(entry["observer"]["key_images"])
            record = {"name": "%s-%d" % (args.name, position), "tx_id": txid, "by": "wallet2",
                      "key_images": entry["observer"]["key_images"], "guard": entry["guard"],
                      "started": time.time(), "result": "pending"}
            ledger.append(record)
            evidence("relayed", ledger)
            if use_proxy:
                lib.set_phase("w2", "sweepback-%s:relay" % args.name)
            relayed = w.raw("relay_tx", {"hex": r["tx_metadata_list"][position]})
            record["result"] = "OK" if "result" in relayed else json.dumps(relayed.get("error"))
            time.sleep(1)
            record["status_after"] = tx_status(txid)
            record["finished"] = time.time()
            entry["relay"] = record["result"]
            entry["status_after"] = record["status_after"]
            evidence("relayed", ledger)
        if use_proxy:
            lib.set_phase("w2", "sweepback-%s:after" % args.name)
        out["balance_after"] = w.call("get_balance", {"account_index": args.account})
    finally:
        w.stop()
        if use_proxy:
            lib.set_phase("w2", "idle")
    evidence("sweep-back-" + args.name, out)
    for entry in out["txs"]:
        print(entry["tx_id"], "amount", entry["amount"], "fee", entry["fee"], "inputs", entry["observer"]["n_inputs"],
              entry["checks"], entry.get("relay"), entry.get("status_after"))


def register(sub):
    s = sub.add_parser("plan", help="write evidence/shapes.json")
    s.add_argument("--sender", default="hemlock")
    s.add_argument("--other", default="honked", help="funded wallet used only as an address for unrelayed builds")
    s.add_argument("--recipient", default="rcpt")
    s.add_argument("--fresh", default="mrfresh")
    s.set_defaults(fn=cmd_plan)
    s = sub.add_parser("shape-build", help="crate build with rings saved, checks, and wallet2's build; no relay")
    s.add_argument("name")
    s.add_argument("--input", action="append", help="spend this output (txid:index); repeatable")
    s.add_argument("--rpc", default=None, help="daemon URL for the crate, for example a logging proxy")
    s.add_argument("--no-w2", action="store_true")
    s.add_argument("--w2-repeat", type=int, default=1)
    s.add_argument("--force", action="store_true")
    s.set_defaults(fn=cmd_shape_build)
    s = sub.add_parser("shape-relay", help="relay a built shape through the crate")
    s.add_argument("name")
    s.add_argument("--rpc", default=None)
    s.set_defaults(fn=cmd_shape_relay)
    s = sub.add_parser("shape-status", help="pool and chain status of relayed transactions")
    s.add_argument("name", nargs="?")
    s.set_defaults(fn=cmd_shape_status)
    s = sub.add_parser("shape-verify", help="recipient and sender wallet2 views against the crate's state")
    s.add_argument("name")
    s.set_defaults(fn=cmd_shape_verify)
    s = sub.add_parser("ring-reuse", help="rebuild a relayed shape's inputs from saved state and compare rings")
    s.add_argument("name")
    s.add_argument("address")
    s.add_argument("amount", type=int)
    s.add_argument("--tag", default="pool")
    s.add_argument("--repeat", type=int, default=3)
    s.set_defaults(fn=cmd_ring_reuse)
    s = sub.add_parser("sweep-back", help="wallet2 builds and relays a sweep of a recipient account")
    s.add_argument("name")
    s.add_argument("wallet")
    s.add_argument("account", type=int)
    s.add_argument("address")
    s.add_argument("--direct", action="store_true", help="do not go through the wallet2 logging proxy")
    s.set_defaults(fn=cmd_sweep_back)
    s = sub.add_parser("fees", help="fee of each implementation at each priority; no relay")
    s.add_argument("shape")
    s.add_argument("--tag", default="")
    s.set_defaults(fn=cmd_fees)
    s = sub.add_parser("decoys-mr", help="complete rings from unrelayed crate builds")
    s.add_argument("shape")
    s.add_argument("n", type=int)
    s.add_argument("--batch", type=int, default=4, help="builds per recorded chain tip")
    s.add_argument("--tag", default="a")
    s.add_argument("--snapshot", action="store_true", help="build from a copy of the state file")
    s.add_argument("--rpc", default=None)
    s.set_defaults(fn=cmd_decoys_mr)
    s = sub.add_parser("decoys-w2", help="complete rings from unrelayed wallet2 builds, one ring database each")
    s.add_argument("shape")
    s.add_argument("n", type=int)
    s.add_argument("--per-db", type=int, default=1)
    s.add_argument("--tag", default="a")
    s.add_argument("--daemon", default=None)
    s.set_defaults(fn=cmd_decoys_w2)
    s = sub.add_parser("capture", help="drive both implementations through the logging proxies")
    s.add_argument("shape")
    s.add_argument("--tag", default="c1")
    s.add_argument("--idle-seconds", type=int, default=0, help="also log wallet2's auto refresh for this long")
    s.add_argument("--plain-scan-blocks", type=int, default=4500,
                   help="also log a crate catch-up scan over this many recent blocks (0 to skip)")
    s.set_defaults(fn=cmd_capture)
    s = sub.add_parser("capture-summary", help="collapse the proxy logs into call sequences")
    s.add_argument("--phase", default="")
    s.set_defaults(fn=cmd_capture_summary)
