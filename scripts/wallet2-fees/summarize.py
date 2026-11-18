#!/usr/bin/env python3
"""Turn a run_matrix.py output directory into a per-scenario comparison table.

  summarize.py <out-dir>            markdown table on stdout
  summarize.py <out-dir> --calls    per-scenario request sequences by implementation
  summarize.py <out-dir> --agreement  where monero-rust's rate is, and is not, the one wallet2 paid
"""

import collections
import json
import sys

LEVELS = {1: "low", 2: "normal", 3: "elevated", 4: "priority"}


def load(path):
    return [json.loads(line) for line in open(path)]


def short_error(err):
    if err is None:
        return ""
    text = err.get("message") if isinstance(err, dict) else str(err)
    return text[:60]


def w2_cell(rec):
    if rec is None:
        return "-"
    if "fee" in rec:
        implied = rec.get("implied") or []
        rates = sorted({(h["rate"], h["mask"]) for h in implied})
        if rates:
            rate, mask = rates[-1] if len({r for r, _ in rates}) == 1 else rates[0]
            return f"{rate}/B m{mask} (fee {rec['fee']}, w {rec['weight']})"
        return f"fee {rec['fee']} w {rec['weight']} ({rec['per_byte']:.1f}/B)"
    return "ERR " + short_error(rec.get("error"))


def rust_cell(rec):
    if rec is None:
        return "-"
    if rec.get("ok"):
        return f"{rec['per_weight']}/B m{rec['mask']}"
    return "ERR " + short_error(rec.get("error"))


def table(out_dir):
    by = collections.OrderedDict()
    for rec in load(f"{out_dir}/results.jsonl"):
        by.setdefault(rec["scenario"], []).append(rec)
    print("| scenario | wallet2 adjust_priority(0) | wallet2 p0 | wallet2 p1 | wallet2 p2 | wallet2 p3 | wallet2 p4 "
          "| rust Automatic (monerod wire) | rust Automatic (object backlog) | rust Low | rust Normal | rust Elevated | rust Priority |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for name, recs in by.items():
        def find(impl, step, wh="any"):
            for r in recs:
                if r["impl"] == impl and r["step"] == step and (wh == "any" or r.get("wallet_height") == wh):
                    return r
            return None
        adj = find("wallet2", "adjust_priority")
        adj_cell = "-"
        if adj:
            adj_cell = str(adj["priority"]) if adj.get("priority") is not None else "ERR " + short_error(adj.get("error"))
        cells = [name, adj_cell]
        for p in range(5):
            cells.append(w2_cell(find("wallet2", f"transfer_p{p}")))
        auto_blob = [r for r in recs if r["impl"] == "rust-blob" and r["step"] == "automatic"]
        auto_obj = [r for r in recs if r["impl"] == "rust-objects" and r["step"] == "automatic"]

        def auto(rs):
            if not rs:
                return "-"
            parts = []
            for r in rs:
                tag = "tip" if r.get("wallet_height") is None else f"wh={r['wallet_height']}"
                parts.append(f"{tag}: {rust_cell(r)}")
            if len({p.split(': ', 1)[1] for p in parts}) == 1:
                return parts[0].split(": ", 1)[1]
            return "; ".join(parts)
        cells.append(auto(auto_blob))
        cells.append(auto(auto_obj))
        for step in ("low", "normal", "elevated", "priority"):
            cells.append(rust_cell(find("rust-blob", step)))
        print("| " + " | ".join(cells) + " |")


def calls(out_dir):
    seqs = collections.OrderedDict()
    for rec in load(f"{out_dir}/transcript.jsonl"):
        label = rec["label"]
        if "|" not in label:
            continue
        call = rec["method"] or rec["path"]
        params = rec.get("params")
        if params:
            params = {k: v for k, v in params.items() if k != "client"}
        seqs.setdefault(label, []).append(call + (json.dumps(params, sort_keys=True) if params else ""))
    for label, seq in seqs.items():
        print(label)
        for call in seq:
            print("   ", call)


def agreement(out_dir):
    by = collections.OrderedDict()
    for rec in load(f"{out_dir}/results.jsonl"):
        by.setdefault(rec["scenario"], []).append(rec)
    counts = collections.Counter()
    notable = []
    steps = [("transfer_p0", "automatic"), ("transfer_p1", "low"), ("transfer_p2", "normal"),
             ("transfer_p3", "elevated"), ("transfer_p4", "priority")]
    for name, recs in by.items():
        for w2_step, rust_step in steps:
            w2 = next((r for r in recs if r["impl"] == "wallet2" and r["step"] == w2_step), None)
            rust = [r for r in recs if r["impl"] == "rust-blob" and r["step"] == rust_step]
            rust = next((r for r in rust if r.get("wallet_height") is not None), rust[0] if rust else None)
            if w2 is None or rust is None:
                continue
            w2_rates = {h["rate"] for h in w2.get("implied") or []}
            if "fee" in w2 and rust.get("ok"):
                verdict = "same rate" if rust["per_weight"] in w2_rates else "DIFFERENT RATE"
            elif "fee" in w2:
                verdict = "wallet2 pays, monero-rust refuses"
            elif rust.get("ok"):
                verdict = "wallet2 refuses, monero-rust has a rate"
            else:
                verdict = "both refuse"
            counts[verdict] += 1
            if verdict != "same rate":
                notable.append(f"{name} {rust_step}: {verdict}: wallet2 {w2_cell(w2)}; monero-rust {rust_cell(rust)}")
    for verdict, n in counts.most_common():
        print(f"{n:4d}  {verdict}")
    print()
    for line in notable:
        print(line)


if __name__ == "__main__":
    if "--calls" in sys.argv:
        calls(sys.argv[1])
    elif "--agreement" in sys.argv:
        agreement(sys.argv[1])
    else:
        table(sys.argv[1])
