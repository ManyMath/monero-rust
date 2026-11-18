#!/usr/bin/env python3
"""A fee RPC that outlasts wallet2's 3m30s daemon call timeout (src/wallet/wallet2.h:239).

Takes about a quarter of an hour.
"""

import json
import os
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common as c
import run_matrix as m

OUT = f"{c.RUN}/hang"
DELAY = 220
XMR = 10**12


def record(fh, rec):
    fh.write(json.dumps(rec) + "\n")
    fh.flush()
    print(json.dumps(rec), flush=True)


def wallet2_side(fh, addresses):
    fake, ports = c.start_fake_node(f"{OUT}/transcript-wallet2.jsonl")
    url = f"http://127.0.0.1:{ports[0]}"
    c.ctl(url, "setup")
    proc, wallet = c.start_wallet_rpc(f"127.0.0.1:{ports[0]}", f"{c.RUN}/wallets", "hang-wallet-rpc")
    try:
        c.open_or_restore(wallet, "A_hang", c.SEED_A)
        c.json_rpc(wallet, "refresh")
        port = 0
        for name, overrides in (
            ("hang_fee_estimate", m.scenario("x", fe=dict(m.fee_estimate(), delay=DELAY), weights=m.FULL)["overrides"]),
            ("hang_backlog", m.scenario("x", backlog={"backlog": [], "delay": DELAY}, weights=m.FULL)["overrides"]),
            ("hang_headers", m.scenario("x", headers={"block_weights": m.FULL, "delay": DELAY})["overrides"]),
        ):
            c.ctl(url, f"{name}|wallet2|adjust_priority", overrides)
            port ^= 1
            c.json_rpc(wallet, "set_daemon", {"address": f"127.0.0.1:{ports[port]}", "ssl_support": "disabled"})
            t = time.time()
            res = c.json_rpc(wallet, "get_default_fee_priority", timeout=3000)
            record(fh, {"scenario": name, "impl": "wallet2", "step": "adjust_priority",
                        "priority": res.get("result", {}).get("priority"), "error": res.get("error"),
                        "secs": round(time.time() - t, 1)})
            if name != "hang_fee_estimate":
                continue
            c.ctl(url, f"{name}|wallet2|transfer_p1", overrides)
            t = time.time()
            res = c.json_rpc(wallet, "transfer", {
                "destinations": [{"address": addresses["B"], "amount": XMR}],
                "priority": 1, "do_not_relay": True, "account_index": 0}, timeout=3000)
            rec = {"scenario": name, "impl": "wallet2", "step": "transfer_p1", "secs": round(time.time() - t, 1)}
            if "result" in res:
                rec.update({"fee": res["result"]["fee"], "weight": res["result"]["weight"],
                            "per_byte": res["result"]["fee"] / res["result"]["weight"]})
            else:
                rec["error"] = res.get("error")
            record(fh, rec)
    finally:
        try:
            c.json_rpc(wallet, "close_wallet", timeout=30)
        except Exception:
            pass
        proc.terminate()
        proc.wait()
        fake.terminate()
        fake.wait()


def rust_side(fh):
    fake, ports = c.start_fake_node(f"{OUT}/transcript-rust.jsonl")
    url = f"http://127.0.0.1:{ports[0]}"
    try:
        for name, overrides, prio in (
            ("hang_fee_estimate", m.scenario("x", fe=dict(m.fee_estimate(), delay=DELAY), weights=m.FULL)["overrides"], "low"),
            ("hang_backlog", m.scenario("x", backlog={"backlog": [], "delay": DELAY, "backlog_format": "objects"},
                                        weights=m.FULL)["overrides"], "automatic"),
        ):
            c.ctl(url, f"{name}|rust|{prio}", overrides)
            t = time.time()
            res = c.probe("probe_rate", {"PROBE_NODE": url, "PROBE_PRIORITY": prio}, timeout=3000)
            rec = {"scenario": name, "impl": "rust", "step": prio, "wall_secs": round(time.time() - t, 1)}
            rec.update(res)
            record(fh, rec)
    finally:
        fake.terminate()
        fake.wait()


def main():
    os.makedirs(OUT, exist_ok=True)
    fh = open(f"{OUT}/results.jsonl", "a")
    addresses = c.addresses()
    threads = [threading.Thread(target=wallet2_side, args=(fh, addresses)), threading.Thread(target=rust_side, args=(fh,))]
    for t in threads:
        t.start()
    for t in threads:
        t.join()


if __name__ == "__main__":
    main()
