#!/usr/bin/env python3
"""Compare complete rings built by monero-rust and by wallet2 on a live node.

usage: stagenet_stats.py OUT.json --mr FILE.jsonl [...] --w2 FILE.jsonl [...] [--use-tips]

Use --use-tips whenever the two samples were not taken at one chain tip.
"""
import argparse
import bisect
import json
import math
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stagenet_lib as lib  # noqa: E402


def ks_two_sample(a, b):
    a = np.sort(np.asarray(a, dtype=np.float64))
    b = np.sort(np.asarray(b, dtype=np.float64))
    both = np.concatenate([a, b])
    cdf_a = np.searchsorted(a, both, side="right") / len(a)
    cdf_b = np.searchsorted(b, both, side="right") / len(b)
    d = float(np.max(np.abs(cdf_a - cdf_b)))
    ne = len(a) * len(b) / (len(a) + len(b))
    lam = (math.sqrt(ne) + 0.12 + 0.11 / math.sqrt(ne)) * d
    if lam < 1e-9:
        p = 1.0
    else:
        p = 2 * sum((-1) ** (j - 1) * math.exp(-2 * j * j * lam * lam) for j in range(1, 101))
        p = min(max(p, 0.0), 1.0)
    return d, p


def gammaincc(a, x):
    if x <= 0:
        return 1.0
    if x < a + 1:
        term = 1.0 / a
        total = term
        n = a
        for _ in range(10000):
            n += 1
            term *= x / n
            total += term
            if abs(term) < abs(total) * 1e-15:
                break
        return 1.0 - total * math.exp(-x + a * math.log(x) - math.lgamma(a))
    tiny = 1e-300
    b = x + 1 - a
    c = 1 / tiny
    d = 1 / b
    h = d
    for i in range(1, 10000):
        an = -i * (i - a)
        b += 2
        d = an * d + b
        if abs(d) < tiny:
            d = tiny
        c = b + an / c
        if abs(c) < tiny:
            c = tiny
        d = 1 / d
        delta = d * c
        h *= delta
        if abs(delta - 1) < 1e-15:
            break
    return math.exp(-x + a * math.log(x) - math.lgamma(a)) * h


def chi_square_2xk(counts_a, counts_b, min_expected=5):
    a = list(counts_a)
    b = list(counts_b)
    na, nb = sum(a), sum(b)
    merged_a, merged_b = [], []
    acc_a = acc_b = 0
    for x, y in zip(a, b):
        acc_a += x
        acc_b += y
        total = acc_a + acc_b
        if min(total * na / (na + nb), total * nb / (na + nb)) >= min_expected:
            merged_a.append(acc_a)
            merged_b.append(acc_b)
            acc_a = acc_b = 0
    if acc_a or acc_b:
        if merged_a:
            merged_a[-1] += acc_a
            merged_b[-1] += acc_b
        else:
            merged_a.append(acc_a)
            merged_b.append(acc_b)
    stat = 0.0
    for x, y in zip(merged_a, merged_b):
        total = x + y
        ea = total * na / (na + nb)
        eb = total * nb / (na + nb)
        stat += (x - ea) ** 2 / ea + (y - eb) ** 2 / eb
    df = len(merged_a) - 1
    p = gammaincc(df / 2, stat / 2) if df > 0 else 1.0
    v = math.sqrt(stat / (na + nb)) if na + nb else 0.0
    return {"statistic": stat, "df": df, "p": p, "cramers_v": v, "bins": len(merged_a),
            "counts_a": merged_a, "counts_b": merged_b}


def load(paths, need_tip=False):
    builds = []
    dropped = 0
    for path in paths:
        with open(path) as f:
            for line in f:
                d = json.loads(line)
                if not d.get("rings"):
                    continue
                if need_tip:
                    if d.get("daemon_height_before") is None or d["daemon_height_before"] != d["daemon_height_after"]:
                        dropped += 1
                        continue
                builds.append(d)
    return builds, dropped


def real_indexes(builds):
    common = {}
    for b in builds:
        for r in b["rings"]:
            s = set(r["indexes"])
            common[r["key_image"]] = s if r["key_image"] not in common else common[r["key_image"]] & s
    return common


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--mr", nargs="+", required=True)
    ap.add_argument("--w2", nargs="+", required=True)
    ap.add_argument("--distribution", default=None)
    ap.add_argument("--seed", type=int, default=20261118)
    ap.add_argument("--boot", type=int, default=2000)
    ap.add_argument("--common-tips", action="store_true",
                    help="keep only builds made at a chain tip that both samples contain")
    ap.add_argument("--use-tips", action="store_true",
                    help="measure each decoy's age from the chain tip recorded with its build")
    args = ap.parse_args()

    (mr, mr_dropped), (w2, w2_dropped) = load(args.mr, args.use_tips), load(args.w2, args.use_tips)
    if args.common_tips:
        shared = {b["daemon_height_before"] for b in mr} & {b["daemon_height_before"] for b in w2}
        mr = [b for b in mr if b["daemon_height_before"] in shared]
        w2 = [b for b in w2 if b["daemon_height_before"] in shared]
    common = real_indexes(mr + w2)
    ambiguous = {k: sorted(v) for k, v in common.items() if len(v) != 1}
    real = {k: next(iter(v)) for k, v in common.items() if len(v) == 1}
    def repeats(builds):
        seen, dup = set(), 0
        for b in builds:
            for r in b["rings"]:
                key = (r["key_image"], tuple(r["indexes"]))
                dup += key in seen
                seen.add(key)
        return dup

    if args.distribution:
        with open(args.distribution) as f:
            dist = json.load(f)
    else:
        dist = lib.daemon_json("get_output_distribution", {"amounts": [0], "cumulative": True, "from_height": 0,
                                                           "to_height": 0, "binary": False, "compress": False},
                               timeout=600)
        with open(args.out.replace(".json", "-distribution.json"), "w") as f:
            json.dump(dist, f)
    d0 = dist["distributions"][0]
    cumulative = d0["distribution"]
    start_height = d0["start_height"]
    top_height = start_height + len(cumulative) - 1

    def height_of(index):
        return start_height + bisect.bisect_right(cumulative, index)

    def decoys(builds):
        out = []
        for b in builds:
            per = {}
            for r in b["rings"]:
                if r["key_image"] in real:
                    per[r["key_image"]] = [i for i in r["indexes"] if i != real[r["key_image"]]]
            per["_tip"] = (b["daemon_height_before"] - 1) if args.use_tips else top_height
            out.append(per)
        return out

    dm, dw = decoys(mr), decoys(w2)
    kis = sorted(real, key=lambda k: real[k])

    def pooled(builds, ki=None):
        return np.array([i for b in builds for k, v in b.items() if k != "_tip" and ki in (None, k) for i in v],
                        dtype=np.int64)

    height_cache = {}

    def cached_height(index):
        if index not in height_cache:
            height_cache[index] = height_of(index)
        return height_cache[index]

    def pooled_ages(builds):
        return np.array([b["_tip"] - cached_height(int(i)) for b in builds for k, v in b.items() if k != "_tip"
                         for i in v], dtype=np.int64)

    rng = np.random.default_rng(args.seed)

    def boot(stat, a_builds, b_builds):
        values = []
        for _ in range(args.boot):
            ra = [a_builds[i] for i in rng.integers(0, len(a_builds), len(a_builds))]
            rb = [b_builds[i] for i in rng.integers(0, len(b_builds), len(b_builds))]
            values.append(stat(ra, rb))
        lo, hi = np.percentile(values, [2.5, 97.5])
        return [float(lo), float(hi)]

    edges = [0] + [10 * 2 ** k for k in range(0, 19)]

    def binned(age):
        counts = [0] * len(edges)
        for a in age:
            counts[bisect.bisect_right(edges, int(a)) - 1] += 1
        return counts

    out = {"inputs": {"mr": args.mr, "w2": args.w2}, "bootstrap": {"seed": args.seed, "resamples": args.boot,
                                                                  "unit": "whole builds"},
           "builds": {"mr": len(mr), "w2": len(w2)},
           "age_reference": "tip recorded with each build" if args.use_tips else "one tip for all builds",
           "common_tips_only": args.common_tips,
           "builds_dropped_because_tip_moved": {"mr": mr_dropped, "w2": w2_dropped},
           "tips_seen": {"mr": sorted({b["_tip"] for b in dm}), "w2": sorted({b["_tip"] for b in dw})},
           "exact_ring_repeats": {"mr": repeats(mr), "w2": repeats(w2)},
           "ring_sizes": {"mr": sorted({len(r["indexes"]) for b in mr for r in b["rings"]}),
                          "w2": sorted({len(r["indexes"]) for b in w2 for r in b["rings"]})},
           "rings_per_build": {"mr": sorted({len(b["rings"]) for b in mr}), "w2": sorted({len(b["rings"]) for b in w2})},
           "real_indexes": {k: real[k] for k in kis}, "ambiguous_real": ambiguous,
           "real_heights": {k: height_of(real[k]) for k in kis},
           "distribution_top_height": top_height, "rct_outputs": cumulative[-1],
           "age_bin_edges_blocks": edges}

    a, b = pooled(dm), pooled(dw)
    d, p = ks_two_sample(a, b)
    out["pooled"] = {"n_decoys": {"mr": int(len(a)), "w2": int(len(b))},
                     "ks_global_index": {"D": d, "p": p,
                                         "D_ci95": boot(lambda x, y: ks_two_sample(pooled(x), pooled(y))[0], dm, dw)}}
    age_a, age_b = pooled_ages(dm), pooled_ages(dw)

    def outputs_behind(builds):
        values = []
        for b in builds:
            newest = cumulative[b["_tip"] - 9 - start_height] - 1
            values.extend(newest - int(i) for k, v in b.items() if k != "_tip" for i in v)
        return np.array(values, dtype=np.int64)

    behind_a, behind_b = outputs_behind(dm), outputs_behind(dw)
    d_behind, p_behind = ks_two_sample(behind_a, behind_b)
    d_age, p_age = ks_two_sample(age_a, age_b)
    out["pooled"]["ks_outputs_behind_newest_selectable"] = {
        "D": d_behind, "p": p_behind,
        "D_ci95": boot(lambda x, y: ks_two_sample(outputs_behind(x), outputs_behind(y))[0], dm, dw),
        "min": {"mr": int(behind_a.min()), "w2": int(behind_b.min())}}
    out["pooled"]["ks_age_blocks"] = {"D": d_age, "p": p_age}
    chi = chi_square_2xk(binned(age_a), binned(age_b))
    out["pooled"]["chi_square_age_bins"] = chi
    out["pooled"]["chi_square_age_bins"]["cramers_v_ci95"] = boot(
        lambda x, y: chi_square_2xk(binned(pooled_ages(x)), binned(pooled_ages(y)))["cramers_v"], dm, dw)
    la, lb = np.log(np.maximum(age_a, 1)), np.log(np.maximum(age_b, 1))
    pooled_sd = math.sqrt((la.var(ddof=1) + lb.var(ddof=1)) / 2)

    def log_age_d(x, y):
        u, v = np.log(np.maximum(pooled_ages(x), 1)), np.log(np.maximum(pooled_ages(y), 1))
        return float((u.mean() - v.mean()) / math.sqrt((u.var(ddof=1) + v.var(ddof=1)) / 2))

    def median_age_diff(x, y):
        return float(np.median(pooled_ages(x)) - np.median(pooled_ages(y)))

    out["pooled"]["age_blocks"] = {
        "median": {"mr": float(np.median(age_a)), "w2": float(np.median(age_b))},
        "median_difference_mr_minus_w2": float(np.median(age_a) - np.median(age_b)),
        "median_difference_ci95": boot(median_age_diff, dm, dw),
        "mean_log_age": {"mr": float(la.mean()), "w2": float(lb.mean())},
        "cohens_d_log_age": float((la.mean() - lb.mean()) / pooled_sd),
        "cohens_d_log_age_ci95": boot(log_age_d, dm, dw),
        "min": {"mr": int(age_a.min()), "w2": int(age_b.min())},
        "max": {"mr": int(age_a.max()), "w2": int(age_b.max())},
        "quantiles": {q: {"mr": float(np.quantile(age_a, q)), "w2": float(np.quantile(age_b, q))}
                      for q in (0.05, 0.25, 0.5, 0.75, 0.95)},
        "share_younger_than": {str(n): {"mr": float((age_a < n).mean()), "w2": float((age_b < n).mean())}
                               for n in (20, 100, 720, 5040, 21600, 262800)},
    }
    out["pooled"]["global_index"] = {
        "median": {"mr": float(np.median(a)), "w2": float(np.median(b))},
        "min": {"mr": int(a.min()), "w2": int(b.min())}, "max": {"mr": int(a.max()), "w2": int(b.max())}}

    out["per_tip"] = {}
    total_stat, total_df = 0.0, 0
    for tip in sorted({b["_tip"] for b in dm} & {b["_tip"] for b in dw}):
        xm = [b for b in dm if b["_tip"] == tip]
        xw = [b for b in dw if b["_tip"] == tip]
        ia, ib = pooled(xm), pooled(xw)
        d_tip, p_tip = ks_two_sample(ia, ib)
        chi_tip = chi_square_2xk(binned(pooled_ages(xm)), binned(pooled_ages(xw)))
        total_stat += chi_tip["statistic"]
        total_df += chi_tip["df"]
        out["per_tip"][str(tip)] = {
            "builds": {"mr": len(xm), "w2": len(xw)}, "n_decoys": {"mr": int(len(ia)), "w2": int(len(ib))},
            "ks_global_index": {"D": d_tip, "p": p_tip},
            "chi_square_age_bins": {k: chi_tip[k] for k in ("statistic", "df", "p", "cramers_v", "counts_a", "counts_b")},
            "share_age_below_20": {"mr": float((pooled_ages(xm) < 20).mean()), "w2": float((pooled_ages(xw) < 20).mean())},
            "share_age_9": {"mr": float((pooled_ages(xm) == 9).mean()), "w2": float((pooled_ages(xw) == 9).mean())}}
    out["per_tip_combined_chi_square"] = {"statistic": total_stat, "df": total_df,
                                          "p": gammaincc(total_df / 2, total_stat / 2) if total_df else 1.0}

    out["per_input"] = {}
    for ki in kis:
        x, y = pooled(dm, ki), pooled(dw, ki)
        d, p = ks_two_sample(x, y)
        pos_a = [sorted(r["indexes"]).index(real[ki]) for bld in mr for r in bld["rings"] if r["key_image"] == ki]
        pos_b = [sorted(r["indexes"]).index(real[ki]) for bld in w2 for r in bld["rings"] if r["key_image"] == ki]
        ca = [pos_a.count(i) for i in range(16)]
        cb = [pos_b.count(i) for i in range(16)]
        out["per_input"][ki] = {"real_index": real[ki], "n_decoys": {"mr": int(len(x)), "w2": int(len(y))},
                                "ks_global_index": {"D": d, "p": p},
                                "real_position_counts": {"mr": ca, "w2": cb},
                                "real_position_chi_square": chi_square_2xk(ca, cb)}

    def cross_ring_overlap(builds):
        overlaps = []
        for bld in builds:
            members = [i for r in bld["rings"] for i in r["indexes"]]
            overlaps.append(len(members) - len(set(members)))
        return {"builds_with_shared_members": sum(1 for o in overlaps if o), "max_shared": max(overlaps or [0]),
                "mean_shared": float(np.mean(overlaps)) if overlaps else 0.0}

    out["composition"] = {"cross_ring_overlap": {"mr": cross_ring_overlap(mr), "w2": cross_ring_overlap(w2)},
                          "duplicate_members_in_a_ring": {
                              "mr": sum(1 for bld in mr for r in bld["rings"] if len(set(r["indexes"])) != len(r["indexes"])),
                              "w2": sum(1 for bld in w2 for r in bld["rings"] if len(set(r["indexes"])) != len(r["indexes"]))},
                          "sorted_ascending": {
                              "mr": all(r["indexes"] == sorted(r["indexes"]) for bld in mr for r in bld["rings"]),
                              "w2": all(r["indexes"] == sorted(r["indexes"]) for bld in w2 for r in bld["rings"])}}
    with open(args.out, "w") as f:
        json.dump(out, f, indent=1)
    print(json.dumps({k: out[k] for k in ("builds", "exact_ring_repeats", "ring_sizes", "rings_per_build",
                                          "pooled", "composition")}, indent=1))
    for ki in kis:
        e = out["per_input"][ki]
        print(ki[:16], "real", e["real_index"], "KS D=%.4f p=%.4f" % (e["ks_global_index"]["D"], e["ks_global_index"]["p"]),
              "position chi2=%.2f df=%d p=%.4f" % (e["real_position_chi_square"]["statistic"],
                                                   e["real_position_chi_square"]["df"], e["real_position_chi_square"]["p"]))


if __name__ == "__main__":
    main()
