#!/usr/bin/env python3
"""Automatic priority on chains of 9, 10 and 11 blocks; wallet2::adjust_priority needs ten."""

import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common as c
import run_matrix as m

OUT = f"{c.RUN}/short"
STATE = f"{c.RUN}/regtest-short"


def node(cmd):
    env = dict(os.environ, MONERO_REGTEST_DIR=STATE, MONEROD=c.MONEROD)
    out = subprocess.run([f"{c.REPO}/scripts/regtest-node.sh", cmd], env=env, capture_output=True, text=True)
    return out.stdout.strip()


def main():
    os.makedirs(OUT, exist_ok=True)
    results = open(f"{OUT}/results.jsonl", "a")
    addresses = c.addresses()
    node("reset")
    daemon = node("start").split("=", 1)[1]
    fake = wallet = None
    try:
        fake, ports = c.start_fake_node(f"{OUT}/transcript.jsonl", upstream=daemon)
        url = f"http://127.0.0.1:{ports[0]}"
        c.ctl(url, "setup")
        wallet, wallet_url = c.start_wallet_rpc(f"127.0.0.1:{ports[0]}", f"{OUT}/wallets", "short-wallet-rpc")
        c.open_or_restore(wallet_url, "S", c.SEED_A)
        overrides = m.scenario("x", weights=m.FULL)["overrides"]
        objects = m.scenario("x", weights=m.FULL)["overrides"]
        objects["get_txpool_backlog"]["backlog_format"] = "objects"
        port = 0
        height = c.json_rpc(daemon, "get_info")["result"]["height"]
        for target in (9, 10, 11):
            c.json_rpc(daemon, "generateblocks", {"amount_of_blocks": target - height, "wallet_address": addresses["A"]})
            height = c.json_rpc(daemon, "get_info")["result"]["height"]
            c.ctl(url, f"height_{height}|setup")
            c.json_rpc(wallet_url, "refresh")
            wallet_height = c.json_rpc(wallet_url, "get_height")["result"]["height"]
            c.ctl(url, f"height_{height}|wallet2|adjust_priority", overrides)
            port ^= 1
            c.json_rpc(wallet_url, "set_daemon", {"address": f"127.0.0.1:{ports[port]}", "ssl_support": "disabled"})
            res = c.json_rpc(wallet_url, "get_default_fee_priority")
            rec = {"daemon_height": height, "impl": "wallet2", "wallet_height": wallet_height,
                   "priority": res.get("result", {}).get("priority"), "error": res.get("error")}
            results.write(json.dumps(rec) + "\n")
            print(rec, flush=True)
            for variant, ov in (("blob", overrides), ("objects", objects)):
                for wh in (None, wallet_height):
                    c.ctl(url, f"height_{height}|rust-{variant}|automatic|wallet_height={wh}", ov)
                    res = c.probe("probe_rate", {"PROBE_NODE": url, "PROBE_PRIORITY": "automatic", "PROBE_WALLET_HEIGHT": wh})
                    rec = {"daemon_height": height, "impl": f"rust-{variant}", "wallet_height": wh}
                    rec.update(res)
                    results.write(json.dumps(rec) + "\n")
                    print(rec, flush=True)
    finally:
        if wallet:
            try:
                c.json_rpc(wallet_url, "close_wallet")
            except Exception:
                pass
            wallet.terminate()
            wallet.wait()
        if fake:
            fake.terminate()
            fake.wait()
        node("stop")


if __name__ == "__main__":
    main()
