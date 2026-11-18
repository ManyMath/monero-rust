#!/usr/bin/env python3
"""Second pass over the fake node: stale wallet height, cold transcripts and whole
transactions. Run after run_real.py."""

import copy
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common as c
import run_matrix as m

OUT = f"{c.RUN}/extra"
XMR = 10**12
ODD = [20001, 80003, 320007, 4000009]


def quantized(weight, rate, mask):
    return (weight * rate + mask - 1) // mask * mask


class Extra(m.Run):
    def stale(self):
        wallet_height = self.refresh_wallet()
        res = c.json_rpc(c.daemon_url(), "generateblocks",
                         {"amount_of_blocks": 30, "wallet_address": self.addresses["A"]})
        daemon_height = res["result"]["height"] + 1
        print("wallet", wallet_height, "daemon", daemon_height, flush=True)
        split_full_tip = {"block_weight_split": {"height": wallet_height, "below": 1000, "at_or_above": m.FRZ}}
        split_full_old = {"block_weight_split": {"height": wallet_height, "below": m.FRZ, "at_or_above": 1000}}
        scs = [
            m.scenario("stale_tip_full_wallet_blocks_empty", headers=split_full_tip),
            m.scenario("stale_tip_empty_wallet_blocks_full", headers=split_full_old),
            m.scenario("wallet_ahead_of_node", headers={"fail": "jsonrpc_error"},
                       info={"patch": {"height": wallet_height - 50, "target_height": 0}}),
        ]
        for sc in scs:
            name, overrides = sc["name"], sc["overrides"]
            c.ctl(self.fake_urls[0], f"{name}|wallet2|adjust_priority", overrides)
            self.cold_wallet2()
            res = c.json_rpc(self.wallet_url, "get_default_fee_priority")
            self.record({"scenario": name, "impl": "wallet2", "step": "adjust_priority",
                         "wallet_height": wallet_height, "daemon_height": daemon_height,
                         "priority": res.get("result", {}).get("priority"), "error": res.get("error")})
            for variant in ("blob", "objects"):
                ov = copy.deepcopy(overrides)
                if variant == "objects":
                    ov["get_txpool_backlog"]["backlog_format"] = "objects"
                for wh in (None, wallet_height, wallet_height - 1):
                    c.ctl(self.fake_urls[0], f"{name}|rust-{variant}|automatic|wallet_height={wh}", ov)
                    res = c.probe("probe_rate", {"PROBE_NODE": self.fake_urls[0], "PROBE_PRIORITY": "automatic",
                                                 "PROBE_WALLET_HEIGHT": wh})
                    rec = {"scenario": name, "impl": f"rust-{variant}", "step": "automatic", "wallet_height": wh,
                           "daemon_height": daemon_height}
                    rec.update(res)
                    self.record(rec)
        return self.refresh_wallet()

    def cold_transcripts(self):
        calm = m.scenario("cold")["overrides"]
        for prio in (0, 1, 2):
            c.ctl(self.fake_urls[0], f"cold|wallet2|transfer_p{prio}_cold_cache", calm)
            self.cold_wallet2()
            c.json_rpc(self.wallet_url, "transfer", {
                "destinations": [{"address": self.addresses["B"], "amount": XMR}],
                "priority": prio, "do_not_relay": True, "account_index": 0})
            c.ctl(self.fake_urls[0], f"cold|wallet2|transfer_p{prio}_again_within_30s", calm)
            c.json_rpc(self.wallet_url, "transfer", {
                "destinations": [{"address": self.addresses["B"], "amount": XMR}],
                "priority": prio, "do_not_relay": True, "account_index": 0})

    def sends(self, wallet_height, daemon_height, outputs_file):
        scs = [
            ("send_calm", m.fee_estimate()),
            ("send_odd_rates", m.fee_estimate(ODD)),
            ("send_mask_1", m.fee_estimate(ODD, mask=1)),
            ("send_mask_7", m.fee_estimate(ODD, mask=7)),
            ("send_mask_12345", m.fee_estimate(ODD, mask=12345)),
            ("send_mask_1e8", m.fee_estimate(ODD, mask=10**8)),
            ("send_mask_1e12_one_xmr", m.fee_estimate(ODD, mask=10**12)),
            ("send_mask_0", m.fee_estimate(ODD, mask=0)),
            ("send_mask_missing", {"patch": {"fee": ODD[0], "fees": ODD}, "remove": ["quantization_mask"]}),
            ("send_rate_at_cap_1e7", m.fee_estimate([10**7] * 4)),
        ]
        names = ["automatic", "low", "normal", "elevated", "priority"]
        for name, fe in scs:
            overrides = m.scenario(name, fe=fe)["overrides"]
            fees = fe["patch"]["fees"]
            mask = fe["patch"].get("quantization_mask", None)
            c.ctl(self.fake_urls[0], f"{name}|wallet2|cold", overrides)
            self.cold_wallet2()
            for prio in range(5):
                rate = fees[max(prio, 1) - 1]
                c.ctl(self.fake_urls[0], f"{name}|wallet2|transfer_p{prio}", overrides)
                res = c.json_rpc(self.wallet_url, "transfer", {
                    "destinations": [{"address": self.addresses["B"], "amount": XMR}],
                    "priority": prio, "do_not_relay": True, "get_tx_hex": True, "account_index": 0})
                rec = {"scenario": name, "impl": "wallet2", "step": names[prio], "rate": rate, "mask": mask}
                if "result" in res:
                    r = res["result"]
                    weighed = c.probe("probe_weigh", {"PROBE_TX_HEX": r["tx_blob"]})
                    rec.update({"fee": r["fee"], "weight": r["weight"], "weighed": weighed.get("weight"),
                                "inputs": weighed.get("inputs"), "outputs": weighed.get("outputs"),
                                "extra_len": weighed.get("extra_len")})
                    for label, msk in (("mask", mask), ("mask_1", 1)):
                        if msk:
                            rec[f"fee_matches_total_rounded_to_{label}"] = r["fee"] == quantized(r["weight"], rate, msk)
                    if mask:
                        per_byte_rounded = quantized(1, rate, mask) * r["weight"]
                        rec["fee_if_rate_were_rounded"] = per_byte_rounded
                else:
                    rec["error"] = res.get("error")
                self.record(rec)

                c.ctl(self.fake_urls[0], f"{name}|rust|create_transaction_{names[prio]}", overrides)
                res = c.probe("probe_send", {
                    "PROBE_NODE": self.fake_urls[0], "PROBE_SEED": c.SEED_A, "PROBE_DEST": self.addresses["B"],
                    "PROBE_AMOUNT": XMR, "PROBE_DAEMON_HEIGHT": daemon_height, "PROBE_OUTPUTS_FILE": outputs_file,
                    "PROBE_INPUTS": 2, "PROBE_PRIORITY": names[prio], "PROBE_WALLET_HEIGHT": wallet_height})
                res.pop("tx_blob", None)
                rec = {"scenario": name, "impl": "rust", "step": names[prio], "rate": rate, "mask": mask}
                rec.update(res)
                if res.get("ok") and mask:
                    rec["fee_matches_total_rounded_to_mask"] = res["fee"] == quantized(res["weight"], rate, mask)
                    rec["fee_if_rate_were_rounded"] = quantized(1, rate, mask) * res["weight"]
                self.record(rec)

        U64 = 2**64 - 1
        for name, fe, cap in (
            ("overflow_mask_2pow63", m.fee_estimate(mask=2**63), None),
            ("overflow_mask_u64_max", m.fee_estimate(mask=U64), None),
            ("overflow_rate_2pow63_cap_removed", m.fee_estimate([2**63] * 4), U64),
            ("overflow_rate_u64_max_cap_removed", m.fee_estimate([U64] * 4), U64),
            ("overflow_rate_and_mask_u64_max_cap_removed", m.fee_estimate([U64] * 4, mask=U64), U64),
            ("overflow_rate_1e9_cap_removed", m.fee_estimate([10**9] * 4), U64),
        ):
            overrides = m.scenario(name, fe=fe)["overrides"]
            for test in ("probe_estimate", "probe_send"):
                c.ctl(self.fake_urls[0], f"{name}|rust|{test}", overrides)
                res = c.probe(test, {
                    "PROBE_NODE": self.fake_urls[0], "PROBE_SEED": c.SEED_A, "PROBE_DEST": self.addresses["B"],
                    "PROBE_AMOUNT": XMR, "PROBE_DAEMON_HEIGHT": daemon_height, "PROBE_OUTPUTS_FILE": outputs_file,
                    "PROBE_INPUTS": 2, "PROBE_PRIORITY": "low", "PROBE_MAX_FEE": cap})
                res.pop("tx_blob", None)
                rec = {"scenario": name, "impl": "rust", "step": test, "max_fee_per_weight": cap}
                rec.update(res)
                self.record(rec)

        calm = m.scenario("flow")["overrides"]
        calm["get_txpool_backlog"]["backlog_format"] = "objects"
        for label, ov in (("monerod_wire", m.scenario("flow")["overrides"]), ("object_backlog", calm)):
            c.ctl(self.fake_urls[0], f"flow|rust-{label}|prepare_send_inputs_with_rpc+create_transaction", ov)
            res = c.probe("probe_send", {
                "PROBE_NODE": self.fake_urls[0], "PROBE_SEED": c.SEED_A, "PROBE_DEST": self.addresses["B"],
                "PROBE_AMOUNT": XMR, "PROBE_DAEMON_HEIGHT": daemon_height, "PROBE_OUTPUTS_FILE": outputs_file,
                "PROBE_PREPARE": 1, "PROBE_PRIORITY": "automatic", "PROBE_WALLET_HEIGHT": wallet_height})
            res.pop("tx_blob", None)
            rec = {"scenario": "flow", "impl": f"rust-{label}", "step": "prepare+create"}
            rec.update(res)
            self.record(rec)
        c.ctl(self.fake_urls[0], "flow|wallet2|transfer_p0_cold_cache", m.scenario("flow")["overrides"])
        self.cold_wallet2()
        c.json_rpc(self.wallet_url, "transfer", {
            "destinations": [{"address": self.addresses["B"], "amount": XMR}],
            "priority": 0, "do_not_relay": True, "account_index": 0})
        c.ctl(self.fake_urls[0], "idle")


def main():
    run = Extra(OUT)
    try:
        wallet_height = run.stale()
        run.cold_transcripts()
        daemon_height = c.json_rpc(c.daemon_url(), "get_info")["result"]["height"]
        run.sends(wallet_height, daemon_height, f"{c.RUN}/real/rust-outputs-A.json")
    finally:
        run.close()


if __name__ == "__main__":
    main()
