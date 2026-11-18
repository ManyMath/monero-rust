#!/usr/bin/env python3
"""Fee-selection matrix: the same crafted daemon answers to wallet2 and monero-rust."""

import argparse
import copy
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common as c

CALM = [20000, 80000, 320000, 4000000]
MASK = 10000
FRZ = 300000
FEE_PER_BYTE = 300000  # wallet2's hardcoded fallback, cryptonote_config.h:71
U64 = 2**64 - 1


def fee_estimate(fees=CALM, mask=MASK, fee=None, **extra):
    patch = {"quantization_mask": mask}
    if fees is not None:
        patch["fees"] = fees
        patch["fee"] = fees[0] if (fee is None and fees) else (fee or 0)
    else:
        patch["fee"] = fee
    o = {"patch": patch}
    if fees is None:
        o["remove"] = ["fees"]
    o.update(extra)
    return o


def scenario(name, fe=None, backlog=None, weights=None, info=None, headers=None, extra=None, **opts):
    overrides = {
        "get_fee_estimate": fe if fe is not None else fee_estimate(),
        "get_txpool_backlog": backlog if isinstance(backlog, dict) else {"backlog": backlog or []},
        "getblockheadersrange": headers if headers is not None else {"block_weights": weights or [1000]},
    }
    if info is not None:
        overrides["get_info"] = info
    if extra:
        overrides.update(extra)
    return {"name": name, "overrides": overrides, **opts}


def at_rate(weight, rate=CALM[0], delta=0):
    return [weight, weight * rate + delta, 0]


FULL = [FRZ]


def spread(total, n=10):
    base = total // n
    out = [base] * n
    out[0] += total - base * n
    return out


def scenarios():
    s = []
    s.append(scenario("calm", w2_prios=[0, 1, 2, 3, 4, 5, 100]))

    s.append(scenario("backlog_299999", backlog=[at_rate(FRZ - 1)]))
    s.append(scenario("backlog_300000", backlog=[at_rate(FRZ)]))
    s.append(scenario("backlog_300001", backlog=[at_rate(FRZ + 1)]))
    s.append(scenario("backlog_2x150000", backlog=[at_rate(FRZ // 2), at_rate(FRZ // 2)]))
    s.append(scenario("backlog_300000_rate_minus_1", backlog=[at_rate(FRZ, delta=-1)]))
    s.append(scenario("backlog_300000_rate_plus_1", backlog=[at_rate(FRZ, delta=1)]))
    s.append(scenario("backlog_299999_plus_cheap_tx", backlog=[at_rate(FRZ - 1), [2000, 2000 * CALM[0] - 1, 0]]))
    s.append(scenario("backlog_299999_plus_zero_weight", backlog=[at_rate(FRZ - 1), [0, 10**9, 0]]))
    s.append(scenario("backlog_10_blocks", backlog=[at_rate(150000)] * 20))
    s.append(scenario("backlog_double_rounding", backlog=[[2**40, CALM[0] * 2**40 - 1, 0]]))
    s.append(scenario("backlog_u64_wrap", backlog=[at_rate(2**63, rate=1), at_rate(2**63, rate=1)],
                      fe=fee_estimate([1, 4, 16, 200])))

    s.append(scenario("fill_0", weights=[0]))
    s.append(scenario("fill_80pct_2400000", weights=spread(2400000)))
    s.append(scenario("fill_2429999", weights=spread(2429999)))
    s.append(scenario("fill_2430000", weights=spread(2430000)))
    s.append(scenario("fill_2430001", weights=spread(2430001)))
    s.append(scenario("fill_100pct", weights=FULL))
    s.append(scenario("fill_one_huge_block", weights=[2430000] + [0] * 9))
    s.append(scenario("fill_2430000_limit_600001", weights=spread(2430000), info={"patch": {"block_weight_limit": 600001}}))
    s.append(scenario("fill_2430000_limit_600002", weights=spread(2430000), info={"patch": {"block_weight_limit": 600002}}))
    s.append(scenario("fill_full_limit_from_size", weights=FULL, info={"patch": {"block_weight_limit": 0, "block_size_limit": 600000}}))
    s.append(scenario("fill_full_limit_zero", weights=FULL, info={"patch": {"block_weight_limit": 0, "block_size_limit": 0}}))
    s.append(scenario("fill_full_limit_one", weights=FULL, info={"patch": {"block_weight_limit": 1, "block_size_limit": 1}}))
    s.append(scenario("fill_full_limit_removed", weights=FULL, info={"remove": ["block_weight_limit", "block_size_limit"]}))
    s.append(scenario("backlog_and_full", backlog=[at_rate(FRZ)], weights=FULL))

    fails = [
        ("http500", {"fail": "http500"}),
        ("drop", {"fail": "drop"}),
        ("garbage", {"fail": "garbage"}),
        ("empty_body", {"fail": "empty"}),
        ("jsonrpc_error", {"fail": "jsonrpc_error"}),
        ("status_busy", {"status": "BUSY"}),
        ("status_failed", {"status": "Failed"}),
        ("status_payment", {"status": "PAYMENT REQUIRED"}),
    ]
    for tag, how in fails:
        fe = fee_estimate()
        fe.update(how)
        s.append(scenario(f"fail_fee_estimate_{tag}", fe=fe, weights=FULL, w2_prios=[0, 1, 2, 4]))
    for tag, how in fails:
        bl = {"backlog": []}
        bl.update(how)
        s.append(scenario(f"fail_backlog_{tag}", backlog=bl, weights=FULL, w2_prios=[0, 2]))
    for tag, how in fails:
        s.append(scenario(f"fail_get_info_{tag}", weights=FULL, info=dict(how), w2_prios=[0, 2]))
    for tag, how in fails:
        hd = {"block_weights": FULL}
        hd.update(how)
        s.append(scenario(f"fail_headers_{tag}", headers=hd, w2_prios=[0, 2]))
    for tag, how in fails:
        s.append(scenario(f"fail_hard_fork_info_{tag}", weights=FULL, extra={"hard_fork_info": dict(how)}, w2_prios=[0, 2]))
    s.append(scenario("headers_count_9", headers={"block_weights": FULL, "count": 9}))
    s.append(scenario("headers_count_11", headers={"block_weights": FULL, "count": 11}))
    s.append(scenario("headers_count_0", headers={"block_weights": FULL, "count": 0}))
    s.append(scenario("headers_without_block_weight",
                      headers={"result": {"headers": [{"height": i, "block_size": FRZ} for i in range(10)],
                                          "status": "OK", "credits": 0, "top_hash": "", "untrusted": False}}))
    s.append(scenario("headers_weight_u64_max", weights=[U64]))

    s.append(scenario("fee_all_zero", fe=fee_estimate([0, 0, 0, 0])))
    s.append(scenario("fee_low_tier_zero", fe=fee_estimate([0, 80000, 320000, 4000000])))
    s.append(scenario("fee_low_tier_zero_full", fe=fee_estimate([0, 80000, 320000, 4000000]), weights=FULL))
    s.append(scenario("fees_missing", fe=fee_estimate(None, fee=20000)))
    s.append(scenario("fees_missing_full", fe=fee_estimate(None, fee=20000), weights=FULL))
    s.append(scenario("fees_empty", fe=fee_estimate([], fee=20000)))
    s.append(scenario("fees_len_1", fe=fee_estimate([20000])))
    s.append(scenario("fees_len_1_full", fe=fee_estimate([20000]), weights=FULL))
    s.append(scenario("fees_len_2", fe=fee_estimate([20000, 80000])))
    s.append(scenario("fees_len_3", fe=fee_estimate([20000, 80000, 320000])))
    s.append(scenario("fees_len_5", fe=fee_estimate([20000, 80000, 320000, 4000000, 9000000])))
    s.append(scenario("fees_not_increasing", fe=fee_estimate([80000, 20000, 4000000, 320000])))
    s.append(scenario("fee_field_disagrees", fe=fee_estimate(CALM, fee=999)))
    s.append(scenario("fee_and_fees_missing", fe={"remove": ["fee", "fees"], "patch": {"quantization_mask": MASK}}))

    s.append(scenario("mask_0", fe=fee_estimate(mask=0)))
    s.append(scenario("mask_missing", fe={"patch": {"fee": CALM[0], "fees": CALM}, "remove": ["quantization_mask"]}))
    s.append(scenario("mask_1", fe=fee_estimate(mask=1), send=True))
    s.append(scenario("mask_7", fe=fee_estimate(mask=7), send=True))
    s.append(scenario("mask_10000", fe=fee_estimate(mask=10000), send=True))
    s.append(scenario("mask_12345", fe=fee_estimate(mask=12345), send=True))
    s.append(scenario("mask_1e8", fe=fee_estimate(mask=10**8), send=True))
    s.append(scenario("mask_1e12_one_xmr", fe=fee_estimate(mask=10**12), send=True))
    s.append(scenario("mask_2pow63", fe=fee_estimate(mask=2**63)))
    s.append(scenario("mask_u64_max", fe=fee_estimate(mask=U64)))

    s.append(scenario("rate_1e7_at_cap", fe=fee_estimate([10**7] * 4)))
    s.append(scenario("rate_1e7_plus_1", fe=fee_estimate([10**7 + 1] * 4)))
    s.append(scenario("rate_only_top_tier_over_cap", fe=fee_estimate([20000, 80000, 320000, 10**7 + 1])))
    s.append(scenario("rate_1e9", fe=fee_estimate([10**9] * 4)))
    s.append(scenario("rate_1e13", fe=fee_estimate([10**13] * 4)))
    s.append(scenario("rate_2pow63", fe=fee_estimate([2**63] * 4)))
    s.append(scenario("rate_2pow64_div_1024", fe=fee_estimate([2**54] * 4)))
    s.append(scenario("rate_u64_max", fe=fee_estimate([U64] * 4)))
    s.append(scenario("regtest_fresh_chain_tiers", fe=fee_estimate([1200000, 4700000, 19000000, 240000000])))
    return s


def implied_rate(fee, weight, candidates, masks):
    hits = []
    for rate in candidates:
        for mask in masks:
            if mask and (weight * rate + mask - 1) // mask * mask == fee:
                hits.append({"rate": rate, "mask": mask})
    return hits


class Run:
    def __init__(self, out_dir, only=None):
        self.out = out_dir
        os.makedirs(out_dir, exist_ok=True)
        self.results = open(f"{out_dir}/results.jsonl", "a")
        self.fake, self.ports = c.start_fake_node(f"{out_dir}/transcript.jsonl")
        self.fake_urls = [f"http://127.0.0.1:{p}" for p in self.ports]
        self.port_idx = 0
        self.wallet, self.wallet_url = c.start_wallet_rpc(
            f"127.0.0.1:{self.ports[0]}", f"{c.RUN}/wallets", os.path.basename(out_dir) + "-wallet-rpc")
        self.addresses = c.addresses()
        c.ctl(self.fake_urls[0], "setup")
        c.open_or_restore(self.wallet_url, "A", c.SEED_A)
        self.only = only

    def close(self):
        try:
            c.json_rpc(self.wallet_url, "close_wallet")
        except Exception:
            pass
        self.wallet.terminate()
        self.wallet.wait()
        self.fake.terminate()
        self.fake.wait()

    def refresh_wallet(self):
        c.ctl(self.fake_urls[0], "setup-refresh")
        c.json_rpc(self.wallet_url, "refresh")
        return c.json_rpc(self.wallet_url, "get_height")["result"]["height"]

    def record(self, rec):
        self.results.write(json.dumps(rec) + "\n")
        self.results.flush()

    def cold_wallet2(self):
        # wallet2 drops its NodeRPCProxy caches only when the daemon address changes
        # (wallet2.cpp:1403-1417), so alternate the two ports.
        self.port_idx ^= 1
        res = c.json_rpc(self.wallet_url, "set_daemon", {"address": f"127.0.0.1:{self.ports[self.port_idx]}", "ssl_support": "disabled"})
        assert "error" not in res, res

    def wallet2(self, sc, wallet_height):
        name, overrides = sc["name"], sc["overrides"]
        fe_patch = overrides.get("get_fee_estimate", {}).get("patch", {})
        candidates = sorted(set(list(fe_patch.get("fees") or []) + [fe_patch.get("fee") or 0, FEE_PER_BYTE]))
        masks = sorted(set([fe_patch.get("quantization_mask") or 0, 1, MASK]))
        c.ctl(self.fake_urls[0], f"{name}|wallet2|adjust_priority", overrides)
        self.cold_wallet2()
        t = time.time()
        res = c.json_rpc(self.wallet_url, "get_default_fee_priority")
        self.record({"scenario": name, "impl": "wallet2", "step": "adjust_priority", "wallet_height": wallet_height,
                     "priority": res.get("result", {}).get("priority"), "error": res.get("error"),
                     "secs": round(time.time() - t, 2)})
        for prio in sc.get("w2_prios", [0, 1, 2, 3, 4]):
            c.ctl(self.fake_urls[0], f"{name}|wallet2|transfer_p{prio}", overrides)
            t = time.time()
            res = c.json_rpc(self.wallet_url, "transfer", {
                "destinations": [{"address": self.addresses["B"], "amount": 10**12}],
                "priority": prio, "do_not_relay": True, "get_tx_hex": True, "account_index": 0})
            rec = {"scenario": name, "impl": "wallet2", "step": f"transfer_p{prio}", "priority_in": prio,
                   "secs": round(time.time() - t, 2)}
            if "result" in res:
                fee, weight = res["result"]["fee"], res["result"]["weight"]
                rec.update({"fee": fee, "weight": weight, "per_byte": fee / weight,
                            "implied": implied_rate(fee, weight, candidates, masks)})
            else:
                rec["error"] = res.get("error")
            self.record(rec)

    def rust(self, sc, wallet_height):
        name = sc["name"]
        for variant in ("blob", "objects"):
            overrides = copy.deepcopy(sc["overrides"])
            if variant == "objects":
                bl = overrides.setdefault("get_txpool_backlog", {})
                if "backlog" not in bl:
                    continue
                bl["backlog_format"] = "objects"
            for prio in ("automatic", "low", "normal", "elevated", "priority"):
                heights = [None, wallet_height] if prio == "automatic" else [None]
                for wh in heights:
                    c.ctl(self.fake_urls[0], f"{name}|rust-{variant}|{prio}|wallet_height={wh}", overrides)
                    res = c.probe("probe_rate", {"PROBE_NODE": self.fake_urls[0], "PROBE_PRIORITY": prio,
                                                 "PROBE_WALLET_HEIGHT": wh})
                    rec = {"scenario": name, "impl": f"rust-{variant}", "step": prio, "wallet_height": wh}
                    rec.update(res)
                    self.record(rec)

    def run(self, scs):
        wallet_height = self.refresh_wallet()
        print("wallet height", wallet_height, flush=True)
        for sc in scs:
            if self.only and not any(o in sc["name"] for o in self.only):
                continue
            t = time.time()
            self.wallet2(sc, wallet_height)
            self.rust(sc, wallet_height)
            print(f"{sc['name']}: {time.time() - t:.1f}s", flush=True)
        c.ctl(self.fake_urls[0], "idle")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=f"{c.RUN}/matrix")
    ap.add_argument("--only", action="append")
    args = ap.parse_args()
    run = Run(args.out, args.only)
    try:
        run.run(scenarios())
    finally:
        run.close()


if __name__ == "__main__":
    main()
