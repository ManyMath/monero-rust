"""View-only wallet and cold-signing subcommands of stagenet_compat.py."""
import json
import os
import re
import shutil
import subprocess
import time

import stagenet_lib as lib
import stagenet_send as send
from stagenet_send import evidence, load_evidence, open_wallet

RUN = lib.RUN
EVID = os.path.join(RUN, "evidence")
RINGDB = os.path.join(RUN, "ringdb")
MAX_RELAYS = 6


def wallet_cli(label, command, stdin="\n\n", timeout=3600):
    binary = os.environ.get("MONERO_WALLET_CLI") or os.path.join(os.path.dirname(lib.WALLET_RPC),
                                                                 "monero-wallet-cli")
    wdir = os.path.join(RUN, "w2", label)
    cmd = [binary, "--" + lib.NETWORK, "--wallet-file", os.path.join(wdir, "w"), "--password", "",
           "--log-file", os.path.join(wdir, "wallet-cli.log"),
           "--daemon-address", lib.rpc_url().replace("http://", ""), "--trusted-daemon",
           "--shared-ringdb-dir", RINGDB] + command
    out = subprocess.run(cmd, input=stdin, capture_output=True, text=True, timeout=timeout, cwd=wdir)
    return {"rc": out.returncode, "stdout": out.stdout[-3000:], "stderr": out.stderr[-1500:]}


def copy_wallet(source, target):
    src, dst = os.path.join(RUN, "w2", source), os.path.join(RUN, "w2", target)
    if os.path.exists(dst):
        shutil.rmtree(dst)
    os.makedirs(dst)
    for name in ("w", "w.keys"):
        shutil.copy2(os.path.join(src, name), os.path.join(dst, name))


def w2_outputs(w):
    out = []
    for account in w.call("get_accounts")["subaddress_accounts"]:
        out += w.call("incoming_transfers", {"transfer_type": "all", "account_index": account["account_index"],
                                             "verbose": True}).get("transfers", [])
    return out


def w2_summary(w):
    balance = w.call("get_balance", {"account_index": 0, "all_accounts": True})
    outputs = w2_outputs(w)
    return {"height": w.call("get_height")["height"], "balance": balance["balance"],
            "unlocked_balance": balance["unlocked_balance"], "outputs": outputs,
            "spent_count": sum(1 for t in outputs if t["spent"]),
            "unspent_count": sum(1 for t in outputs if not t["spent"]),
            "spent_amount": sum(t["amount"] for t in outputs if t["spent"]),
            "unspent_amount": sum(t["amount"] for t in outputs if not t["spent"])}


def restore_height_of(label):
    info = load_evidence("wallet-%s-vo" % label)
    assert info, "no wallet2 view-only wallet for %s (run w2-viewonly first)" % label
    return info["restore_height"]


def cmd_vo_scan(args):
    tag = args.state_tag
    prefix = (tag + "-" if tag else "")
    if args.crate_default:
        window = {"lookahead_default": True}
    elif args.default_lookahead:
        window = {"lookahead_account": 0, "lookahead_subaddress": 20}
    else:
        window = send.LOOKAHEAD
    wallets_all = []
    for label in args.labels:
        name = label + "-vo"
        start = restore_height_of(label)
        wallets_all.append({"label": name, "seed_file": lib.seed_file(name), "state": lib.state_file(name, tag),
                            "restore_height": start,
                            "response": os.path.join(RUN, "mr", "%s%s-scan.json" % (prefix, name))})
        wallets_all[-1].update(window)
        if args.twin_default:
            twin_tag = tag + "dl"
            wallets_all.append({"label": name + "@dl", "seed_file": lib.seed_file(name),
                                "state": lib.state_file(name, twin_tag),
                                "restore_height": max(start, args.twin_from),
                                "lookahead_account": 0, "lookahead_subaddress": 20,
                                "response": os.path.join(RUN, "mr", "%s-%s-scan.json" % (twin_tag, name))})
    heights = sorted({w["restore_height"] for w in wallets_all})
    phases = []
    for i, start in enumerate(heights):
        stop = heights[i + 1] if i + 1 < len(heights) else args.stop_height
        if start < args.skip_below:
            continue
        active = [w for w in wallets_all if w["restore_height"] <= start]
        for w in active:
            if w["restore_height"] == start:
                assert not os.path.exists(w["state"]), "%s exists; scan with `scan` to continue it" % w["state"]
            else:
                assert os.path.exists(w["state"]), "%s missing: an earlier phase was not run" % w["state"]
        request = {"op": "scan", "wallets": active, "keep_batches": args.keep_batches}
        request.update(window)
        if args.prune is not None:
            request["prune"] = args.prune
        if stop is not None:
            request["stop_height"] = stop
        started = time.time()
        r = lib.tool(request, "vo-scan-%s%d" % (prefix, start))
        r.update({"phase_start": start, "stop_height": stop, "wallets_active": [w["label"] for w in active],
                  "wall_seconds": time.time() - started})
        if stop is not None:
            assert all(w["height"] == stop for w in r["wallets"].values()), "phase did not stop at %d" % stop
        phases.append(r)
        print(json.dumps(r), flush=True)
    evidence("vo-scan-%s%s-from%d" % (prefix, "-".join(args.labels), args.skip_below), phases)


def cmd_meter(args):
    if args.set:
        lib.set_phase("meter", args.set)
        print("phase", args.set)
        return
    per_phase, per_route, total = {}, {}, 0
    with open(lib.proxy_log("meter")) as f:
        for line in f:
            e = json.loads(line)
            size = e["req_bytes"] + e["resp_bytes"]
            total += size
            route = e.get("method") or e["path"]
            p = per_phase.setdefault(e["phase"], {"requests": 0, "bytes": 0})
            p["requests"] += 1
            p["bytes"] += size
            r = per_route.setdefault(route, {"requests": 0, "bytes": 0})
            r["requests"] += 1
            r["bytes"] += size
    out = {"total_bytes": total, "per_phase": per_phase,
           "per_route": dict(sorted(per_route.items(), key=lambda kv: -kv[1]["bytes"]))}
    evidence("traffic", out)
    print(json.dumps(out, indent=1))


def cmd_vo_check(args):
    import stagenet_compat as compat
    tag = args.state_tag
    prefix = (tag + "-" if tag else "")
    out = {}
    for label in args.labels:
        name = label + "-vo"
        for _ in range(4):
            request = {"op": "scan", "seed_file": lib.seed_file(name), "state": lib.state_file(name, tag)}
            if args.default_lookahead:
                request.update({"lookahead_account": 0, "lookahead_subaddress": 20})
            else:
                request.update(send.LOOKAHEAD)
            scan = lib.tool(request, "vo-check-%s%s" % (prefix, name))
            view = compat.export_wallet2(name, prefix + args.tag)
            full = compat.export_wallet2(label, prefix + args.tag)
            if scan["height"] == view["height"] == full["height"]:
                break
        else:
            raise SystemExit("could not bring %s to one height" % label)
        diff = compat.scan_diff(view, scan)
        unlock = {}
        for t in view["transfers"].get("in", []):
            unlock.setdefault(t["txid"], set()).add(t["unlock_time"])
        unlock_diffs, unlock_checked = [], 0
        for o in scan["outputs"]:
            if o["tx_hash"] in unlock:
                unlock_checked += 1
                if unlock[o["tx_hash"]] != {o["unlock_time"]}:
                    unlock_diffs.append({"tx_hash": o["tx_hash"], "w2": sorted(unlock[o["tx_hash"]]),
                                         "mr": o["unlock_time"]})
        w2_out = [t for v in view["incoming"].values() for t in v]
        full_out = [t for v in full["incoming"].values() for t in v]
        row = {
            "height": scan["height"],
            "outputs": [diff["w2_outputs"], diff["mr_outputs"]],
            "only_w2": len(diff["only_wallet2"]), "only_mr": len(diff["only_monero_rust"]),
            "field_diffs": len(diff["field_diffs"]), "field_diff_examples": diff["field_diffs"][:5],
            "duplicate_keys": [len(diff["w2_duplicate_keys"]), len(diff["mr_duplicate_keys"])],
            "crate_duplicate_one_time_keys_seen_in_scan": len(scan.get("duplicate_one_time_keys", [])),
            "unlock_time": {"checked": unlock_checked, "diffs": len(unlock_diffs), "examples": unlock_diffs[:5],
                            "nonzero": sum(1 for o in scan["outputs"] if o["unlock_time"])},
            "balances_per_account": diff["balances"],
            "w2_viewonly": {"balance": view["balance"]["balance"],
                            "unlocked_balance": view["balance"]["unlocked_balance"],
                            "outputs_marked_spent": sum(1 for t in w2_out if t["spent"]),
                            "outputs_with_key_image": sum(1 for t in w2_out if t["key_image"]),
                            "total_received": sum(t["amount"] for t in w2_out),
                            "out_transfers": len(view["transfers"].get("out", []))},
            "crate_viewonly": {"balance_at_height": scan["balance"],
                               "unspent_sum": sum(o["amount"] for o in scan["outputs"] if not o["spent"]),
                               "spendable_sum": sum(o["amount"] for o in scan["outputs"]
                                                    if not o["spent"] and o["spendable"]),
                               "outputs_marked_spent": sum(1 for o in scan["outputs"] if o["spent"]),
                               "outputs_with_key_image": sum(1 for o in scan["outputs"] if o["key_image"]),
                               "total_received": sum(o["amount"] for o in scan["outputs"])},
            "w2_full": {"balance": full["balance"]["balance"], "unlocked_balance": full["balance"]["unlocked_balance"],
                        "outputs": len(full_out), "spent_outputs": sum(1 for t in full_out if t["spent"]),
                        "out_transfers": len(full["transfers"].get("out", []))},
        }
        evidence("vo-check-%s%s-%s" % (prefix, args.tag, label), {"summary": row, "diff": diff})
        out[label] = row
        print(label, json.dumps(row, indent=1), flush=True)
    evidence("vo-check-%s%s" % (prefix, args.tag), out)


def brief(summary):
    return {k: v for k, v in summary.items() if k != "outputs"}


def compare_outputs(reference, other, other_is_crate):
    ref = {(t["tx_hash"], t["pubkey"]): t for t in reference}
    if other_is_crate:
        oth = {(o["tx_hash"], o["key"]): o for o in other}
    else:
        oth = {(t["tx_hash"], t["pubkey"]): t for t in other}
    spent_diff = [list(k) for k in ref if k in oth and ref[k]["spent"] != oth[k]["spent"]]
    image_diff = [list(k) for k in ref if k in oth and ref[k]["key_image"] != oth[k]["key_image"]]
    return {"reference_outputs": len(ref), "other_outputs": len(oth),
            "only_reference": len(set(ref) - set(oth)), "only_other": len(set(oth) - set(ref)),
            "spent_flag_differences": len(spent_diff), "key_image_differences": len(image_diff),
            "spent_flag_examples": spent_diff[:5], "key_image_examples": image_diff[:5]}


STATUS_CHUNK = 0


def crate_ki_import(label, tag, name, file, fmt, pristine, mark_spent=True):
    state = lib.state_file(label + "-vo", (tag + "-" if tag else "") + "ki-" + name)
    shutil.copy2(pristine, state)
    return lib.tool({"op": "ki-import", "seed_file": lib.viewonly_seed_file(label), "state": state, "file": file,
                     "format": fmt, "mark_spent": mark_spent, "status_chunk": STATUS_CHUNK},
                    "ki-import-%s-%s-%s" % (tag or "a", label, name))


def w2_import_rpc(w, offset, signed, chunk):
    if not chunk:
        return [w.raw("import_key_images", {"offset": offset, "signed_key_images": signed}, timeout=7200)]
    results = []
    for start in range(0, len(signed), chunk):
        results.append(w.raw("import_key_images", {"offset": offset + start,
                                                   "signed_key_images": signed[start:start + chunk]},
                             timeout=7200))
    return results


def cli_import_totals(stdout):
    m = re.search(r"imported to height (\d+), ([0-9.]+) spent, ([0-9.]+) unspent", stdout)
    if not m:
        return None
    return {"height": int(m.group(1)), "spent": atomic(m.group(2)), "unspent": atomic(m.group(3))}


def atomic(text):
    whole, _, frac = text.partition(".")
    return int(whole) * 10 ** 12 + int((frac + "0" * 12)[:12])


def sync_label(label, tag, prefix, tries=4):
    for _ in range(tries):
        scans = {}
        for name in (label, label + "-vo"):
            request = {"op": "scan", "seed_file": lib.seed_file(name), "state": lib.state_file(name, tag)}
            request.update(send.LOOKAHEAD)
            scans[name] = lib.tool(request, "ki-%ssync-%s" % (prefix, name))
        w = open_wallet(label)
        try:
            w.call("refresh")
            full = w2_summary(w)
            started = time.time()
            exported = w.call("export_key_images", {"all": True}, timeout=7200)
            export_seconds = time.time() - started
        finally:
            w.stop()
        w = open_wallet(label + "-vo")
        try:
            w.call("refresh")
            pristine_w2 = w2_summary(w)
        finally:
            w.stop()
        heights = {full["height"], pristine_w2["height"]} | {s["height"] for s in scans.values()}
        if len(heights) == 1 and lib.daemon_height() == full["height"]:
            return scans, full, exported, export_seconds, pristine_w2
    raise SystemExit("could not bring the four wallets to one height")


def w2_vo_import_copy(label, copy, action):
    copy_wallet(label + "-vo", copy)
    try:
        return action(copy)
    finally:
        shutil.rmtree(os.path.join(RUN, "w2", copy), ignore_errors=True)


def cmd_ki_roundtrip(args):
    global STATUS_CHUNK
    STATUS_CHUNK = args.status_chunk
    label, tag = args.label, args.state_tag
    prefix = (tag + "-" if tag else "")
    out = {"label": label, "state_tag": tag, "tool": lib.TOOL, "status_chunk": args.status_chunk,
           "w2_chunk": args.w2_chunk}
    scans, full, exported, export_seconds, pristine_w2 = sync_label(label, tag, prefix)
    height = full["height"]
    out["height"] = height
    out["w2_full"] = brief(full)
    out["w2_export_seconds"] = export_seconds
    out["w2_viewonly_before_import"] = brief(pristine_w2)
    crate_vo = scans[label + "-vo"]
    out["crate_viewonly_before_import"] = {
        "outputs": len(crate_vo["outputs"]), "balance": crate_vo["balance"],
        "with_key_image": sum(1 for o in crate_vo["outputs"] if o["key_image"]),
        "spent": sum(1 for o in crate_vo["outputs"] if o["spent"]),
        "total_received": sum(o["amount"] for o in crate_vo["outputs"])}
    out["crate_full"] = {"outputs": len(scans[label]["outputs"]), "balance": scans[label]["balance"],
                         "spent": sum(1 for o in scans[label]["outputs"] if o["spent"])}
    out["before_import_vs_w2_full"] = {
        "w2_viewonly": compare_outputs(full["outputs"], pristine_w2["outputs"], False),
        "crate_viewonly": compare_outputs(full["outputs"], crate_vo["outputs"], True),
        "crate_full": compare_outputs(full["outputs"], scans[label]["outputs"], True)}

    rpc_export = os.path.join(EVID, "ki-%sw2-%s.json" % (prefix, label))
    with open(rpc_export, "w") as f:
        json.dump({"offset": exported.get("offset", 0), "signed_key_images": exported["signed_key_images"]}, f)
    out["w2_export"] = {"offset": exported.get("offset", 0), "count": len(exported["signed_key_images"])}
    file_export = os.path.join(EVID, "ki-%sw2-%s.bin" % (prefix, label))
    if os.path.exists(file_export):
        os.remove(file_export)
    out["w2_cli_export"] = wallet_cli(label, ["export_key_images", "all", file_export])
    out["w2_cli_export"]["bytes"] = os.path.getsize(file_export) if os.path.exists(file_export) else None
    crate_export = os.path.join(EVID, "ki-%smr-%s.bin" % (prefix, label))
    started = time.time()
    out["crate_export"] = lib.tool({"op": "ki-export", "seed_file": lib.seed_file(label),
                                    "state": lib.state_file(label, tag), "out": crate_export},
                                   "ki-%sexport-%s" % (prefix, label))
    out["crate_export"]["wall_seconds"] = time.time() - started

    reads = {}
    for name, path, fmt in (("w2-rpc", rpc_export, "rpc-json"), ("w2-file", file_export, "signed"),
                            ("crate-file", crate_export, "signed")):
        reads[name] = lib.tool({"op": "ki-read", "seed_file": lib.seed_file(label), "file": path, "format": fmt},
                               "ki-%sread-%s-%s" % (prefix, label, name))
    w2_order = reads["w2-rpc"].get("key_images") or []
    out["export_contents"] = {
        name: {"ok": r.get("ok"), "error": r.get("error"), "offset": r.get("offset"), "count": r.get("count"),
               "same_list_as_w2_rpc": r.get("key_images") == w2_order,
               "same_set_as_w2_rpc": set(r.get("key_images") or []) == set(w2_order),
               "first_difference": next((i for i, (a, b) in enumerate(zip(r.get("key_images") or [], w2_order))
                                         if a != b), None)}
        for name, r in reads.items()}

    def w2_rpc_import(copy):
        w = open_wallet(copy)
        try:
            started = time.time()
            results = w2_import_rpc(w, out["w2_export"]["offset"], exported["signed_key_images"], args.w2_chunk)
            seconds = time.time() - started
            after = w2_summary(w)
            return {"result": results[-1].get("result", results[-1].get("error")), "calls": len(results),
                    "all_calls": [r.get("result", r.get("error")) for r in results], "seconds": seconds,
                    "after": brief(after), "vs_w2_full": compare_outputs(full["outputs"], after["outputs"], False)}
        finally:
            w.stop(store=False)

    def w2_cli_import(path):
        def run(copy):
            started = time.time()
            cli = wallet_cli(copy, ["import_key_images", path])
            row = {"cli_rc": cli["rc"], "cli_tail": cli["stdout"][-600:], "seconds": time.time() - started,
                   "totals": cli_import_totals(cli["stdout"])}
            w = open_wallet(copy)
            try:
                after = w2_summary(w)
                row["after"] = brief(after)
                row["vs_w2_full"] = compare_outputs(full["outputs"], after["outputs"], False)
            finally:
                w.stop(store=False)
            return row
        return run

    out["w2_viewonly_imports_w2_rpc"] = w2_vo_import_copy(label, label + "-vo-kiw2", w2_rpc_import)
    out["w2_viewonly_imports_crate_file"] = w2_vo_import_copy(label, label + "-vo-kimr", w2_cli_import(crate_export))
    if args.w2_chunk:
        crate_read = lib.tool({"op": "ki-read", "seed_file": lib.seed_file(label), "file": crate_export},
                              "ki-%sread-sigs-%s" % (prefix, label))
        signed = [{"key_image": k, "signature": g} for k, g in zip(crate_read["key_images"], crate_read["signatures"])]

        def w2_rpc_import_crate(copy):
            w = open_wallet(copy)
            try:
                started = time.time()
                results = w2_import_rpc(w, crate_read["offset"], signed, args.w2_chunk)
                seconds = time.time() - started
                after = w2_summary(w)
                return {"result": results[-1].get("result", results[-1].get("error")), "calls": len(results),
                        "seconds": seconds, "after": brief(after),
                        "vs_w2_full": compare_outputs(full["outputs"], after["outputs"], False)}
            finally:
                w.stop(store=False)
        out["w2_viewonly_imports_crate_rpc_chunked"] = w2_vo_import_copy(label, label + "-vo-kimrc",
                                                                        w2_rpc_import_crate)

    pristine = lib.state_file(label + "-vo", prefix + "ki-pristine")
    shutil.copy2(lib.state_file(label + "-vo", tag), pristine)
    out["crate_viewonly_imports"] = {}
    for name, path, fmt in (("w2-rpc", rpc_export, "rpc-json"), ("w2-file", file_export, "signed"),
                            ("crate-file", crate_export, "signed")):
        r = crate_ki_import(label, tag, name, path, fmt, pristine)
        row = {k: v for k, v in r.items() if k != "outputs"}
        if r.get("ok"):
            row["vs_w2_full"] = compare_outputs(full["outputs"], r["outputs"], True)
        out["crate_viewonly_imports"][name] = row

    out["refusals"] = {}
    order = sorted(scans[label]["outputs"], key=lambda o: o["block_height"])
    spent_pos = next((i for i, o in enumerate(order) if o["spent"]), None)
    unspent_pos = next((i for i, o in enumerate(order) if not o["spent"]), None)
    if spent_pos is not None and unspent_pos is not None:
        swap = [spent_pos, unspent_pos]
        out["refusals"]["swap_positions"] = swap
        for name, legacy, fmt, do_swap in (("signed-swapped", False, "signed", True),
                                           ("legacy-unsigned", True, "unsigned", False),
                                           ("legacy-unsigned-swapped", True, "unsigned", True)):
            path = os.path.join(EVID, "ki-%s%s-%s.bin" % (prefix, name, label))
            request = {"op": "ki-export", "seed_file": lib.seed_file(label), "state": lib.state_file(label, tag),
                       "out": path, "legacy": legacy}
            if do_swap:
                request["swap"] = swap
            made = lib.tool(request, "ki-%smake-%s-%s" % (prefix, name, label))
            r = crate_ki_import(label, tag, name, path, fmt, pristine)
            row = {"export": made, "crate_import": {k: v for k, v in r.items() if k != "outputs"}}
            if r.get("ok"):
                row["vs_w2_full"] = compare_outputs(full["outputs"], r["outputs"], True)
            if name == "signed-swapped":
                row["w2_viewonly_cli_import"] = w2_vo_import_copy(label, label + "-vo-kisw", w2_cli_import(path))
            out["refusals"][name] = row
    path = evidence("ki-roundtrip-%s%s" % (prefix, label), out)
    if args.keep and out["crate_viewonly_imports"]["w2-rpc"].get("ok"):
        shutil.copy2(lib.state_file(label + "-vo", prefix + "ki-w2-rpc"), lib.state_file(label + "-vo", tag))
    print(json.dumps(summarize_roundtrip(out), indent=1))
    print("evidence:", path)


def summarize_roundtrip(out):
    def cmp(row):
        v = (row or {}).get("vs_w2_full") or {}
        return {k: v.get(k) for k in ("only_reference", "only_other", "spent_flag_differences",
                                      "key_image_differences")}
    s = {"height": out["height"], "w2_full": out["w2_full"], "w2_vo_before": out["w2_viewonly_before_import"],
         "crate_vo_before": out["crate_viewonly_before_import"], "export_contents": out["export_contents"],
         "w2_vo<-w2": {"result": out["w2_viewonly_imports_w2_rpc"]["result"],
                       "after": out["w2_viewonly_imports_w2_rpc"]["after"],
                       "cmp": cmp(out["w2_viewonly_imports_w2_rpc"])},
         "w2_vo<-crate-rpc-chunked": {k: (out.get("w2_viewonly_imports_crate_rpc_chunked") or {}).get(k)
                                      for k in ("result", "calls", "after")},
         "w2_vo<-crate": {"totals": out["w2_viewonly_imports_crate_file"]["totals"],
                          "after": out["w2_viewonly_imports_crate_file"].get("after"),
                          "cmp": cmp(out["w2_viewonly_imports_crate_file"])}}
    for name, row in out["crate_viewonly_imports"].items():
        s["crate_vo<-" + name] = {k: row.get(k) for k in ("ok", "error", "assigned", "spent_amount",
                                                           "unspent_amount", "import_seconds", "balance")}
        s["crate_vo<-" + name]["cmp"] = cmp(row)
    for name, row in out["refusals"].items():
        if isinstance(row, dict):
            s["refusal " + name] = {"crate_ok": row["crate_import"].get("ok"),
                                    "crate_error": row["crate_import"].get("error"), "cmp": cmp(row)}
            if "w2_viewonly_cli_import" in row:
                s["refusal " + name]["w2_cli_tail"] = row["w2_viewonly_cli_import"]["cli_tail"][-300:]
    return s


def ledger_guard(key_images):
    ledger = load_evidence("relayed", [])
    assert len(ledger) < MAX_RELAYS, "relay budget used"
    used = {k for entry in ledger for k in entry.get("key_images", [])}
    assert not (set(key_images) & used), "an input was already relayed in another transaction"
    return ledger


def decode_checks(tx, expect_outputs, tiers):
    return {"version_2": tx["version"] == 2, "unlock_time_0": tx["unlock_time"] == 0,
            "rct_type_bulletproof_plus": tx.get("rct_type") == 6, "output_count": len(tx["outputs"]) == expect_outputs,
            "ring_size_16": all(len(i["offsets"]) == 16 for i in tx["inputs"]),
            "fee_sane": 0 < tx["fee"] <= send.MAX_SANE_FEE,
            "fee_matches_a_daemon_tier": bool(send.fee_tier_of(tx, tiers)),
            "no_trailing_bytes": tx.get("trailing_bytes") == 0}


def state_rings_match(state_path, tx):
    with open(state_path) as f:
        state = json.load(f)
    by_ki = {o["key_image"]: o for o in state["outputs"] if o["key_image"]}
    ok = True
    for i in tx["inputs"]:
        stored = by_ki.get(i["key_image"], {}).get("ring")
        if not stored or stored["offsets"] != i["offsets"]:
            ok = False
        elif i["indexes"][stored["real_index"]] != by_ki[i["key_image"]]["global_index"]:
            ok = False
        elif by_ki[i["key_image"]]["spent"] or i["key_image"] in state["pending_spends"]:
            ok = False
    return ok


def ring_db_file(label, tag):
    return lib.state_file(label + "-vo", tag)[:-len("-state.json")] + "-rings.json"


def ring_db_matches(path, tx):
    if not os.path.exists(path):
        return False
    with open(path) as f:
        rings = json.load(f).get("rings", {})
    return all(i["key_image"] in rings and rings[i["key_image"]]["offsets"] == i["offsets"] for i in tx["inputs"])


def self_payment_only(preview, amount):
    txs = preview["transactions"]
    return (len(txs) == 1 and all(d["is_ours"] for d in txs[0]["destinations"])
            and preview["wallet_debit"] == preview["fee"]
            and any(d["amount"] == amount and not d["is_change"] for d in txs[0]["destinations"]))


def cmd_cold_mr(args):
    label, tag, name = args.label, args.state_tag, args.name
    vo_state = lib.state_file(label + "-vo", tag)
    tiers = send.fee_tiers()
    request = {"op": "scan", "seed_file": lib.viewonly_seed_file(label), "state": vo_state}
    request.update(send.LOOKAHEAD)
    scan = lib.tool(request, "cold-%s-scan" % name)
    out = {"name": name, "pairing": "crate view-only, crate signer", "tool": lib.TOOL, "scan_height": scan["height"],
           "recipients": [[args.address, args.amount]], "account": args.account, "inputs": args.input}
    unsigned_path = os.path.join(EVID, "cold-%s-unsigned.json" % name)
    ring_db = ring_db_file(label, tag)
    built = lib.tool({"op": "unsigned-build", "seed_file": lib.viewonly_seed_file(label), "state": vo_state,
                      "ring_db": ring_db,
                      "recipients": [[args.address, args.amount]], "source_account": args.account,
                      "inputs": args.input, "out": unsigned_path}, "cold-%s-build" % name)
    out["build"] = built
    if "rings" not in built:
        evidence("cold-%s" % name, out)
        print(json.dumps(out, indent=1))
        return
    signed_path = os.path.join(EVID, "cold-%s-signed.json" % name)
    signed = lib.tool({"op": "sign", "seed_file": lib.seed_file(label), "unsigned": unsigned_path,
                       "out": signed_path, "max_fee": send.MAX_SANE_FEE, "max_wallet_debit": send.MAX_SANE_FEE},
                      "cold-%s-sign" % name)
    out["sign"] = {k: v for k, v in signed.items() if k != "tx_blobs"}
    if not signed.get("ok"):
        evidence("cold-%s" % name, out)
        print(json.dumps(out, indent=1))
        return
    blob = signed["tx_blobs"][0]
    tx = lib.decode_tx(blob)
    checks = decode_checks(tx, 2, tiers)
    checks["one_transaction"] = len(signed["tx_ids"]) == 1
    checks["fee_as_built"] = tx["fee"] == built["fee"] == signed["fees"][0]
    checks["inputs_are_the_built_rings"] = sorted(i["key_image"] for i in tx["inputs"]) == sorted(
        r["key_image"] for r in built["rings"])
    checks["rings_saved_in_state_file_match_blob"] = state_rings_match(vo_state, tx)
    checks["rings_in_ring_database_file_match_blob"] = ring_db_matches(ring_db, tx)
    checks["self_payment_only"] = self_payment_only(signed["preview"], args.amount)
    out.update({"tx_id": signed["tx_ids"][0], "tx_hex": blob, "decoded": tx, "observer": lib.observer_view(tx),
                "fee_tier": send.fee_tier_of(tx, tiers), "checks": checks, "all_checks_pass": all(checks.values())})
    evidence("cold-%s" % name, out)
    print(json.dumps({k: out[k] for k in ("tx_id", "checks", "all_checks_pass", "observer")}, indent=1))
    if not out["all_checks_pass"] or not args.relay:
        return
    relay(out, name, [i["key_image"] for i in tx["inputs"]], "monero-rust",
          lambda: lib.tool({"op": "relay-signed", "state": vo_state, "ring_db": ring_db, "signed": signed_path},
                           "cold-%s-relay" % name)["relay"])


def relay(out, name, key_images, by, action):
    ledger = ledger_guard(key_images)
    assert not send.tx_status(out["tx_id"])["known"], "transaction is already known to the node"
    entry = {"name": name, "tx_id": out["tx_id"], "key_images": key_images, "by": by, "started": time.time(),
             "result": "pending"}
    ledger.append(entry)
    evidence("relayed", ledger)
    try:
        entry["result"] = action()
    except Exception as exc:  # noqa: BLE001
        entry["result"] = "failure: %r" % exc
    time.sleep(1)
    entry["status_after"] = send.tx_status(out["tx_id"])
    entry["finished"] = time.time()
    evidence("relayed", ledger)
    out["relay"] = entry
    evidence("cold-%s" % name, out)
    print(json.dumps(entry, indent=1))


def cmd_cold_w2(args):
    label, tag, name = args.label, args.state_tag, args.name
    vo_state = lib.state_file(label + "-vo", tag)
    tiers = send.fee_tiers()
    request = {"op": "scan", "seed_file": lib.viewonly_seed_file(label), "state": vo_state}
    request.update(send.LOOKAHEAD)
    scan = lib.tool(request, "cold-%s-scan" % name)
    out = {"name": name, "pairing": "wallet2 view-only, crate signer", "tool": lib.TOOL, "scan_height": scan["height"],
           "recipients": [[args.address, args.amount]], "account": args.account}
    unsigned_path = os.path.join(EVID, "cold-%s-unsigned_monero_tx" % name)
    outputs_path = os.path.join(EVID, "cold-%s-outputs" % name)
    signed_path = os.path.join(EVID, "cold-%s-signed.json" % name)
    txset_path = os.path.join(EVID, "cold-%s-signed_monero_tx" % name)
    w = open_wallet(label + "-vo")
    try:
        w.call("refresh")
        if not os.path.exists(unsigned_path):
            params = {"destinations": [{"address": args.address, "amount": args.amount}],
                      "account_index": args.account, "priority": 0, "get_tx_key": False}
            if args.subaddr:
                params["subaddr_indices"] = args.subaddr
            r = w.raw("transfer", params)
            if "error" in r or not r["result"].get("unsigned_txset"):
                out["w2_transfer"] = r
                evidence("cold-%s" % name, out)
                print(json.dumps(out, indent=1))
                return
            with open(unsigned_path, "wb") as f:
                f.write(bytes.fromhex(r["result"]["unsigned_txset"]))
            out["w2_transfer"] = {k: v for k, v in r["result"].items() if k != "unsigned_txset"}
        exported = w.call("export_outputs", {"all": True})
        with open(outputs_path, "wb") as f:
            f.write(bytes.fromhex(exported["outputs_data_hex"]))

        ring_db = ring_db_file(label, tag)
        out["record_rings"] = lib.tool({"op": "record-rings", "seed_file": lib.viewonly_seed_file(label),
                                        "state": vo_state, "ring_db": ring_db, "unsigned": unsigned_path},
                                       "cold-%s-rings" % name)
        signed = lib.tool({"op": "sign", "seed_file": lib.seed_file(label), "unsigned": unsigned_path,
                           "wallet2_outputs": outputs_path, "out": signed_path, "txset_out": txset_path,
                           "max_fee": send.MAX_SANE_FEE, "max_wallet_debit": send.MAX_SANE_FEE},
                          "cold-%s-sign" % name)
        out["sign"] = {k: v for k, v in signed.items() if k != "tx_blobs"}
        if not signed.get("ok") or not out["record_rings"].get("ok"):
            evidence("cold-%s" % name, out)
            print(json.dumps(out, indent=1))
            return
        blob = signed["tx_blobs"][0]
        tx = lib.decode_tx(blob)
        checks = decode_checks(tx, 2, tiers)
        checks["one_transaction"] = len(signed["tx_ids"]) == 1
        if args.crate_check:
            out["crate_txset_check"] = lib.tool({"op": "txset-check", "seed_file": lib.viewonly_seed_file(label),
                                                 "signed": txset_path, "unsigned": unsigned_path},
                                                "cold-%s-txset-check" % name)
            checks["crate_reader_accepts_signed_set"] = (out["crate_txset_check"].get("ok") is True and
                                                         out["crate_txset_check"]["tx_ids"] == signed["tx_ids"])
        checks["inputs_are_the_recorded_rings"] = sorted(i["key_image"] for i in tx["inputs"]) == sorted(
            r["key_image"] for r in out["record_rings"]["rings"])
        checks["rings_saved_in_state_file_match_blob"] = state_rings_match(vo_state, tx)
        checks["rings_in_ring_database_file_match_blob"] = ring_db_matches(ring_db, tx)
        checks["self_payment_only"] = self_payment_only(signed["preview"], args.amount)
        out.update({"tx_id": signed["tx_ids"][0], "tx_hex": blob, "decoded": tx, "observer": lib.observer_view(tx),
                    "fee_tier": send.fee_tier_of(tx, tiers), "checks": checks,
                    "all_checks_pass": all(checks.values())})
        evidence("cold-%s" % name, out)
        print(json.dumps({k: out[k] for k in ("tx_id", "checks", "all_checks_pass", "observer")}, indent=1))
        if not out["all_checks_pass"] or not args.relay:
            return
        key_images = [i["key_image"] for i in tx["inputs"]]

        def submit():
            with open(txset_path, "rb") as f:
                r = w.raw("submit_transfer", {"tx_data_hex": f.read().hex()})
            out["w2_submit_transfer"] = r.get("result", r.get("error"))
            if "result" in r:
                marked = lib.tool({"op": "relay-signed", "state": vo_state, "ring_db": ring_db,
                                   "signed": signed_path, "broadcast": False}, "cold-%s-mark" % name)
                out["crate_marked_pending"] = marked
                return "OK (wallet2 submit_transfer)"
            status = send.tx_status(out["tx_id"])
            assert not status["known"], "wallet2 refused but the node knows the transaction"
            relayed = lib.tool({"op": "relay-signed", "state": vo_state, "ring_db": ring_db,
                                "signed": signed_path}, "cold-%s-relay" % name)
            return "wallet2 refused the signed set; crate relay: %s" % relayed["relay"]

        relay(out, name, key_images, "wallet2 view-only (crate signer)", submit)
    finally:
        w.stop()


def cmd_four_way(args):
    label, tag = args.label, args.state_tag
    prefix = (tag + "-" if tag else "")
    for _ in range(4):
        scans = {}
        for name in (label, label + "-vo"):
            request = {"op": "scan", "seed_file": lib.seed_file(name), "state": lib.state_file(name, tag)}
            request.update(send.LOOKAHEAD)
            scans[name] = lib.tool(request, "four-%s%s-%s" % (prefix, args.tag, name))
        w = open_wallet(label)
        try:
            w.call("refresh")
            full = w2_summary(w)
            exported = w.call("export_key_images", {"all": True})
        finally:
            w.stop()
        w = open_wallet(label + "-vo")
        try:
            w.call("refresh")
            imported = w.raw("import_key_images", {"offset": exported.get("offset", 0),
                                                   "signed_key_images": exported["signed_key_images"]})
            vo = w2_summary(w)
        finally:
            w.stop()
        if len({full["height"], vo["height"]} | {s["height"] for s in scans.values()}) == 1:
            break
    path = os.path.join(EVID, "four-%s%s-ki-%s.json" % (prefix, args.tag, label))
    with open(path, "w") as f:
        json.dump({"offset": exported.get("offset", 0), "signed_key_images": exported["signed_key_images"]}, f)
    vo_import = lib.tool({"op": "ki-import", "seed_file": lib.viewonly_seed_file(label),
                          "state": lib.state_file(label + "-vo", tag), "file": path, "format": "rpc-json"},
                         "four-%s%s-import-%s" % (prefix, args.tag, label))

    def crate_summary(outputs):
        return {"outputs": len(outputs), "unspent_count": sum(1 for o in outputs if not o["spent"]),
                "balance": sum(o["amount"] for o in outputs if not o["spent"]),
                "unlocked_balance": sum(o["amount"] for o in outputs if not o["spent"] and o["spendable"]
                                        and not o["pending_spend"])}

    out = {"label": label, "state_tag": tag, "tool": lib.TOOL, "height": full["height"],
           "w2_full": brief(full), "w2_viewonly": brief(vo), "w2_viewonly_import": imported.get("result", imported),
           "crate_full": crate_summary(scans[label]["outputs"]),
           "crate_viewonly": crate_summary(vo_import.get("outputs", [])),
           "crate_viewonly_import": {k: v for k, v in vo_import.items() if k != "outputs"},
           "vs_w2_full": {"w2_viewonly": compare_outputs(full["outputs"], vo["outputs"], False),
                          "crate_full": compare_outputs(full["outputs"], scans[label]["outputs"], True),
                          "crate_viewonly": compare_outputs(full["outputs"], vo_import.get("outputs", []), True)}}
    balances = {out["w2_full"]["balance"], out["w2_viewonly"]["balance"], out["crate_full"]["balance"],
                out["crate_viewonly"]["balance"]}
    unlocked = {out["w2_full"]["unlocked_balance"], out["w2_viewonly"]["unlocked_balance"],
                out["crate_full"]["unlocked_balance"], out["crate_viewonly"]["unlocked_balance"]}
    out["all_four_agree"] = (len(balances) == 1 and len(unlocked) == 1 and all(
        v["only_reference"] == 0 and v["only_other"] == 0 and v["spent_flag_differences"] == 0
        and v["key_image_differences"] == 0 for v in out["vs_w2_full"].values()))
    evidence("four-way-%s%s-%s" % (prefix, args.tag, label), out)
    print(json.dumps(out, indent=1))


def register(sub):
    s = sub.add_parser("vo-scan", help="crate view-only scans from wallet2's restore heights, one download")
    s.add_argument("labels", nargs="+")
    s.add_argument("--state-tag", default="")
    s.add_argument("--stop-height", type=int, default=None)
    s.add_argument("--keep-batches", type=int, default=32)
    s.add_argument("--default-lookahead", action="store_true", help="the crate's default (0, 20) window")
    s.add_argument("--crate-default", action="store_true", help="the crate's DEFAULT_LOOKAHEAD, whatever it is")
    s.add_argument("--twin-default", action="store_true", help="also scan each wallet with the (0, 20) window")
    s.add_argument("--twin-from", type=int, default=0, help="lowest start height of a twin")
    s.add_argument("--skip-below", type=int, default=0, help="skip phases that start below this height")
    s.add_argument("--prune", type=lambda v: v == "true", default=None,
                   help="true or false; only for a crate that can scan pruned blocks")
    s.set_defaults(fn=cmd_vo_scan)
    s = sub.add_parser("vo-check", help="crate view-only scans against wallet2's view-only wallets, one height")
    s.add_argument("labels", nargs="+")
    s.add_argument("--state-tag", default="")
    s.add_argument("--tag", default="vo")
    s.add_argument("--default-lookahead", action="store_true")
    s.set_defaults(fn=cmd_vo_check)
    s = sub.add_parser("meter", help="traffic through the metering proxy, or --set PHASE")
    s.add_argument("--set", default=None)
    s.set_defaults(fn=cmd_meter)
    s = sub.add_parser("ki-roundtrip", help="key image export and import both ways on real history")
    s.add_argument("label")
    s.add_argument("--state-tag", default="")
    s.add_argument("--keep", action="store_true",
                   help="afterwards keep wallet2's key images in the crate's view-only state")
    s.add_argument("--status-chunk", type=int, default=0,
                   help="crate: ask the node about this many key images per request (0: one request)")
    s.add_argument("--w2-chunk", type=int, default=0,
                   help="wallet2: import this many key images per import_key_images call (0: one call)")
    s.set_defaults(fn=cmd_ki_roundtrip)
    for name, fn, text in (("cold-mr", cmd_cold_mr, "crate view-only builds, crate signer signs"),
                           ("cold-w2", cmd_cold_w2, "wallet2 view-only builds, crate signer signs")):
        s = sub.add_parser(name, help=text + "; relays only with --relay and only if every check passes")
        s.add_argument("name")
        s.add_argument("label")
        s.add_argument("address")
        s.add_argument("amount", type=int)
        s.add_argument("--account", type=int, default=0)
        s.add_argument("--input", action="append", help="crate builds only: spend this output (txid:index)")
        s.add_argument("--subaddr", type=int, action="append", help="wallet2 builds only: spend from this minor index")
        s.add_argument("--crate-check", action="store_true",
                       help="wallet2 builds only: also read the signed set back with the crate's checked reader")
        s.add_argument("--state-tag", default="")
        s.add_argument("--relay", action="store_true")
        s.set_defaults(fn=fn)
    s = sub.add_parser("four-way", help="full and view-only wallets of both implementations at one height")
    s.add_argument("label")
    s.add_argument("--state-tag", default="")
    s.add_argument("--tag", default="check")
    s.set_defaults(fn=cmd_four_way)
