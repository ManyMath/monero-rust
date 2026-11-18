#!/usr/bin/env python3
"""Confirmatory picker comparison on `dense`: wallet2 seeds 3 to 5 against monero-rust seeds 201 to 212."""
import json

import numpy as np

import analyze_picker as ap
import statlib
from mrpc import PICKS, RESULTS, RUN

name = "dense"
cum = np.fromfile(f"{RUN}/dist/{name}.u64", "<u8")
reals = json.load(open(f"{PICKS}/{name}.rust.seed101.meta.json"))["real_indexes"]
w = np.concatenate([ap.group(np.fromfile(f"{PICKS}/{name}.w2.seed{k}.u64", "<u8"), reals, rings=480000)[0]
                    for k in (3, 4, 5)])
r = np.concatenate([np.fromfile(f"{PICKS}/{name}.rust.seed{s}.cand.u64", "<u8") for s in range(201, 213)])
aw, ar = ap.ages(cum, w), ap.ages(cum, r)
P = "picker[dense, replication]"
results = [statlib.ks(f"{P} wallet2 vs monero-rust, candidate global index", w, r)]
for label, pred in (("newest spendable block (age 9)", lambda a: a == 9),
                    ("five newest spendable blocks (age 9-13)", lambda a: a <= 13),
                    ("recent zone (age 9-23)", lambda a: a <= 23)):
    results.append(statlib.two_proportions(f"{P} wallet2 vs monero-rust: share in {label}",
                                           int(pred(aw).sum()), len(aw), int(pred(ar).sum()), len(ar)))
results.append(statlib.contingency(f"{P} wallet2 vs monero-rust, block-age bins",
                                   ap.age_counts(cum, w), ap.age_counts(cum, r), ap.age_labels()))
json.dump({"info": {"wallet2_candidates": int(len(w)), "rust_candidates": int(len(r))}, "results": results},
          open(f"{RESULTS}/picker_dense_replication.json", "w"), indent=1)
for x in results:
    print(x["name"], x["test"], "p=%.4g" % x["p"], x["effect_value"], x["ci"])
