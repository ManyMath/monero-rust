#!/usr/bin/env python3
"""Print every comparison under $STAT_PARITY_RESULTS as markdown tables, Holm-Bonferroni adjusted.

    report_tables.py [--all]     (--all also lists non-significant selection rows)
"""
import glob
import json
import sys

import statlib
from mrpc import RESULTS

ORDER = ["picker_stagenet", "picker_stagenet_zeros", "picker_dense", "picker_dense_replication",
         "picker_regtest_T1", "picker_regtest_S", "picker_regtest_L", "position",
         "rings_T0", "rings_P90", "rings_P100", "rings_P120", "rings_P150", "rings_T1", "rings_S", "rings_L", "multi_S", "multi_L", "rpccheck_rings", "rpccheck", "select"]


def fmt(x, digits=4):
    if x is None or x != x:
        return "n/a"
    if x == 0:
        return "0" if digits != 3 else "<1e-300"
    if abs(x) < 1e-3 or abs(x) >= 1e5:
        return f"{x:.{digits - 1}e}"
    return f"{x:.{digits}g}"


def main():
    show_all = "--all" in sys.argv
    files = {p.split("/")[-1][:-5]: p for p in glob.glob(RESULTS + "/*.json")}
    names = [n for n in ORDER if n in files] + sorted(set(files) - set(ORDER))
    rows = []
    for name in names:
        for r in json.load(open(files[name]))["results"]:
            r["file"] = name
            rows.append(r)
    adjusted = statlib.holm([r["p"] for r in rows])
    for r, adj in zip(rows, adjusted):
        r["p_holm"] = float(adj)
    family = sum(1 for r in rows if r["p"] == r["p"])
    print(f"Family: {family} comparisons with a p-value (of {len(rows)} rows). "
          f"p(Holm) is the Holm-Bonferroni adjusted p-value over that family.\n")
    for name in names:
        section = [r for r in rows if r["file"] == name]
        if not section:
            continue
        hidden = 0
        print(f"### {name}\n")
        print("| comparison | n1 / n2 | test | statistic | p | p(Holm) | effect, 95% CI | min. detectable | reading |")
        print("| --- | --- | --- | --- | --- | --- | --- | --- | --- |")
        for r in section:
            significant = r["p_holm"] == r["p_holm"] and r["p_holm"] < 0.05
            if name in ("select", "rpccheck") and not significant and not show_all:
                hidden += 1
                continue
            within = " internal:" in r["name"] or " info:" in r["name"]
            if r["p"] != r["p"]:
                reading = "not testable"
            elif within:
                reading = "within one implementation, not a parity test" + (" (differs, as expected)" if significant else "")
            elif significant:
                reading = "DIFFERENT"
            else:
                reading = "no significant difference"
            effect = f"{r['effect']} = {fmt(r['effect_value'])} [{fmt(r['ci'][0])}, {fmt(r['ci'][1])}]"
            label = r["name"].split("] ", 1)[1] if "] " in r["name"] else r["name"]
            if name in ("select", "rpccheck"):
                label = r["name"].split("[", 1)[1].replace("]", ":", 1)
            print(f"| {label} | {r['n1']} / {r['n2']} | {r['test']} | {fmt(r['statistic'])} | {fmt(r['p'], 3)} | "
                  f"{fmt(r['p_holm'], 3)} | {effect} | {fmt(r['mde'], 3)} {r.get('mde_unit', '')} | {reading} |")
        if hidden:
            print(f"\n{hidden} further rows of this section show no significant difference after correction "
                  f"(listed by `report_tables.py --all`).")
        print()


if __name__ == "__main__":
    main()
