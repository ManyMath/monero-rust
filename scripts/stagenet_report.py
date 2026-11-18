#!/usr/bin/env python3
"""Print markdown tables from the stagenet harness evidence directory.

usage: stagenet_report.py [section ...]
sections: relayed observer verify fees scan decoys requests
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagenet_lib as lib  # noqa: E402

EVID = os.path.join(lib.RUN, "evidence")


def load(name, default=None):
    path = os.path.join(EVID, name + ".json")
    if not os.path.exists(path):
        return default
    with open(path) as f:
        return json.load(f)


def xmr(v):
    return "%d.%012d" % (v // 10 ** 12, v % 10 ** 12)


def relayed():
    ledger = load("relayed", [])
    print("| # | shape | relayed by | txid | relay result | block |")
    print("|---|---|---|---|---|---|")
    for n, e in enumerate(ledger, 1):
        status = lib.get_tx(e["tx_id"]) or {}
        print("| %d | %s | %s | %s | %s | %s |" % (n, e["name"], e["by"], e["tx_id"], e["result"],
                                               status.get("block_height", "pool" if status.get("in_pool") else "?")))


def observer():
    plan = load("shapes")
    print("| shape | impl | in | out | extra layout | extra bytes | add. keys | payment ID field | fee | weight | "
          "fee/byte | ver | unlock | rct | view tags | key images desc |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for name in plan:
        d = load("shape-%s-build" % name)
        if not d:
            continue
        rows = [("monero-rust (relayed)", d["observer"])]
        for w in d.get("wallet2", []):
            if "observer" in w:
                rows.append(("wallet2 (not relayed)", w["observer"]))
            else:
                print("| %s | wallet2 | refused: %s |" % (name, w.get("error")))
        for impl, o in rows:
            print("| %s | %s | %d | %d | %s | %d | %d | %s | %d | %d | %.0f | %d | %d | %s | %s | %s |" % (
                name, impl, o["n_inputs"], o["n_outputs"], " + ".join(o["extra_layout"]), o["extra_len"],
                o["additional_pubkeys"], ",".join(o["payment_id_fields"]) or "none", o["fee"], o["weight"],
                o["fee_per_byte"], o["version"], o["unlock_time"], o["rct_type"],
                "all" if all(o["view_tags"]) else "no", "yes" if o["key_images_sorted_desc"] else "no"))


def verify():
    plan = load("shapes")
    print("| shape | txid | block | recipient credited (wallet2) | sender in wallet2 | change (both) | checks |")
    print("|---|---|---|---|---|---|---|")
    for name in plan:
        v = load("shape-%s-verify" % name)
        if not v:
            continue
        rec = "; ".join("%s %s: %s" % (w, k, xmr(a)) for w, r in v["recipients"].items()
                        for k, a in r["credited"].items())
        sender = "; ".join("%s amount %s fee %s" % (t["type"], xmr(t["amount"]), xmr(t["fee"]))
                           for t in v.get("w2_sender", {}).get("transfers", []))
        change = "; ".join("%s to %s" % (xmr(c["amount"]), c["subaddress_index"] or [0, 0]) for c in v["mr_change"])
        failed = [k for k, ok in v["checks"].items() if not ok]
        print("| %s | %s | %s | %s | %s | %s | %s |" % (name, v["tx_id"][:16], v["status"].get("block_height"), rec,
                                                      sender, change or "none",
                                                      "all %d pass" % len(v["checks"]) if not failed else
                                                      "FAILED: " + ", ".join(failed)))


def fees():
    d = load("fees")
    t = d["daemon_fee_estimate"]
    print("Daemon get_fee_estimate: fees %s, mask %d, height %d, pool size %s" % (
        t["fees"], t["quantization_mask"], d["daemon_height"], d["tx_pool_size"]))
    print()
    print("| wallet2 priority | wallet2 fee | weight | rate | tier | monero-rust priority | fee | weight | rate | tier |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    names = ["automatic", "low", "normal", "elevated", "priority"]
    for p in range(5):
        w = d["wallet2"][str(p)]
        m = d["monero_rust"][names[p]]
        print("| %d | %s | %s | %s | %s | %s | %s | %s | %s | %s |" % (
            p, w.get("fee"), w.get("weight"), "%.0f" % w["fee_per_byte"] if "fee_per_byte" in w else "",
            w.get("tier"), names[p], m.get("fee", m.get("error")), m.get("weight"),
            "%.0f" % m["fee_per_byte"] if "fee_per_byte" in m else "", m.get("tier")))


def scan():
    print("| wallet | scan | wallet2 outputs | monero-rust outputs | only wallet2 | only monero-rust | field diffs | "
          "outgoing txs w2 / mr | outgoing diffs | balances |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for name in sorted(os.listdir(EVID)):
        if not name.startswith("scan-diff-"):
            continue
        d = load(name[:-5])
        ok = all(b["match"] for b in d["balances"])
        print("| %s | %s | %d | %d | %d | %d | %d | %d / %d | %d | %s |" % (
            d["label"], name[len("scan-diff-"):-5], d["w2_outputs"], d["mr_outputs"], len(d["only_wallet2"]),
            len(d["only_monero_rust"]), len(d["field_diffs"]), d["w2_outgoing_txs"], d["mr_outgoing_txs"],
            len(d["outgoing_diffs"]) + len(d["outgoing_only_wallet2"]) + len(d["outgoing_only_monero_rust"]),
            "equal in %d accounts" % len(d["balances"]) if ok else "DIFFER"))


def decoys():
    for name in sorted(os.listdir(EVID)):
        if not name.startswith("decoy-stats") or name.endswith("-distribution.json"):
            continue
        d = load(name[:-5])
        p = d["pooled"]
        c = p["chi_square_age_bins"]
        a = p["age_blocks"]
        print("### %s" % name)
        print("- builds: monero-rust %d, wallet2 %d; decoys %d and %d; age measured from %s" % (
            d["builds"]["mr"], d["builds"]["w2"], p["n_decoys"]["mr"], p["n_decoys"]["w2"],
            d.get("age_reference", "one tip for all builds")))
        print("- KS on global index: D = %.4f, p = %.4g, 95%% bootstrap interval for D [%.4f, %.4f]" % (
            p["ks_global_index"]["D"], p["ks_global_index"]["p"], *p["ks_global_index"]["D_ci95"]))
        if "ks_outputs_behind_newest_selectable" in p:
            k = p["ks_outputs_behind_newest_selectable"]
            print("- KS on distance behind the newest selectable output: D = %.4f, p = %.4g, interval [%.4f, %.4f]; "
                  "KS on age in blocks: D = %.4f, p = %.4g" % (k["D"], k["p"], *k["D_ci95"], p["ks_age_blocks"]["D"],
                                                              p["ks_age_blocks"]["p"]))
        print("- chi-square on %d age bins: statistic %.2f, df %d, p = %.4g, Cramer's V %.4f [%.4f, %.4f]" % (
            c["bins"], c["statistic"], c["df"], c["p"], c["cramers_v"], *c["cramers_v_ci95"]))
        print("- median age in blocks: %.0f and %.0f, difference %.0f [%.1f, %.1f]; Cohen's d on log age %.4f "
              "[%.4f, %.4f]" % (a["median"]["mr"], a["median"]["w2"], a["median_difference_mr_minus_w2"],
                                *a["median_difference_ci95"], a["cohens_d_log_age"], *a["cohens_d_log_age_ci95"]))
        print("- youngest decoy age: %d and %d blocks" % (a["min"]["mr"], a["min"]["w2"]))
        print("- bin counts monero-rust: %s" % c["counts_a"])
        print("- bin counts wallet2:     %s" % c["counts_b"])
        print("- share younger than N blocks: %s" % json.dumps(a["share_younger_than"]))
        print("- cross-ring shared members per build: %s" % json.dumps(d["composition"]["cross_ring_overlap"]))
        for tip, e in d.get("per_tip", {}).items():
            print("- tip %s: builds %d and %d, decoys %d and %d; KS D = %.4f p = %.4f; chi-square %.2f df %d p = %.4f; "
                  "share at age 9: %.4f and %.4f; share younger than 20: %.4f and %.4f" % (
                      tip, e["builds"]["mr"], e["builds"]["w2"], e["n_decoys"]["mr"], e["n_decoys"]["w2"],
                      e["ks_global_index"]["D"], e["ks_global_index"]["p"], e["chi_square_age_bins"]["statistic"],
                      e["chi_square_age_bins"]["df"], e["chi_square_age_bins"]["p"], e["share_age_9"]["mr"],
                      e["share_age_9"]["w2"], e["share_age_below_20"]["mr"], e["share_age_below_20"]["w2"]))
        if "per_tip_combined_chi_square" in d:
            k = d["per_tip_combined_chi_square"]
            print("- per-tip chi-squares combined: statistic %.2f, df %d, p = %.4f" % (k["statistic"], k["df"], k["p"]))
        for ki, e in d["per_input"].items():
            print("- input %s (global index %d): KS D = %.4f p = %.4f; real position chi-square %.2f df %d p = %.4f" % (
                ki[:16], e["real_index"], e["ks_global_index"]["D"], e["ks_global_index"]["p"],
                e["real_position_chi_square"]["statistic"], e["real_position_chi_square"]["df"],
                e["real_position_chi_square"]["p"]))
        print()


def requests():
    d = load("capture-summary")
    for label in ("w2", "mr"):
        for phase, rows in d["sequences"][label].items():
            print("**%s, %s**" % ("wallet2" if label == "w2" else "monero-rust", phase))
            print()
            previous, repeats = None, 0
            for row in rows:
                params = {k: v for k, v in row["params"].items() if k not in ("client", "block_ids_first",
                                                                              "block_ids_last")}
                text = "%s %s" % (row["call"], json.dumps(params, sort_keys=True))
                if text == previous:
                    repeats += 1
                    continue
                if repeats:
                    print("  - (repeated %d more times)" % repeats)
                previous, repeats = text, 0
                print("- `%s` request %d bytes, reply %d bytes" % (text[:300], row["req_bytes"], row["resp_bytes"]))
            if repeats:
                print("  - (repeated %d more times)" % repeats)
            print()
    print("flagged requests:", json.dumps(d["flagged"]))
    print("headers:", json.dumps(d["request_headers"]))


if __name__ == "__main__":
    sections = sys.argv[1:] or ["relayed", "observer", "verify", "fees", "scan", "decoys"]
    for section in sections:
        print("\n<!-- %s -->" % section)
        globals()[section]()
