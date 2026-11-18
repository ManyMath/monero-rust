#!/usr/bin/env python3
"""Input selection comparison on the final regtest snapshot.

    analyze_select.py [prefix]      (default prefix: select)
"""
import glob
import json
import sys
from collections import Counter, defaultdict

import numpy as np

import statlib
from mrpc import RESULTS, SAMPLES

FRACTIONAL = 1_000_000_000


def read(path):
    with open(path) as f:
        return [json.loads(line) for line in f]


def cell_stats(rows, transfers, height):
    by_i = {t["i"]: t for t in transfers}
    out = {"n": len(rows), "ok": 0, "errors": Counter(), "n_inputs": [], "n_txs": [], "first": [], "mean_age": [],
           "mean_amount": [], "subaddrs": [], "same_tx": 0, "related": 0, "inclusion": Counter(), "total_in": [],
           "min_amount": [], "sets": Counter(), "dust": 0, "tx_sizes": []}
    for r in rows:
        if not r["ok"]:
            out["errors"][r["err"][:90]] += 1
            continue
        out["ok"] += 1
        sel = [by_i[i] for tx in r["txs"] for i in tx["sel"]]
        out["n_txs"].append(len(r["txs"]))
        out["tx_sizes"] += [len(tx["sel"]) for tx in r["txs"]]
        out["n_inputs"].append(len(sel))
        out["first"].append(sel[0]["gidx"])
        out["mean_age"].append(float(np.mean([height - t["height"] for t in sel])))
        out["mean_amount"].append(float(np.mean([t["amount"] for t in sel])) / 1e12)
        out["min_amount"].append(min(t["amount"] for t in sel) / 1e12)
        out["total_in"].append(sum(t["amount"] for t in sel) / 1e12)
        out["subaddrs"].append(len({(t["major"], t["minor"]) for t in sel}))
        out["sets"][tuple(sorted(t["gidx"] for t in sel))] += 1
        out["dust"] += any(t["amount"] < FRACTIONAL for t in sel)
        same_tx = related = False
        for tx in r["txs"]:
            ts = [by_i[i] for i in tx["sel"]]
            for a in range(len(ts)):
                for b in range(a + 1, len(ts)):
                    if ts[a]["txid"] == ts[b]["txid"]:
                        same_tx = True
                    if abs(ts[a]["height"] - ts[b]["height"]) < 10:
                        related = True
        out["same_tx"] += same_tx
        out["related"] += related
        for t in sel:
            out["inclusion"][t["gidx"]] += 1
    return out


def counts(a, b):
    keys = sorted(set(a) | set(b))
    ca, cb = Counter(a), Counter(b)
    return [ca[k] for k in keys], [cb[k] for k in keys], keys


def main():
    prefix = sys.argv[1] if len(sys.argv) > 1 else "select"
    results, info = [], {}
    for w2_path in sorted(glob.glob(f"{SAMPLES}/{prefix}.*.wallet2.jsonl")):
        wallet = w2_path.split(f"{prefix}.")[1].split(".wallet2")[0]
        w2_all = read(w2_path)
        rust_all = read(w2_path.replace(".wallet2.jsonl", ".rust.jsonl"))
        w2_t = [r for r in w2_all if r.get("ev") == "transfer"]
        rust_t = [r for r in rust_all if r.get("ev") == "transfer"]
        height = next(r["height"] for r in w2_all if r.get("ev") == "restored")
        rust_height = next(r["height"] for r in rust_all if r.get("ev") == "restored")
        w2_by_g = {t["gidx"]: t for t in w2_t}
        rust_by_g = {t["gidx"]: t for t in rust_t}
        winfo = {
            "height": {"wallet2": height, "rust": rust_height},
            "outputs": {"wallet2": len(w2_t), "rust": len(rust_t)},
            "spendable": {"wallet2": sum(t["unlocked"] for t in w2_t), "rust": sum(t["unlocked"] for t in rust_t)},
            "same_output_set": sorted(w2_by_g) == sorted(rust_by_g),
            "same_storage_order": [t["gidx"] for t in w2_t] == [t["gidx"] for t in rust_t],
            "same_amounts_heights_subaddresses": all(
                (w2_by_g[g]["amount"], w2_by_g[g]["height"], w2_by_g[g]["major"], w2_by_g[g]["minor"], w2_by_g[g]["txid"]) ==
                (rust_by_g[g]["amount"], rust_by_g[g]["height"], rust_by_g[g]["major"], rust_by_g[g]["minor"], rust_by_g[g]["txid"])
                for g in w2_by_g if g in rust_by_g),
            "spendability_disagreements": [g for g in w2_by_g if g in rust_by_g and w2_by_g[g]["unlocked"] != rust_by_g[g]["unlocked"]],
            "cells": {},
        }
        w2_cells, rust_cells = defaultdict(list), defaultdict(list)
        for r in w2_all:
            if r.get("ev") == "sample":
                w2_cells[r["tag"]].append(r)
        for r in rust_all:
            if r.get("ev") == "sample":
                rust_cells[r["tag"]].append(r)
        for tag in w2_cells:
            a = cell_stats(w2_cells[tag], w2_t, height)
            b = cell_stats(rust_cells.get(tag, []), rust_t, height)
            P = f"{prefix}[{wallet} {tag}]"
            cinfo = {}
            for label, s in (("wallet2", a), ("rust", b)):
                cinfo[label] = {
                    "samples": s["n"], "ok": s["ok"], "errors": dict(s["errors"]),
                    "inputs": dict(sorted(Counter(s["n_inputs"]).items())),
                    "transactions": dict(sorted(Counter(s["n_txs"]).items())),
                    "mean_inputs": float(np.mean(s["n_inputs"])) if s["n_inputs"] else None,
                    "mean_input_age_blocks": float(np.mean(s["mean_age"])) if s["mean_age"] else None,
                    "mean_input_amount": float(np.mean(s["mean_amount"])) if s["mean_amount"] else None,
                    "mean_total_input": float(np.mean(s["total_in"])) if s["total_in"] else None,
                    "distinct_subaddresses": dict(sorted(Counter(s["subaddrs"]).items())),
                    "share_with_same_tx_inputs": s["same_tx"] / s["ok"] if s["ok"] else None,
                    "share_with_inputs_under_10_blocks_apart": s["related"] / s["ok"] if s["ok"] else None,
                    "distinct_input_sets": len(s["sets"]),
                    "share_spending_fractional_output": s["dust"] / s["ok"] if s["ok"] else None,
                    "most_common_input_sets": [[list(k), v] for k, v in s["sets"].most_common(3)],
                }
            cinfo["outputs_only_wallet2_selects"] = sorted(set(a["inclusion"]) - set(b["inclusion"]))[:40]
            cinfo["outputs_only_rust_selects"] = sorted(set(b["inclusion"]) - set(a["inclusion"]))[:40]
            winfo["cells"][tag] = cinfo
            if tag.startswith("sweep") and a["ok"] and b["ok"]:
                first = next(r for r in rust_cells[tag] if r["ok"])
                cinfo["rust_sweep_build"] = [{"inputs": len(t["sel"]), "tx_bytes": t.get("tx_bytes"),
                                              "error": t.get("build_error")} for t in first["txs"]]
                c1, c2, keys = counts(a["n_txs"], b["n_txs"])
                results.append(statlib.contingency(f"{P} number of transactions", c1, c2, keys))
                c1, c2, keys = counts(a["tx_sizes"], b["tx_sizes"])
                results.append(statlib.contingency(f"{P} inputs per transaction", c1, c2, keys))
            results.append(statlib.two_proportions(f"{P} build fails", a["n"] - a["ok"], a["n"], b["n"] - b["ok"], b["n"]))
            if not a["ok"] or not b["ok"]:
                continue
            c1, c2, keys = counts(a["n_inputs"], b["n_inputs"])
            results.append(statlib.contingency(f"{P} number of inputs", c1, c2, keys))
            results.append(statlib.mean_difference(f"{P} mean number of inputs", a["n_inputs"], b["n_inputs"]))
            spendable = [t for t in w2_t if t["unlocked"] and not t["spent"]]
            labels = [f"output {t['gidx']} ({t['amount'] / 1e12:g} XMR, height {t['height']}, "
                      f"subaddress {t['major']}/{t['minor']})" for t in spendable]
            results.append(statlib.inclusion(
                f"{P} which outputs are spent", [a["inclusion"][t["gidx"]] for t in spendable], a["ok"],
                [b["inclusion"][t["gidx"]] for t in spendable], b["ok"], labels))
            results.append(statlib.mean_difference(f"{P} mean age of selected inputs (blocks)", a["mean_age"], b["mean_age"]))
            results.append(statlib.mean_difference(f"{P} mean amount of selected inputs (XMR)", a["mean_amount"], b["mean_amount"]))
            results.append(statlib.mean_difference(f"{P} smallest selected input (XMR)", a["min_amount"], b["min_amount"]))
            c1, c2, keys = counts(a["subaddrs"], b["subaddrs"])
            results.append(statlib.contingency(f"{P} distinct subaddresses among inputs", c1, c2, keys))
            results.append(statlib.two_proportions(f"{P} transaction spends an output below wallet2's fractional threshold",
                                                   a["dust"], a["ok"], b["dust"], b["ok"]))
            results.append(statlib.two_proportions(f"{P} transaction has two inputs from one earlier transaction",
                                                   a["same_tx"], a["ok"], b["same_tx"], b["ok"]))
            results.append(statlib.two_proportions(f"{P} transaction has two inputs received under 10 blocks apart",
                                                   a["related"], a["ok"], b["related"], b["ok"]))
        info[wallet] = winfo
    with open(f"{RESULTS}/{prefix}.json", "w") as f:
        json.dump({"info": info, "results": results}, f, indent=1)
    for wallet, winfo in info.items():
        print("==", wallet, {k: v for k, v in winfo.items() if k != "cells"})
        for tag, c in winfo["cells"].items():
            print(f"  {tag}")
            for label in ("wallet2", "rust"):
                s = c[label]
                print(f"    {label:8s} ok {s['ok']}/{s['samples']} inputs {s['inputs']} txs {s['transactions']} "
                      f"age {s['mean_input_age_blocks'] and round(s['mean_input_age_blocks'])} "
                      f"amount {s['mean_input_amount'] and round(s['mean_input_amount'], 4)} subaddrs {s['distinct_subaddresses']} "
                      f"same_tx {s['share_with_same_tx_inputs']} related {s['share_with_inputs_under_10_blocks_apart']} "
                      f"sets {s['distinct_input_sets']} errors {s['errors']}")
    for r in results:
        if r["p"] == r["p"] and r["p"] < 0.01:
            print(f"{r['name']}: {r['test']} p={r['p']:.3g} {r['effect']}={r['effect_value']:.4g} CI=[{r['ci'][0]:.4g}, {r['ci'][1]:.4g}]")


if __name__ == "__main__":
    main()
