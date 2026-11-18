#!/usr/bin/env python3
"""Grow the regtest chain for the stat-parity comparison and snapshot it as T0, T1, S and L.

    python build_chain.py
"""
import json
import os
import random
import shutil
import sys
import time

from mrpc import ATOMIC, RUN, Rpc, start_monerod, start_wallet_rpc, stop_monerod

STATE = RUN + "/regtest"
SNAPSHOTS = {80: "T0", 200: "T1", 1100: "S", 4000: "L"}
FINAL = 4000
SEED = 20261118
SHAPES = ["ring", "many_small", "few_large", "mixed", "subaddr", "burst", "dusty", "sweep_big"]


def xmr(x):
    return int(round(x * ATOMIC))


def spread(rng, n, lo, hi):
    return sorted(rng.sample(range(lo, hi), n))


def make_schedule(addr):
    rng = random.Random(SEED)
    sched = {}

    def tx(height, dests):
        sched.setdefault(height, []).append(dests)

    ring = addr["ring"]["main"]
    ring_heights = [90, 110, 130, 150, 170]
    ring_heights += list(range(240, 1061, 40)) + list(range(1160, 3941, 60))
    for snap in (200, 1100, 4000):
        ring_heights += [snap - 12, snap - 11, snap - 10, snap - 9, snap - 8]
    for h in ring_heights:
        tx(h, [(ring, xmr(1.0))])

    for i, h in enumerate(spread(rng, 60, 250, 3950)):
        tx(h, [(addr["many_small"]["main"], xmr(0.05 + 0.03 * (i * 7 % 6)))])

    for h, a in zip([300, 1500, 2800, 3800], [5, 8, 12, 20]):
        tx(h, [(addr["few_large"]["main"], xmr(a))])

    for h, a in zip([400, 2000, 3500], [3, 7, 15]):
        tx(h, [(addr["mixed"]["main"], xmr(a))])
    for i, h in enumerate(spread(rng, 30, 260, 3950)):
        tx(h, [(addr["mixed"]["main"], xmr(round(rng.uniform(0.01, 0.5), 4)))])

    sub = addr["subaddr"]
    heights = spread(rng, 25, 270, 3950)
    rng.shuffle(heights)
    for minor in range(5):
        for j, base in enumerate([0.3, 0.5, 0.7, 1.1, 1.3]):
            tx(heights[minor * 5 + j], [(sub["sub0"][minor], xmr(base * (1 + 0.1 * minor)))])
    for h in (500, 1700, 3000):
        tx(h, [(sub["acct1"], xmr(2.0))])

    burst = addr["burst"]["main"]
    for h in (600, 601, 2200, 3300):
        tx(h, [(burst, xmr(0.4))] * 4)
    for h in (900, 3600):
        tx(h, [(burst, xmr(0.4))])

    dusty = addr["dusty"]["main"]
    for h in spread(rng, 10, 280, 3950):
        tx(h, [(dusty, xmr(0.3))])
    for h in spread(rng, 10, 280, 3950):
        tx(h, [(dusty, 5_000_000)])
    for h in spread(rng, 5, 280, 3950):
        tx(h, [(dusty, 30_000_000)])

    big = addr["sweep_big"]["main"]
    for h in spread(rng, 16, 300, 3950):
        tx(h, [(big, xmr(0.01))] * 15)
    return sched


def fill_count(height):
    if height < 85:
        return 0
    if height < 1100:
        return 1
    if height < 2500:
        return 1 if height % 2 == 0 else 0
    return 2


def main():
    os.makedirs(RUN + "/wallets", exist_ok=True)
    os.makedirs(RUN + "/snapshots", exist_ok=True)
    if os.path.exists(STATE + "/data"):
        raise RuntimeError(STATE + "/data exists; remove it to rebuild the chain")
    url = start_monerod(STATE)
    daemon = Rpc(url)
    proc, w = start_wallet_rpc(
        RUN + "/wallets", url, RUN + "/logs/wallet-rpc-build.log",
        ["--shared-ringdb-dir", RUN + "/ringdb-build"],
    )
    try:
        addr = {}
        for name in ["miner", "sink"] + SHAPES:
            w.call("create_wallet", {"filename": name, "password": "", "language": "English"})
            entry = {
                "mnemonic": w.call("query_key", {"key_type": "mnemonic"})["key"],
                "main": w.call("get_address", {"account_index": 0})["address"],
            }
            if name == "subaddr":
                entry["sub0"] = [entry["main"]] + [
                    w.call("create_address", {"account_index": 0})["address"] for _ in range(4)
                ]
                entry["acct1"] = w.call("create_account")["address"]
            addr[name] = entry
            w.call("close_wallet")
        with open(RUN + "/wallets.json", "w") as f:
            json.dump(addr, f, indent=1)
        sched = make_schedule(addr)
        with open(RUN + "/schedule.json", "w") as f:
            json.dump({str(k): v for k, v in sorted(sched.items())}, f)

        w.call("open_wallet", {"filename": "miner", "password": ""})
        rng = random.Random(SEED + 1)
        sink = addr["sink"]["main"]
        started = time.time()
        stats = {"txs": 0, "failed": 0}

        def send(dests):
            dests = list(dests)
            while len(dests) < 15:
                dests.append((sink, xmr(round(rng.uniform(0.01, 0.05), 4))))
            try:
                w.call("transfer", {
                    "destinations": [{"address": a, "amount": v} for a, v in dests],
                    "priority": 1,
                })
                stats["txs"] += 1
                return True
            except RuntimeError as err:
                stats["failed"] += 1
                print("transfer failed at", daemon.other("get_info")["height"], err, flush=True)
                return False

        while True:
            height = daemon.other("get_info")["height"]
            if height in SNAPSHOTS:
                name = SNAPSHOTS[height]
                stop_monerod(STATE)
                dst = RUN + "/snapshots/" + name
                shutil.rmtree(dst, ignore_errors=True)
                shutil.copytree(RUN + "/regtest/data", dst)
                print("snapshot", name, "at height", height, flush=True)
                if height >= FINAL:
                    break
                url = start_monerod(STATE)
                daemon = Rpc(url)
                w.call("set_daemon", {"address": url, "trusted": True})
                w.call("refresh")
            pending = list(sched.get(height, []))
            for _ in range(fill_count(height)):
                pending.append([])
            for dests in pending:
                if not send(dests) and dests:
                    raise RuntimeError(f"scheduled funding failed at height {height}")
            miner = addr["ring"]["main"] if height <= 20 else addr["miner"]["main"]
            daemon.call("generateblocks", {"amount_of_blocks": 1, "wallet_address": miner})
            w.call("refresh")
            if height % 100 == 0:
                print(f"height {height} txs {stats['txs']} failed {stats['failed']} "
                      f"elapsed {time.time() - started:.0f}s", flush=True)
        print("done", stats, flush=True)
    finally:
        try:
            w.call("close_wallet")
        except Exception:
            pass
        proc.terminate()
        proc.wait()
        stop_monerod(STATE)


if __name__ == "__main__":
    sys.exit(main())
