#!/usr/bin/env python3
"""Run the regtest sampling experiments for both implementations.

    run_regtest.py node <snapshot>
    run_regtest.py rings <snapshot> <n per output> [procs] [wallet2|rust]
    run_regtest.py multi <snapshot> <n>
    run_regtest.py select <n wallet2> <n rust> [snapshot] [prefix]
    run_regtest.py pop <source snapshot> <new name> <height>
    run_regtest.py stop
"""
import glob
import json
import os
import shutil
import subprocess
import sys

from mrpc import RUN, SAMPLES, TARGET, W2HARNESS, Rpc, start_monerod, stop_monerod

XMR = 10**12
SEED = 20261118


def wallets():
    with open(RUN + "/wallets.json") as f:
        return json.load(f)


def node(snapshot):
    state = f"{RUN}/node-{snapshot}"
    try:
        with open(state + "/rpc-port") as f:
            url = "http://127.0.0.1:" + f.read().strip()
        Rpc(url).other("get_info")
    except Exception:
        if not os.path.exists(state + "/data"):
            os.makedirs(state, exist_ok=True)
            shutil.copytree(f"{RUN}/snapshots/{snapshot}", state + "/data")
        url = start_monerod(state)
    dist = f"{RUN}/dist/regtest_{snapshot}.u64"
    if not os.path.exists(dist):
        import numpy as np
        os.makedirs(RUN + "/dist", exist_ok=True)
        reply = Rpc(url).call("get_output_distribution", {"amounts": [0], "from_height": 0, "cumulative": True, "binary": False})
        entry = reply["distributions"][0]
        assert entry["start_height"] == 0
        np.asarray(entry["distribution"], dtype="<u8").tofile(dist)
    return url


def pop(source, name, height):
    state = f"{RUN}/node-{name}"
    if not os.path.exists(state + "/data"):
        os.makedirs(state, exist_ok=True)
        shutil.copytree(f"{RUN}/snapshots/{source}", state + "/data")
        url = start_monerod(state)
        current = Rpc(url).other("get_info")["height"]
        Rpc(url).other("pop_blocks", {"nblocks": current - height})
        Rpc(url).call("flush_txpool")
    return node(name)


def w2_start(url, tag, seed, commands, kind="seeded"):
    work = f"{RUN}/w2work/{tag}"
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    out = f"{SAMPLES}/{tag}.wallet2.jsonl"
    proc = subprocess.Popen(
        [f"{W2HARNESS}/w2harness-{kind}", "wallet", url.replace("http://", ""), work, str(seed), out],
        stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=open(work + "/stderr.log", "w"), text=True)
    proc.stdin.write("\n".join(commands) + "\nquit\n")
    proc.stdin.close()
    return proc, out


def rust_bin():
    bins = [p for p in glob.glob(TARGET + "/release/deps/stat_parity_harness-*") if not p.endswith(".d")]
    if not bins:
        raise SystemExit("build the sampler first: cargo test --release --test stat_parity_harness --no-run")
    return max(bins, key=os.path.getmtime)


def rust_start(url, tag, seed, mnemonic, cells):
    out = f"{SAMPLES}/{tag}.rust.jsonl"
    spec = f"{SAMPLES}/{tag}.rust.spec.json"
    with open(spec, "w") as f:
        json.dump({"node": url, "mnemonic": mnemonic, "lookahead": [2, 10], "seed": seed, "out": out,
                   "cells": cells}, f)
    proc = subprocess.Popen([rust_bin(), "regtest_samples", "--exact"],
                            env=dict(os.environ, STATP_SPEC=spec),
                            stdout=open(f"{SAMPLES}/{tag}.rust.log", "w"), stderr=subprocess.STDOUT)
    return proc, out


def wait(procs):
    for proc in procs:
        code = proc.wait()
        if code != 0:
            print("process exited with", code, proc.args, flush=True)


def read(path, ev):
    with open(path) as f:
        return [row for row in map(json.loads, f) if row.get("ev") == ev]


def w2_transfers(url, tag, name):
    w = wallets()[name]
    proc, out = w2_start(url, tag + ".dump", SEED, [f"restore {name} {w['mnemonic']}", "transfers"])
    wait([proc])
    return read(out, "transfer")


def rings(snapshot, n, procs=8, only=None):
    url = node(snapshot)
    w = wallets()["ring"]
    transfers = w2_transfers(url, f"rings-{snapshot}", "ring")
    spendable = [t for t in transfers if t["unlocked"] and not t["spent"]]
    print(f"{snapshot}: {len(transfers)} outputs, {len(spendable)} spendable per wallet2", flush=True)
    running = []
    for chunk in range(procs):
        mine = spendable[chunk::procs]
        if not mine:
            continue
        commands = [f"restore ring {w['mnemonic']}", "trace on"]
        commands += [f"single ring.{t['gidx']} {n} 0 {t['i']} {w['main']}" for t in mine]
        if only != "rust":
            running.append(w2_start(url, f"rings-{snapshot}.{chunk}", SEED + 100 + chunk, commands)[0])
        cells = [{"kind": "ring", "tag": "ring", "n": n, "transfers": [t["gidx"] for t in mine]}]
        if only != "wallet2":
            running.append(rust_start(url, f"rings-{snapshot}.{chunk}", SEED + 200 + chunk, w["mnemonic"], cells)[0])
    wait(running)


def multi(snapshot, n):
    url = node(snapshot)
    w = wallets()
    ring, sink = w["ring"], w["sink"]["main"]
    amounts = {"S": [40, 100, 250], "L": [40, 100, 250]}[snapshot]
    running = []
    for j, amount in enumerate(amounts):
        tag = f"multi-{snapshot}.{amount}"
        commands = [f"restore ring {ring['mnemonic']}", "trace on",
                    f"transfer pay{amount} {n} 0 0 - {sink}:{amount * XMR}"]
        running.append(w2_start(url, tag, SEED + 300 + j, commands)[0])
        cells = [{"kind": "tx", "tag": f"pay{amount}", "n": n, "priority": 0, "account": 0,
                  "amounts": [amount * XMR]}]
        running.append(rust_start(url, tag, SEED + 400 + j, ring["mnemonic"], cells)[0])
    wait(running)


SELECT_CELLS = {
    "many_small": [("one0.03", [0.03], 0, 0), ("one0.19", [0.19], 0, 0), ("pair0.3", [0.3], 0, 0),
                   ("multi0.45", [0.45], 0, 0), ("multi2", [2.0], 0, 0), ("multi5", [5.0], 0, 0),
                   ("two-recipients0.3", [0.1, 0.2], 0, 0), ("pair0.3-priority2", [0.3], 2, 0),
                   ("pair0.3-priority4", [0.3], 4, 0)],
    "few_large": [("one1", [1.0], 0, 0), ("one10", [10.0], 0, 0), ("pair24", [24.0], 0, 0),
                  ("multi40", [40.0], 0, 0)],
    "mixed": [("one0.005", [0.005], 0, 0), ("one1", [1.0], 0, 0), ("one9", [9.0], 0, 0), ("pair20", [20.0], 0, 0),
              ("multi24.9", [24.9], 0, 0), ("multi26", [26.0], 0, 0), ("three-recipients1", [0.5, 0.3, 0.2], 0, 0)],
    "subaddr": [("one0.2", [0.2], 0, 0), ("pair1.5", [1.5], 0, 0), ("multi3", [3.0], 0, 0),
                ("multi8", [8.0], 0, 0), ("multi20", [20.0], 0, 0), ("account1-one1", [1.0], 0, 1),
                ("account1-multi5", [5.0], 0, 1)],
    "burst": [("one0.1", [0.1], 0, 0), ("pair0.6", [0.6], 0, 0), ("multi1.5", [1.5], 0, 0), ("multi5", [5.0], 0, 0)],
    "dusty": [("one0.1", [0.1], 0, 0), ("pair0.5", [0.5], 0, 0), ("multi2", [2.0], 0, 0),
              ("multi2.9", [2.9], 0, 0), ("dust-only0.00002", [0.00002], 0, 0)],
    "sweep_big": [("one0.005", [0.005], 0, 0), ("multi1", [1.0], 0, 0)],
}
SWEEP_CELLS = {
    "many_small": [("sweep", 0, "-", 10)],
    "dusty": [("sweep", 0, "-", 10)],
    "burst": [("sweep", 0, "-", 10)],
    "subaddr": [("sweep-default", 0, "-", 60), ("sweep-all-subaddresses", 0, "0,1,2,3,4", 10),
                ("sweep-account1", 1, "-", 60)],
    "sweep_big": [("sweep", 0, "-", 10)],
}


def select(n_w2, n_rust, snapshot="L", prefix="select"):
    url = node(snapshot)
    w = wallets()
    sink, other = w["sink"]["main"], w["miner"]["main"]
    extra = [sink, other, w["ring"]["main"]]
    running = []
    for j, (name, cells) in enumerate(SELECT_CELLS.items()):
        commands = [f"restore {name} {w[name]['mnemonic']}", "transfers"]
        rust_cells = []
        for tag, amounts, prio, account in cells:
            dests = " ".join(f"{extra[i]}:{int(round(a * XMR))}" for i, a in enumerate(amounts))
            many = sum(amounts) >= 2 or name == "sweep_big"
            commands.append(f"transfer {tag} {max(n_w2 // 3, 20) if many else n_w2} {prio} {account} - {dests}")
            rust_cells.append({"kind": "select", "tag": tag, "n": n_rust, "priority": prio, "account": account,
                               "amounts": [int(round(a * XMR)) for a in amounts]})
        for tag, account, csv, n in SWEEP_CELLS.get(name, []):
            commands.append(f"sweep_all {tag} {n} 0 {account} {csv} {sink}")
            rust_cells.append({"kind": "sweep", "tag": tag, "n": n, "account": account,
                               "subaddresses": "one" if csv == "-" else "all"})
        running.append(w2_start(url, f"{prefix}.{name}", SEED + 500 + j, commands)[0])
        running.append(rust_start(url, f"{prefix}.{name}", SEED + 600 + j, w[name]["mnemonic"], rust_cells)[0])
    wait(running)


def main():
    os.makedirs(SAMPLES, exist_ok=True)
    cmd = sys.argv[1]
    if cmd == "rings":
        rings(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]) if len(sys.argv) > 4 else 8,
              sys.argv[5] if len(sys.argv) > 5 else None)
    elif cmd == "multi":
        multi(sys.argv[2], int(sys.argv[3]))
    elif cmd == "select":
        select(int(sys.argv[2]), int(sys.argv[3]), *sys.argv[4:6])
    elif cmd == "pop":
        print(pop(sys.argv[2], sys.argv[3], int(sys.argv[4])))
    elif cmd == "node":
        print(node(sys.argv[2]))
    elif cmd == "stop":
        for state in glob.glob(RUN + "/node-*"):
            stop_monerod(state)
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
