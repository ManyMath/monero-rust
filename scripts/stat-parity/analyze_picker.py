#!/usr/bin/env python3
"""Picker-level comparison on one output distribution, from run_picker.sh samples.

    analyze_picker.py <name>
"""
import json
import sys

import numpy as np

import statlib
from mrpc import PICKS, RESULTS, RUN

BAD = np.uint64(2**64 - 1)
AGE_EDGES = [9, 10, 11, 12, 14, 19, 24, 59, 100, 200, 500, 1000, 2000, 5000, 10000, 20000, 50000,
             100000, 262800, 525600, 1000000]


def group(picks, reals, per_ring=74, rings=None):
    """First `per_ring` distinct valid draws per ring, the real output excluded, as
    wallet2::get_outs takes them (wallet2.cpp:9817-9917)."""
    out = []
    values = picks.tolist()
    bad = int(BAD)
    pos, ring = 0, 0
    total = len(values)
    while rings is None or ring < rings:
        seen = {reals[ring % len(reals)]}
        got = 0
        start = len(out)
        while got < per_ring and pos < total:
            v = values[pos]
            pos += 1
            if v == bad or v in seen:
                continue
            seen.add(v)
            out.append(v)
            got += 1
        if got < per_ring:
            del out[start:]
            break
        ring += 1
    return np.array(out, dtype=np.uint64), pos


def ages(cumulative, indexes):
    block = np.searchsorted(cumulative, indexes, side="right")
    return (len(cumulative) - 1) - block


def age_counts(cumulative, indexes):
    a = ages(cumulative, indexes)
    edges = np.array(AGE_EDGES + [np.iinfo(np.int64).max])
    return np.histogram(a, bins=edges)[0]


def age_labels():
    labels = []
    for lo, hi in zip(AGE_EDGES, AGE_EDGES[1:] + [None]):
        labels.append(f"{lo}+" if hi is None else (f"{lo}" if hi == lo + 1 else f"{lo}-{hi - 1}"))
    return labels


def region_tests(prefix, cumulative, a, b):
    out = []
    age_a, age_b = ages(cumulative, a), ages(cumulative, b)
    total = int(cumulative[-10])
    regions = {
        "newest spendable block (age 9)": lambda age, idx: age == 9,
        "five newest spendable blocks (age 9-13)": lambda age, idx: age <= 13,
        "recent zone (age 9-23)": lambda age, idx: age <= 23,
        "older than one year (age >= 262800)": lambda age, idx: age >= 262800,
        "oldest 10% of outputs": lambda age, idx: idx < total // 10,
        "first 1% of outputs": lambda age, idx: idx < total // 100,
    }
    for label, pred in regions.items():
        x1, x2 = int(pred(age_a, a).sum()), int(pred(age_b, b).sum())
        out.append(statlib.two_proportions(f"{prefix}: share in {label}", x1, len(a), x2, len(b)))
    return out


def main():
    name = sys.argv[1]
    cumulative = np.fromfile(f"{RUN}/dist/{name}.u64", "<u8")
    w2 = {k: np.fromfile(f"{PICKS}/{name}.w2.{k}.u64", "<u8") for k in ("seed1", "seed2", "native")}
    metas = [json.load(open(f"{PICKS}/{name}.rust.seed{s}.meta.json")) for s in (101, 102, 103, 104)]
    rust_cand = np.concatenate([np.fromfile(f"{PICKS}/{name}.rust.seed{s}.cand.u64", "<u8") for s in (101, 102, 103, 104)])
    rust_ring = np.concatenate([np.fromfile(f"{PICKS}/{name}.rust.seed{s}.ring.u64", "<u8") for s in (101, 102, 103, 104)])
    reals = metas[0]["real_indexes"]
    rings = sum(m["builds"] * m["inputs"] for m in metas)

    results = []
    info = {
        "distribution": name, "blocks": int(len(cumulative)), "outputs": int(cumulative[-1]),
        "spendable_outputs": int(cumulative[-10]),
        "zero_output_blocks": int((np.diff(np.concatenate([[0], cumulative])) == 0).sum()),
        "wallet2_bad_pick_rate": {k: float((v == BAD).mean()) for k, v in w2.items()},
        "rust_builds": sum(m["builds"] for m in metas), "rust_rings": rings,
        "rust_errors": [m["errors"] for m in metas],
        "rust_get_outs_calls_per_build": [m["get_outs_calls_per_build_histogram"] for m in metas],
        "rust_candidates": int(len(rust_cand)), "rust_ring_members": int(len(rust_ring)),
    }
    info["rust_candidates_per_ring"] = len(rust_cand) / rings

    g1, used1 = group(w2["seed1"], reals, rings=rings)
    g2, _ = group(w2["seed2"], reals, rings=rings)
    gn, _ = group(w2["native"], reals, rings=rings)
    info["wallet2_draws_used_for_grouping"] = int(used1)
    info["wallet2_grouped_candidates"] = int(len(g1))
    raw1 = w2["seed1"][w2["seed1"] != BAD]
    info["wallet2_duplicate_draw_rate_within_ring"] = 1 - len(g1) / float((w2["seed1"][:used1] != BAD).sum())

    labels = age_labels()
    P = f"picker[{name}]"
    results.append(statlib.ks(f"{P} control: wallet2 seed 1 vs wallet2 seed 2, candidate index", g1, g2))
    results.append(statlib.ks(f"{P} control: wallet2 seeded RNG vs wallet2 native RNG, candidate index", g1, gn))
    results.append(statlib.contingency(f"{P} control: wallet2 seed 1 vs seed 2, block-age bins",
                                       age_counts(cumulative, g1), age_counts(cumulative, g2), labels))
    results.append(statlib.ks(f"{P} wallet2 vs monero-rust, candidate global index", g1, rust_cand))
    results.append(statlib.ks(f"{P} wallet2 (native RNG) vs monero-rust, candidate global index", gn, rust_cand))
    results.append(statlib.contingency(f"{P} wallet2 vs monero-rust, block-age bins",
                                       age_counts(cumulative, g1), age_counts(cumulative, rust_cand), labels))
    results += region_tests(f"{P} wallet2 vs monero-rust", cumulative, g1, rust_cand)
    results.append(statlib.ks(f"{P} monero-rust internal: ring members vs all candidates", rust_ring, rust_cand))
    results.append(statlib.ks(f"{P} wallet2 internal: first 15 of each group vs all grouped candidates",
                              g1.reshape(-1, 74)[:, :15].ravel(), g1))
    results.append(statlib.ks(f"{P} wallet2 vs monero-rust, ring member global index (first 15 of each group)",
                              g1.reshape(-1, 74)[:, :15].ravel(), rust_ring))
    results.append(statlib.ks(f"{P} info: wallet2 raw draws vs wallet2 grouped candidates", raw1[: len(g1)], g2,
                              note="shows how much de-duplication alone moves the distribution"))

    per_block = np.diff(np.concatenate([[0], cumulative]))
    for label, sample in (("wallet2", g1), ("monero-rust", rust_cand)):
        block = np.searchsorted(cumulative, sample, side="right")
        info[f"{label}_candidates_in_zero_output_blocks"] = int((per_block[block] == 0).sum())
        info[f"{label}_candidates_at_or_above_spendable_bound"] = int((sample >= cumulative[-10]).sum())
        info[f"{label}_max_candidate"] = int(sample.max())
        info[f"{label}_min_candidate"] = int(sample.min())

    with open(f"{RESULTS}/picker_{name}.json", "w") as f:
        json.dump({"info": info, "results": results}, f, indent=1)
    print(json.dumps(info, indent=1))
    for r in results:
        print(f"{r['name']}\n    {r['test']}: n={r['n1']}/{r['n2']} stat={r['statistic']:.6g} p={r['p']:.4g} "
              f"{r['effect']}={r['effect_value']:.6g} CI=[{r['ci'][0]:.6g}, {r['ci'][1]:.6g}] mde={r['mde']:.3g}")


if __name__ == "__main__":
    main()
