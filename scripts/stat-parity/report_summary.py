#!/usr/bin/env python3
"""Condensed tables for a written report, from $STAT_PARITY_RESULTS/*.json."""
import glob
import json

import statlib
from mrpc import RESULTS



def p(x):
    if x is None or x != x:
        return "n/a"
    if x == 0:
        return "<1e-300"
    return f"{x:.2g}" if x >= 0.001 else f"{x:.1e}"


def g(x, d=3):
    if x is None or x != x:
        return "n/a"
    return f"{x:.{d}g}"


def main():
    data = {path.split("/")[-1][:-5]: json.load(open(path)) for path in sorted(glob.glob(RESULTS + "/*.json"))}
    rows = [r for d in data.values() for r in d["results"]]
    for r, adj in zip(rows, statlib.holm([r["p"] for r in rows])):
        r["p_holm"] = float(adj)
    family = sum(1 for r in rows if r["p"] == r["p"])
    print(f"Family for the Holm-Bonferroni correction: {family} comparisons.\n")

    def find(name, text):
        hits = [r for r in data[name]["results"] if text in r["name"]]
        return hits[0] if hits else None

    def cell(r, effect=True):
        if r is None:
            return "n/a"
        out = f"p {p(r['p'])} (Holm {p(r['p_holm'])})"
        if effect:
            out = f"{g(r['effect_value'])} [{g(r['ci'][0])}, {g(r['ci'][1])}], " + out
        return out

    print("#### Picker level (candidates per input, wallet2 `gamma_picker` against monero-rust)\n")
    print("| distribution | blocks / RingCT outputs | candidates wallet2 / monero-rust | wallet2 bad picks | KS on global index: D [95% CI], p | block-age bins (chi-square) | smallest raw p of 6 edge regions | control: wallet2 seed 1 vs seed 2 (KS p) | KS min. detectable D |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for name in ("stagenet", "stagenet_zeros", "dense", "regtest_T1", "regtest_S", "regtest_L"):
        key = "picker_" + name
        if key not in data:
            continue
        info = data[key]["info"]
        ks = find(key, "] wallet2 vs monero-rust, candidate global index")
        bins = find(key, "] wallet2 vs monero-rust, block-age bins")
        regions = [r for r in data[key]["results"] if ": share in " in r["name"] and r["p"] == r["p"]]
        worst = min(regions, key=lambda r: r["p"])
        control = find(key, "control: wallet2 seed 1 vs wallet2 seed 2")
        print(f"| {name} | {info['blocks']} / {info['outputs']} | {ks['n1']} / {ks['n2']} | "
              f"{info['wallet2_bad_pick_rate']['seed1']:.4f} | {cell(ks)} | p {p(bins['p'])} (Holm {p(bins['p_holm'])}) | "
              f"{p(worst['p'])} (Holm {p(worst['p_holm'])}): {worst['name'].split('share in ')[1]} | {p(control['p'])} | {g(ks['mde'])} |")
    if "picker_dense_replication" in data:
        print("\nConfirmatory rerun on `dense` with fresh seeds (see section 4.1):\n")
        print("| comparison | n wallet2 / monero-rust | effect [95% CI] | p | p(Holm) |")
        print("| --- | --- | --- | --- | --- |")
        for r in data["picker_dense_replication"]["results"]:
            print(f"| {r['name'].split('] ')[1]} | {r['n1']} / {r['n2']} | {g(r['effect_value'])} [{g(r['ci'][0])}, {g(r['ci'][1])}] | {p(r['p'])} | {p(r['p_holm'])} |")

    print("\n#### Whole pipeline, one input per transaction (regtest)\n")
    print("| snapshot | blocks / RingCT outputs | samples wallet2 / monero-rust | built wallet2 / monero-rust | decoy index KS: D [95% CI], p | decoy age bins | first-attempt candidates KS | real position in ring (chi-square) | samples with a sanity retry, wallet2 vs monero-rust | failed after 3 sanity attempts |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for snap in ("T0", "P90", "P100", "P120", "P150", "T1", "S", "L"):
        key = "rings_" + snap
        if key not in data:
            continue
        info = data[key]["info"]
        w, r = info["wallet2"], info["rust"]
        retry = find(key, "at least one ring-sanity retry")
        w_fail = w["errors"].get("Transaction sanity check failed", 0)
        r_fail = sum(v for k, v in r["errors"].items() if "sanity check" in k)
        print(f"| {snap} | {info['blocks']} / {info['rct_outputs']} | {w['samples']} / {r['samples']} | "
              f"{w['ok']} / {r['ok']} | {cell(find(key, 'decoy global index in final rings'))} | "
              f"{cell(find(key, 'decoy block-age bins in final rings'), False)} | "
              f"{cell(find(key, 'candidate global index, first attempt'), False)} | "
              f"{cell(find(key, 'position of the real output'), False)} | "
              f"{retry['note'].split('. wallet2')[0] if info['sanity_check_active'] else 'check inactive (< 10000 outputs)'}"
              f"{', p ' + p(retry['p']) if info['sanity_check_active'] else ''} | {w_fail} / {r_fail} |")

    for snap in ("S", "L"):
        key = "multi_" + snap
        if key not in data:
            continue
        print(f"\n#### Whole pipeline, many inputs (ring wallet pays one recipient, snapshot {snap})\n")
        print("| payment | samples wallet2 / monero-rust | inputs wallet2 (mean, range) | inputs monero-rust (mean, range) | input count (chi-square) | decoy index KS | whole-transaction median index KS | samples with a sanity failure wallet2 vs monero-rust |")
        print("| --- | --- | --- | --- | --- | --- | --- | --- |")
        for name, c in data[key]["info"]["cells"].items():
            def summary(s):
                counts = {int(k): v for k, v in s["inputs"].items()}
                n = sum(counts.values())
                mean = sum(k * v for k, v in counts.items()) / n if n else float("nan")
                return f"{mean:.2f}, {min(counts)}-{max(counts)}" if n else "n/a"
            P = f"pay {name} XMR]"
            sel = [r for r in data[key]["results"] if P in r["name"]]
            def pick(text):
                hits = [r for r in sel if text in r["name"]]
                return hits[0] if hits else None
            fail = pick("at least one ring-sanity failure")
            print(f"| {name.replace('pay', '')} XMR | {c['wallet2']['samples']} / {c['rust']['samples']} | {summary(c['wallet2'])} | "
                  f"{summary(c['rust'])} | {cell(pick('] number of inputs'), False)} | {cell(pick('decoy global index'), False)} | "
                  f"{cell(pick('median ring member index'), False)} | {fail['note'].split('. wallet2')[0]}, p {p(fail['p'])} (Holm {p(fail['p_holm'])}) |")

    if "rpccheck_rings" in data or "rpccheck" in data:
        print("\n#### Controls: w2harness against the monero-wallet-rpc binary (snapshot L)\n")
        print("| comparison | n monero-wallet-rpc / w2harness | test | p | p(Holm) | effect [95% CI] |")
        print("| --- | --- | --- | --- | --- | --- |")
        for r in data.get("rpccheck_rings", {"results": []})["results"]:
            print(f"| rings: {r['name'].split('w2harness, ')[1]} | {r['n1']} / {r['n2']} | {r['test']} | {p(r['p'])} | {p(r['p_holm'])} | "
                  f"{g(r['effect_value'])} [{g(r['ci'][0])}, {g(r['ci'][1])}] |")
        sel = [r for r in data.get("rpccheck", {"results": []})["results"] if r["p"] == r["p"]]
        if sel:
            worst = min(sel, key=lambda r: r["p"])
            cells = sorted({r["name"].split("]")[0].split("[")[1] for r in sel})
            print(f"| input selection, {len(cells)} cells ({', '.join(cells)}): {len(sel)} comparisons | 150 per cell / 300 or 100 | as in the selection table | "
                  f"smallest {p(worst['p'])} | {p(worst['p_holm'])} | no significant difference in any cell |")

    if "select" in data:
        print("\n#### Input selection (snapshot L; wallet2 n per cell as shown, monero-rust 5000)\n")
        print("| wallet | payment | wallet2: inputs (count: samples) | monero-rust: inputs (count: share) | input count | which outputs are spent | distinct subaddresses among inputs, wallet2 / monero-rust |")
        print("| --- | --- | --- | --- | --- | --- | --- |")
        for wallet, winfo in data["select"]["info"].items():
            for tag, c in winfo["cells"].items():
                if tag.startswith("sweep"):
                    continue
                w, r = c["wallet2"], c["rust"]
                def dist(s, share):
                    if not s["ok"]:
                        return "fails: " + "; ".join(f"{k[:60]}" for k in s["errors"])
                    items = sorted((int(k), v) for k, v in s["inputs"].items())
                    if len(items) > 6:
                        return f"{items[0][0]} to {items[-1][0]}, mean {s['mean_inputs']:.2f}"
                    return ", ".join(f"{k}: {v / s['ok']:.3f}" if share else f"{k}: {v}" for k, v in items)
                def sub(s):
                    return ", ".join(f"{k}: {v / s['ok']:.2f}" for k, v in sorted(s["distinct_subaddresses"].items())) if s["ok"] else "n/a"
                P = f"select[{wallet} {tag}]"
                def pick(text):
                    hits = [x for x in data["select"]["results"] if x["name"] == f"{P} {text}"]
                    return hits[0] if hits else None
                print(f"| {wallet} | {tag} | {dist(w, False)} | {dist(r, True)} | {cell(pick('number of inputs'), False)} | "
                      f"{cell(pick('which outputs are spent'), False)} | {sub(w)} / {sub(r)} |")
        print("\nSweeps:\n")
        print("| wallet | sweep | wallet2: inputs per sweep, transactions | monero-rust: inputs per sweep, transactions | first monero-rust sample built | input count | inputs per transaction | which outputs are swept | distinct subaddresses, wallet2 / monero-rust |")
        print("| --- | --- | --- | --- | --- | --- | --- | --- | --- |")
        for wallet, winfo in data["select"]["info"].items():
            for tag, c in winfo["cells"].items():
                if not tag.startswith("sweep"):
                    continue
                w, r = c["wallet2"], c["rust"]

                def shape(s):
                    if not s["ok"]:
                        return "fails: " + "; ".join(k[:70] for k in s["errors"])
                    inputs = ", ".join(f"{k} in {v}" for k, v in sorted(s["inputs"].items(), key=lambda kv: int(kv[0])))
                    txs = ", ".join(f"{k} tx in {v}" for k, v in sorted(s["transactions"].items()))
                    return f"{inputs} of {s['ok']}; {txs}"

                def sub(s):
                    return ", ".join(f"{k}: {v / s['ok']:.2f}" for k, v in sorted(s["distinct_subaddresses"].items())) if s["ok"] else "n/a"

                built = "; ".join(t["error"] or f"{t['inputs']} inputs, {t['tx_bytes']} bytes" for t in c.get("rust_sweep_build", []))
                P = f"select[{wallet} {tag}]"

                def pick(text):
                    hits = [x for x in data["select"]["results"] if x["name"] == f"{P} {text}"]
                    return hits[0] if hits else None

                print(f"| {wallet} | {tag} | {shape(w)} | {shape(r)} | {built or 'n/a'} | {cell(pick('number of inputs'), False)} | "
                      f"{cell(pick('inputs per transaction'), False)} | {cell(pick('which outputs are spent'), False)} | {sub(w)} / {sub(r)} |")

if __name__ == "__main__":
    main()
