#!/usr/bin/env python3
"""Real congestion on a private regtest daemon: no daemon answer is altered."""

import json
import os
import struct
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common as c

OUT = f"{c.RUN}/real"
XMR = 10**12


def raw(method, params=None):
    body = {"jsonrpc": "2.0", "id": "0", "method": method}
    if params is not None:
        body["params"] = params
    return c.post(c.daemon_url() + "/json_rpc", body)


def pool_backlog():
    data = raw("get_txpool_backlog")
    key = b'"backlog": "'
    start = data.find(key)
    if start < 0:
        return []
    i = start + len(key)
    out = bytearray()
    unescape = {ord("b"): 8, ord("f"): 12, ord("n"): 10, ord("r"): 13, ord("t"): 9, ord("v"): 11}
    while data[i] != ord('"'):
        if data[i] == ord("\\"):
            i += 1
            out.append(unescape.get(data[i], data[i]))
        else:
            out.append(data[i])
        i += 1
    assert len(out) % 24 == 0, len(out)
    return [struct.unpack_from("<QQQ", out, off) for off in range(0, len(out), 24)]


class Real:
    def __init__(self):
        os.makedirs(OUT, exist_ok=True)
        self.results = open(f"{OUT}/results.jsonl", "a")
        self.addresses = c.addresses()
        self.fake, self.ports = c.start_fake_node(f"{OUT}/transcript.jsonl")
        self.fake_url = f"http://127.0.0.1:{self.ports[0]}"
        self.port_idx = {"fresh": 0, "stale": 0}
        c.ctl(self.fake_url, "setup")
        daemon_addr = c.daemon_url().replace("http://", "")
        self.wb, self.wb_url = c.start_wallet_rpc(daemon_addr, f"{c.RUN}/wallets", "real-wallet-B")
        c.open_or_restore(self.wb_url, "B", c.SEED_B)
        self.wallets = {}
        for kind, name in (("fresh", "A"), ("stale", "A_stale")):
            proc, url = c.start_wallet_rpc(f"127.0.0.1:{self.ports[0]}", f"{c.RUN}/wallets", f"real-wallet-{name}")
            c.open_or_restore(url, name, c.SEED_A)
            self.wallets[kind] = (proc, url)
        self.stale_height = None
        self.outputs_file = f"{OUT}/rust-outputs-A.json"

    def close(self):
        for proc, url in [(self.wb, self.wb_url)] + list(self.wallets.values()):
            try:
                c.json_rpc(url, "close_wallet")
            except Exception:
                pass
            proc.terminate()
            proc.wait()
        self.fake.terminate()
        self.fake.wait()

    def record(self, rec):
        self.results.write(json.dumps(rec) + "\n")
        self.results.flush()
        print(json.dumps(rec)[:300], flush=True)

    def wallet2(self, stage, kind):
        url = self.wallets[kind][1]
        c.ctl(self.fake_url, f"{stage}|wallet2-{kind}|adjust_priority")
        self.port_idx[kind] ^= 1
        c.json_rpc(url, "set_daemon", {"address": f"127.0.0.1:{self.ports[self.port_idx[kind]]}", "ssl_support": "disabled"})
        height = c.json_rpc(url, "get_height")["result"]["height"]
        res = c.json_rpc(url, "get_default_fee_priority")
        self.record({"stage": stage, "impl": f"wallet2-{kind}", "step": "adjust_priority", "wallet_height": height,
                     "priority": res.get("result", {}).get("priority"), "error": res.get("error")})
        c.ctl(self.fake_url, f"{stage}|wallet2-{kind}|transfer_p0")
        res = c.json_rpc(url, "transfer", {
            "destinations": [{"address": self.addresses["B"], "amount": XMR}],
            "priority": 0, "do_not_relay": True, "get_tx_hex": True, "account_index": 0})
        rec = {"stage": stage, "impl": f"wallet2-{kind}", "step": "transfer_p0", "wallet_height": height}
        if "result" in res:
            fee, weight = res["result"]["fee"], res["result"]["weight"]
            rec.update({"fee": fee, "weight": weight, "per_byte": fee / weight})
        else:
            rec["error"] = res.get("error")
        self.record(rec)
        return height

    def measure(self, stage):
        daemon = c.daemon_url()
        info = c.json_rpc(daemon, "get_info")["result"]
        height = info["height"]
        os.makedirs(f"{OUT}/raw/{stage}", exist_ok=True)
        captures = {
            "get_fee_estimate.json": raw("get_fee_estimate", {"grace_blocks": 10}),
            "get_txpool_backlog.bin": raw("get_txpool_backlog", {}),
            "get_info.json": raw("get_info", {}),
            "getblockheadersrange.json": raw("getblockheadersrange", {"start_height": height - 10, "end_height": height - 1}),
        }
        for name, data in captures.items():
            open(f"{OUT}/raw/{stage}/{name}", "wb").write(data)
        fees = json.loads(captures["get_fee_estimate.json"])["result"]
        headers = json.loads(captures["getblockheadersrange.json"])["result"]["headers"]
        backlog = pool_backlog()
        frz = info["block_weight_limit"] // 2
        counted = sum(w for w, fee, _ in backlog if w and fee / w >= fees["fees"][0])
        last10 = sum(h["block_weight"] for h in headers)
        self.record({"stage": stage, "impl": "daemon", "height": height, "fees": fees["fees"],
                     "mask": fees["quantization_mask"], "full_reward_zone": frz,
                     "pool_txs": len(backlog), "pool_weight": sum(w for w, _, _ in backlog),
                     "pool_weight_at_or_above_low_tier": counted, "backlog_blocks": counted // frz,
                     "last10_block_weight": last10, "last10_fill_pct": 100 * last10 // (10 * frz),
                     "expected_wallet2_priority": 2 if (counted // frz > 0 or 100 * last10 // (10 * frz) > 80) else 1})

        c.ctl(self.fake_url, f"{stage}|wallet2-fresh|refresh")
        c.json_rpc(self.wallets["fresh"][1], "refresh")
        fresh_height = self.wallet2(stage, "fresh")
        if self.stale_height is None:
            c.ctl(self.fake_url, f"{stage}|wallet2-stale|refresh")
            c.json_rpc(self.wallets["stale"][1], "refresh")
        self.stale_height = self.wallet2(stage, "stale")

        for wh in (None, fresh_height, self.stale_height):
            c.ctl(self.fake_url, f"{stage}|rust|automatic|wallet_height={wh}")
            res = c.probe("probe_rate", {"PROBE_NODE": self.fake_url, "PROBE_PRIORITY": "automatic",
                                         "PROBE_WALLET_HEIGHT": wh})
            rec = {"stage": stage, "impl": "rust", "step": "automatic", "wallet_height": wh}
            rec.update(res)
            self.record(rec)

        if not os.path.exists(self.outputs_file):
            c.ctl(self.fake_url, "setup|rust-scan")
            res = c.probe("probe_scan", {"PROBE_NODE": self.fake_url, "PROBE_SEED": c.SEED_A,
                                         "PROBE_SCAN_FROM": 1, "PROBE_SCAN_TO": 40, "PROBE_SCAN_WANT": 4,
                                         "PROBE_OUTPUTS_FILE": self.outputs_file})
            print("scan", res, flush=True)
        c.ctl(self.fake_url, f"{stage}|rust|prepare+create_transaction")
        res = c.probe("probe_send", {"PROBE_NODE": self.fake_url, "PROBE_SEED": c.SEED_A,
                                     "PROBE_DEST": self.addresses["B"], "PROBE_AMOUNT": XMR,
                                     "PROBE_DAEMON_HEIGHT": height, "PROBE_OUTPUTS_FILE": self.outputs_file,
                                     "PROBE_PREPARE": 1, "PROBE_PRIORITY": "automatic",
                                     "PROBE_WALLET_HEIGHT": fresh_height})
        rec = {"stage": stage, "impl": "rust", "step": "prepare+create_transaction", "wallet_height": fresh_height}
        res.pop("tx_blob", None)
        rec.update(res)
        if res.get("ok"):
            rec["per_byte"] = res["fee"] / res["weight"]
        self.record(rec)
        c.ctl(self.fake_url, "idle")

    def spam(self, count, amount):
        sent = []
        for _ in range(count):
            res = c.json_rpc(self.wb_url, "transfer", {
                "destinations": [{"address": self.addresses["B"], "amount": amount}],
                "priority": 1, "account_index": 0})
            if "result" not in res:
                raise RuntimeError(res)
            sent.append((res["result"]["weight"], res["result"]["fee"]))
        return sent


def main():
    real = Real()
    try:
        c.json_rpc(real.wb_url, "refresh")
        transfers = c.json_rpc(real.wb_url, "incoming_transfers", {"transfer_type": "available", "account_index": 0})
        coinbase = [t for t in transfers.get("result", {}).get("transfers", []) if t["unlocked"] and t["amount"] < 100 * XMR]
        need = 62 * 72 - len(coinbase)
        if need > 0:
            t = time.time()
            height = c.mine(real.addresses["B"], need + 60)
            print(f"mined to {height} in {time.time() - t:.0f}s", flush=True)
            c.json_rpc(real.wb_url, "refresh")
        balance = c.json_rpc(real.wb_url, "get_balance", {"account_index": 0})["result"]
        print("B unlocked", balance["unlocked_balance"] / XMR, flush=True)

        real.measure("R0_calm")

        amount = 2450 * XMR
        first = real.spam(4, amount)
        print("spam tx (weight, fee):", first, flush=True)
        real.measure("backlog_under_one_block")

        total = sum(w for w, _ in first)
        while total < 310000:
            total += sum(w for w, _ in real.spam(1, amount))
        real.measure("backlog_over_one_block")

        sent = len(pool_backlog())
        real.spam(62 - sent, amount)
        print("pool before mining:", len(pool_backlog()), flush=True)
        c.mine(real.addresses["A"], 10)
        real.measure("ten_full_blocks")

        c.mine(real.addresses["A"], 2)
        real.measure("two_more_blocks")

        c.mine(real.addresses["A"], 10)
        real.measure("calm_again")
    finally:
        real.close()


if __name__ == "__main__":
    main()
