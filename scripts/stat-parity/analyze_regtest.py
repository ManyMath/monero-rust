#!/usr/bin/env python3
"""Whole-pipeline ring comparison on a regtest snapshot.

    analyze_regtest.py rings <snapshot> | multi <snapshot> | rpccheck
"""
import glob
import json
import sys
from collections import Counter

import numpy as np

import statlib
from mrpc import RESULTS, RUN, SAMPLES

AGE_EDGES = [9, 10, 11, 12, 14, 19, 24, 59, 100, 200, 500, 1000, 2000]


def load(pattern):
    rows = []
    for path in sorted(glob.glob(pattern)):
        with open(path) as f:
            rows += [r for r in map(json.loads, f) if r.get("ev") == "sample"]
    return rows


def parse_trace(trace):
    calls = []
    call = None
    section = None
    for tok in trace.split():
        kind = tok[0]
        if kind == "A":
            call = {"inputs": [], "examined": [], "fail": None}
            calls.append(call)
        elif kind == "F":
            call["inputs"].append({"real": None, "picks": [], "known": False, "gamma": False})
            section = "pick"
        elif kind == "R":
            call["inputs"][-1]["real"] = int(tok[1:])
        elif kind == "K":
            call["inputs"][-1]["known"] = True
        elif kind == "G":
            call["inputs"][-1]["gamma"] = True
        elif kind == "p":
            call["inputs"][-1]["picks"].append(int(tok[1:]))
        elif kind == "L":
            call["examined"].append([])
            section = "ring"
        elif kind == "i":
            idx, unlocked = tok[1:].split(":")
            call["examined"][-1].append((int(idx), unlocked == "1"))
        elif kind in "MQ":
            call["fail"] = kind
    return calls


def age_counts(cumulative, indexes):
    block = np.searchsorted(cumulative, np.asarray(indexes, dtype=np.uint64), side="right")
    age = (len(cumulative) - 1) - block
    return np.histogram(age, bins=np.array(AGE_EDGES + [10**9]))[0], age


def age_labels():
    return [f"{lo}+" if hi is None else (f"{lo}" if hi == lo + 1 else f"{lo}-{hi - 1}")
            for lo, hi in zip(AGE_EDGES, AGE_EDGES[1:] + [None])]


def summarize_wallet2(rows):
    out = {"n": len(rows), "ok": 0, "errors": Counter(), "decoys": [], "cand_first": [], "cand_all": [],
           "fails": [], "positions": [], "n_inputs": [], "requested_per_input": Counter(), "locked_examined": 0,
           "examined": 0, "locked_in_ring": 0, "ring_not_first_usable": 0, "reused_ring_first_call": 0,
           "median_frac": [], "unused_cand": [], "used_all_path": 0, "dup_in_ring": 0, "ring_sizes": Counter(),
           "first_ring_decoys": [], "cand_rejected": [], "cand_accepted": [], "fail_kinds": Counter(),
           "unlocked_flags_first": [0, 0], "inner_calls": Counter(), "decoys_by_fail": {0: [], 1: []}}
    for r in rows:
        calls = parse_trace(r.get("trace", ""))
        fresh = [c for c in calls if not any(i["known"] for i in c["inputs"])]
        nfail = sum(1 for c in calls if c["fail"])
        for c in calls:
            if c["fail"]:
                out["fail_kinds"][c["fail"]] += 1
        out["inner_calls"][len(calls)] += 1
        if calls and any(i["known"] for i in calls[0]["inputs"]):
            out["reused_ring_first_call"] += 1
        if not r["ok"]:
            out["errors"][r["err"][:80]] += 1
            out["fails"].append(nfail)
            continue
        out["ok"] += 1
        out["fails"].append(nfail)
        inputs = [i for tx in r["txs"] for i in tx["inputs"]]
        out["n_inputs"].append(len(inputs))
        final = fresh[-1] if fresh else None
        for c in fresh:
            for i in c["inputs"]:
                out["requested_per_input"][len(i["picks"]) + (1 if i["real"] is not None else 0)] += 1
                target = out["cand_rejected"] if c["fail"] else out["cand_accepted"]
                target += i["picks"]
                out["cand_all"] += i["picks"]
            if not c["inputs"] or not c["inputs"][0]["gamma"]:
                out["used_all_path"] += 1
        if fresh:
            for i in fresh[0]["inputs"]:
                out["cand_first"] += i["picks"]
            for ex in fresh[0]["examined"]:
                usable = [idx for idx, unlocked in ex[1:] if unlocked]
                out["first_ring_decoys"] += usable[:15]
        all_ring = []
        for n, inp in enumerate(inputs):
            ring = inp["ring"]
            out["ring_sizes"][len(ring)] += 1
            if len(set(ring)) != len(ring):
                out["dup_in_ring"] += 1
            all_ring += ring
            if len(inputs) != 1 or final is None:
                continue
            real = final["examined"][0][0][0] if final["examined"] and final["examined"][0] else None
            ex = final["examined"][0]
            out["examined"] += len(ex) - 1
            out["locked_examined"] += sum(1 for _, u in ex[1:] if not u)
            usable = [idx for idx, u in ex[1:] if u]
            if sorted(usable[:15] + [real]) != ring:
                out["ring_not_first_usable"] += 1
            locked = {idx for idx, u in ex if not u}
            out["locked_in_ring"] += sum(1 for idx in ring if idx in locked)
            decoys = [idx for idx in ring if idx != real]
            out["decoys"] += decoys
            out["decoys_by_fail"][1 if nfail else 0] += decoys
            out["positions"].append(ring.index(real))
            picks = final["inputs"][0]["picks"]
            out["unused_cand"] += [p for p in picks if p not in set(ring)]
        if len(inputs) > 1:
            reals = set()
            for c in calls:
                for i in c["inputs"]:
                    if i["real"] is not None:
                        reals.add(i["real"])
            for inp in inputs:
                out["decoys"] += [idx for idx in inp["ring"] if idx not in reals]
        out["median_frac"].append(float(np.median(sorted(set(all_ring)))))
    return out


def summarize_rust(rows, real_of):
    out = {"n": len(rows), "ok": 0, "errors": Counter(), "decoys": [], "cand_first": [], "cand_all": [],
           "fails": [], "positions": [], "n_inputs": [], "requested_per_input": Counter(), "locked_cand": 0,
           "cand_total": 0, "locked_in_ring": 0, "ring_outside_candidates": 0, "median_frac": [],
           "unused_cand": [], "dup_in_ring": 0, "ring_sizes": Counter(), "cand_rejected": [], "cand_accepted": [],
           "calls": Counter(), "unlocked_flags_first": [0, 0], "decoys_by_fail": {0: [], 1: []}}
    for r in rows:
        calls = r.get("calls", [])
        out["calls"][len(calls)] += 1
        if not r["ok"]:
            out["errors"][r["err"][:80]] += 1
            out["fails"].append(len(calls))
            for c in calls:
                out["cand_rejected"] += c["idx"]
            continue
        out["ok"] += 1
        tx = r["txs"][0]
        inputs = tx["inputs"]
        k = len(inputs)
        out["n_inputs"].append(k)
        reals = [real_of(r, inp) for inp in inputs]
        real_set = set(reals)
        flat_idx = [i for c in calls for i in c["idx"]]
        flat_unl = "".join(c["unl"] for c in calls)
        per_attempt = 75 * k
        attempts = len(flat_idx) // per_attempt
        out["fails"].append(attempts - 1)
        for a in range(attempts):
            idx = flat_idx[a * per_attempt:(a + 1) * per_attempt]
            unl = flat_unl[a * per_attempt:(a + 1) * per_attempt]
            for j in range(k):
                seg = idx[j * 75:(j + 1) * 75]
                useg = unl[j * 75:(j + 1) * 75]
                out["requested_per_input"][len(seg)] += 1
                cand = [x for x in seg if x != reals[j]]
                out["cand_all"] += cand
                (out["cand_accepted"] if a == attempts - 1 else out["cand_rejected"]).extend(cand)
                if a == 0:
                    out["cand_first"] += cand
                    out["unlocked_flags_first"][0] += useg.count("1")
                    out["unlocked_flags_first"][1] += len(useg)
                if a == attempts - 1:
                    ring = inputs[j]["ring"]
                    out["ring_sizes"][len(ring)] += 1
                    if len(set(ring)) != len(ring):
                        out["dup_in_ring"] += 1
                    locked = {x for x, u in zip(seg, useg) if u == "0"}
                    out["locked_cand"] += len(locked)
                    out["cand_total"] += len(seg)
                    out["locked_in_ring"] += sum(1 for x in ring if x in locked)
                    if not set(ring) <= set(seg):
                        out["ring_outside_candidates"] += 1
                    decoys = [x for x in ring if x != reals[j]]
                    out["decoys"] += decoys
                    if k == 1:
                        out["decoys_by_fail"][1 if attempts > 1 else 0] += decoys
                        out["positions"].append(ring.index(reals[j]))
                        ring_set = set(ring)
                        out["unused_cand"] += [x for x in seg if x not in ring_set]
        out["median_frac"].append(float(np.median(sorted({x for inp in inputs for x in inp["ring"]}))))
    return out


def dump_transfers(path):
    with open(path) as f:
        return [r for r in map(json.loads, f) if r.get("ev") == "transfer"]


def rings(snapshot):
    cumulative = np.fromfile(f"{RUN}/dist/regtest_{snapshot}.u64", "<u8")
    total = int(cumulative[-1])
    w2_rows = load(f"{SAMPLES}/rings-{snapshot}.[0-9]*.wallet2.jsonl")
    rust_rows = load(f"{SAMPLES}/rings-{snapshot}.[0-9]*.rust.jsonl")
    w2 = summarize_wallet2(w2_rows)
    rust = summarize_rust(rust_rows, lambda r, inp: r["gidx"])

    w2_t = {t["gidx"]: t for t in dump_transfers(f"{SAMPLES}/rings-{snapshot}.dump.wallet2.jsonl")}
    rust_t = {t["gidx"]: t for t in dump_transfers(sorted(glob.glob(f"{SAMPLES}/rings-{snapshot}.[0-9]*.rust.jsonl"))[0])}
    spend_diff = [g for g in sorted(set(w2_t) | set(rust_t))
                  if (w2_t.get(g, {}).get("unlocked"), ) != (rust_t.get(g, {}).get("unlocked"), )]
    order_same = [t["gidx"] for t in sorted(w2_t.values(), key=lambda t: t["i"])] == \
                 [t["gidx"] for t in sorted(rust_t.values(), key=lambda t: t["i"])]

    info = {
        "snapshot": snapshot, "blocks": int(len(cumulative)), "rct_outputs": total,
        "spendable_bound": int(cumulative[-10]), "sanity_check_active": total >= 10000,
        "wallet_outputs": {"wallet2": len(w2_t), "rust": len(rust_t)},
        "wallet_outputs_spendable": {"wallet2": sum(t["unlocked"] for t in w2_t.values()),
                                     "rust": sum(t["unlocked"] for t in rust_t.values())},
        "spendability_disagreements": spend_diff, "storage_order_identical": order_same,
        "newest_spendable_height": {"wallet2": max((t["height"] for t in w2_t.values() if t["unlocked"]), default=None),
                                    "rust": max((t["height"] for t in rust_t.values() if t["unlocked"]), default=None)},
    }
    for label, s in (("wallet2", w2), ("rust", rust)):
        info[label] = {
            "samples": s["n"], "ok": s["ok"], "errors": dict(s["errors"]),
            "sanity_failures_per_sample": dict(Counter(s["fails"])),
            "requested_per_input": dict(s["requested_per_input"]), "ring_sizes": dict(s["ring_sizes"]),
            "rings_with_duplicates": s["dup_in_ring"], "locked_members_in_rings": s["locked_in_ring"],
            "decoys": len(s["decoys"]), "first_attempt_candidates": len(s["cand_first"]),
        }
    info["wallet2"].update({
        "use_everything_path_calls": w2["used_all_path"], "examined_candidates": w2["examined"],
        "locked_candidates_examined_and_skipped": w2["locked_examined"],
        "rings_not_equal_to_first_15_usable_in_secret_order": w2["ring_not_first_usable"],
        "samples_whose_first_call_reused_a_stored_ring": w2["reused_ring_first_call"],
        "sanity_failure_kinds": dict(w2["fail_kinds"]), "inner_get_outs_calls_per_sample": dict(w2["inner_calls"]),
    })
    info["rust"].update({
        "locked_candidates_in_final_requests": rust["locked_cand"], "candidates_in_final_requests": rust["cand_total"],
        "rings_with_member_outside_requested_candidates": rust["ring_outside_candidates"],
        "get_outs_calls_per_sample": dict(rust["calls"]),
    })

    P = f"rings[{snapshot}]"
    results = []
    labels = age_labels()
    if w2["decoys"] and rust["decoys"]:
        results.append(statlib.ks(f"{P} decoy global index in final rings", np.array(w2["decoys"]), np.array(rust["decoys"])))
        c1, a1 = age_counts(cumulative, w2["decoys"])
        c2, a2 = age_counts(cumulative, rust["decoys"])
        results.append(statlib.contingency(f"{P} decoy block-age bins in final rings", c1, c2, labels))
        for label, pred in (("newest spendable block (age 9)", lambda a: a == 9),
                            ("five newest spendable blocks (age 9-13)", lambda a: a <= 13),
                            ("recent zone (age 9-23)", lambda a: a <= 23),
                            ("coinbase-lock zone (age 9-58)", lambda a: a <= 58)):
            results.append(statlib.two_proportions(f"{P} decoy share in {label}", int(pred(a1).sum()), len(a1),
                                                   int(pred(a2).sum()), len(a2)))
        d1, d2 = np.array(w2["decoys"]), np.array(rust["decoys"])
        results.append(statlib.two_proportions(f"{P} decoy share in oldest 10% of outputs",
                                               int((d1 < total // 10).sum()), len(d1), int((d2 < total // 10).sum()), len(d2)))
        results.append(statlib.contingency(f"{P} position of the real output in the sorted ring",
                                           np.bincount(w2["positions"], minlength=16),
                                           np.bincount(rust["positions"], minlength=16), list(range(16)),
                                           note="same real outputs and same samples per output on both sides"))
        results.append(statlib.ks(f"{P} median ring member index of the final ring",
                                  np.array(w2["median_frac"]), np.array(rust["median_frac"])))
    if w2["cand_first"] and rust["cand_first"]:
        results.append(statlib.ks(f"{P} candidate global index, first attempt",
                                  np.array(w2["cand_first"]), np.array(rust["cand_first"])))
        c1, _ = age_counts(cumulative, w2["cand_first"])
        c2, _ = age_counts(cumulative, rust["cand_first"])
        results.append(statlib.contingency(f"{P} candidate block-age bins, first attempt", c1, c2, labels))
    f1, f2 = np.array(w2["fails"]), np.array(rust["fails"])
    results.append(statlib.two_proportions(f"{P} samples with at least one ring-sanity retry",
                                           int((f1 >= 1).sum()), len(f1), int((f2 >= 1).sum()), len(f2),
                                           note="wallet2: tx_sanity_check failures in its log; monero-rust: extra get_outs.bin rounds"))
    results.append(statlib.two_proportions(f"{P} samples that failed outright",
                                           w2["n"] - w2["ok"], w2["n"], rust["n"] - rust["ok"], rust["n"]))
    if w2["examined"] and rust["cand_total"]:
        rings_rust = sum(rust["ring_sizes"].values())
        results.append(statlib.two_proportions(
            f"{P} info: share of candidates the daemon reports locked (like for like only on large chains)",
            w2["locked_examined"], w2["examined"], rust["locked_cand"], rust["cand_total"] - rings_rust,
            note="wallet2: the candidates it examined for the ring (the first picks of each input); "
                 "monero-rust: all 74 requested per input. On small chains first picks differ from later "
                 "picks in both wallets, so the two shares are not the same quantity there"))
    if w2["unused_cand"] and rust["unused_cand"]:
        results.append(statlib.ks(f"{P} candidates left out of the final ring, global index",
                                  np.array(w2["unused_cand"]), np.array(rust["unused_cand"]),
                                  note="requested candidates of the accepted attempt that are not ring members, locked ones included"))
    for label, s in (("wallet2", w2), ("monero-rust", rust)):
        if s["decoys"] and s["unused_cand"]:
            results.append(statlib.ks(f"{P} {label} internal: ring decoys vs candidates left out",
                                      np.array(s["decoys"]), np.array(s["unused_cand"]),
                                      note="within one implementation; the left-out set includes locked candidates, "
                                           "so a difference is expected and should be the same size in both"))
    for label, s in (("wallet2", w2), ("monero-rust", rust)):
        a, b = s["decoys_by_fail"][1], s["decoys_by_fail"][0]
        if len(a) > 100 and len(b) > 100:
            results.append(statlib.ks(f"{P} {label} internal: decoys of retried samples vs first-time samples",
                                      np.array(a), np.array(b)))
            info["rust" if label == "monero-rust" else label]["mean_decoy_index_fraction"] = {"retried": float(np.mean(a)) / total,
                                                        "first_time": float(np.mean(b)) / total}
    if w2["decoys_by_fail"][1] and rust["decoys_by_fail"][1]:
        results.append(statlib.ks(f"{P} decoy global index, retried samples only",
                                  np.array(w2["decoys_by_fail"][1]), np.array(rust["decoys_by_fail"][1])))
    if w2["cand_rejected"] and rust["cand_rejected"]:
        results.append(statlib.ks(f"{P} candidate global index of rejected attempts",
                                  np.array(w2["cand_rejected"]), np.array([x for x in rust["cand_rejected"]])))

    with open(f"{RESULTS}/rings_{snapshot}.json", "w") as f:
        json.dump({"info": info, "results": results}, f, indent=1)
    print(json.dumps(info, indent=1))
    show(results)


def multi(snapshot):
    cumulative = np.fromfile(f"{RUN}/dist/regtest_{snapshot}.u64", "<u8")
    total = int(cumulative[-1])
    results, info = [], {"snapshot": snapshot, "cells": {}}
    for path in sorted(glob.glob(f"{SAMPLES}/multi-{snapshot}.*.wallet2.jsonl")):
        cell = path.split(f"multi-{snapshot}.")[1].split(".wallet2")[0]
        w2_rows = load(path)
        rust_path = path.replace(".wallet2.jsonl", ".rust.jsonl")
        rust_rows = load(rust_path)
        gidx = {t["i"]: t["gidx"] for t in dump_transfers(rust_path)}
        w2 = summarize_wallet2(w2_rows)
        rust = summarize_rust(rust_rows, lambda r, inp: gidx[inp["t"]])
        P = f"multi[{snapshot} pay {cell} XMR]"
        info["cells"][cell] = {
            label: {"samples": s["n"], "ok": s["ok"], "errors": dict(s["errors"]),
                    "inputs": dict(Counter(s["n_inputs"])), "sanity_failures": dict(Counter(s["fails"])),
                    "ring_sizes": dict(s["ring_sizes"]), "rings_with_duplicates": s["dup_in_ring"],
                    "locked_members_in_rings": s["locked_in_ring"]}
            for label, s in (("wallet2", w2), ("rust", rust))}
        info["cells"][cell]["wallet2"]["sanity_failure_kinds"] = dict(w2["fail_kinds"])
        info["cells"][cell]["wallet2"]["inner_get_outs_calls"] = dict(w2["inner_calls"])
        top = max(w2["n_inputs"] + rust["n_inputs"] + [1])
        results.append(statlib.contingency(f"{P} number of inputs", np.bincount(w2["n_inputs"], minlength=top + 1),
                                           np.bincount(rust["n_inputs"], minlength=top + 1), list(range(top + 1))))
        results.append(statlib.mean_difference(f"{P} mean number of inputs", w2["n_inputs"], rust["n_inputs"]))
        f1, f2 = np.array(w2["fails"]), np.array(rust["fails"])
        results.append(statlib.two_proportions(
            f"{P} samples with at least one ring-sanity failure", int((f1 >= 1).sum()), len(f1),
            int((f2 >= 1).sum()), len(f2),
            note="wallet2 re-runs get_outs each time it adds an input, so it has more chances to fail per sample"))
        if w2["decoys"] and rust["decoys"]:
            results.append(statlib.ks(f"{P} decoy global index in final rings", np.array(w2["decoys"]), np.array(rust["decoys"])))
            results.append(statlib.ks(f"{P} median ring member index over the whole transaction",
                                      np.array(w2["median_frac"]), np.array(rust["median_frac"])))
    with open(f"{RESULTS}/multi_{snapshot}.json", "w") as f:
        json.dump({"info": info, "results": results}, f, indent=1)
    print(json.dumps(info, indent=1))
    show(results)


def rpccheck():
    cumulative = np.fromfile(f"{RUN}/dist/regtest_L.u64", "<u8")
    with open(f"{SAMPLES}/rpccheck-rings.walletrpc.jsonl") as f:
        rpc_rows = [json.loads(line) for line in f]
    rpc_decoys = [x for r in rpc_rows for x in r["ring"] if x != r["gidx"]]
    rpc_positions = [r["ring"].index(r["gidx"]) for r in rpc_rows]
    seen = Counter((r["batch"], r["gidx"]) for r in rpc_rows)
    w2 = summarize_wallet2(load(f"{SAMPLES}/rings-L.[0-9]*.wallet2.jsonl"))
    info = {"wallet_rpc_rings": len(rpc_rows), "wallet_rpc_batches": len({r["batch"] for r in rpc_rows}),
            "wallet_rpc_outputs": len({r["gidx"] for r in rpc_rows}),
            "outputs_built_more_than_once_in_a_batch": sum(1 for v in seen.values() if v > 1),
            "identical_rings_for_one_output_across_batches":
                sum(v - 1 for v in Counter((r["gidx"], tuple(r["ring"])) for r in rpc_rows).values()),
            "ring_sizes": dict(Counter(len(r["ring"]) for r in rpc_rows)),
            "w2harness_rings": w2["ok"]}
    P = "rpccheck[L rings]"
    labels = age_labels()
    c1, _ = age_counts(cumulative, rpc_decoys)
    c2, _ = age_counts(cumulative, w2["decoys"])
    results = [
        statlib.ks(f"{P} control: monero-wallet-rpc vs w2harness, decoy global index", np.array(rpc_decoys), np.array(w2["decoys"])),
        statlib.contingency(f"{P} control: monero-wallet-rpc vs w2harness, decoy block-age bins", c1, c2, labels),
        statlib.contingency(f"{P} control: monero-wallet-rpc vs w2harness, position of the real output",
                            np.bincount(rpc_positions, minlength=16), np.bincount(w2["positions"], minlength=16),
                            list(range(16))),
    ]
    with open(f"{RESULTS}/rpccheck_rings.json", "w") as f:
        json.dump({"info": info, "results": results}, f, indent=1)
    print(json.dumps(info, indent=1))
    show(results)


def show(results):
    for r in results:
        print(f"{r['name']}\n    {r['test']}: n={r['n1']}/{r['n2']} stat={r['statistic']:.6g} p={r['p']:.4g} "
              f"{r['effect']}={r['effect_value']:.6g} CI=[{r['ci'][0]:.6g}, {r['ci'][1]:.6g}] mde={r['mde']:.3g}"
              + (f"\n    {r['note']}" if r.get("note") else ""))


if __name__ == "__main__":
    if sys.argv[1] == "rpccheck":
        rpccheck()
    else:
        {"rings": rings, "multi": multi}[sys.argv[1]](sys.argv[2])
