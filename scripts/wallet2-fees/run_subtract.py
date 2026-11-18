#!/usr/bin/env python3
"""How wallet2 spreads a fee over destinations with subtract_fee_from_outputs,
against monero_rust::adjust_recipients_for_fee."""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common as c
import run_matrix as m

OUT = f"{c.RUN}/subtract"
XMR = 10**12


def rust_split(amounts, fee):
    share, rest = divmod(fee, len(amounts))
    return [a - share - (rest if i == 0 else 0) for i, a in enumerate(amounts)]


def main():
    run = m.Run(OUT)
    try:
        run.refresh_wallet()
        dests = [run.addresses["B"]]
        for _ in range(2):
            dests.append(c.json_rpc(run.wallet_url, "create_address", {"account_index": 0})["result"]["address"])
        seen = set()
        for rate in range(20001, 20012):
            overrides = m.scenario("x", fe=m.fee_estimate([rate, rate * 4, rate * 16, rate * 200], mask=1))["overrides"]
            c.ctl(run.fake_urls[0], f"subtract_rate_{rate}|wallet2|transfer", overrides)
            run.cold_wallet2()
            res = c.json_rpc(run.wallet_url, "transfer", {
                "destinations": [{"address": d, "amount": XMR} for d in dests],
                "subtract_fee_from_outputs": [0, 1, 2], "priority": 1, "do_not_relay": True, "account_index": 0})
            if "result" not in res:
                run.record({"rate": rate, "error": res.get("error")})
                continue
            fee = res["result"]["fee"]
            got = res["result"]["amounts_by_dest"]["amounts"]
            rec = {"rate": rate, "fee": fee, "fee_mod_3": fee % 3, "weight": res["result"]["weight"],
                   "wallet2_amounts": got, "wallet2_subtracted": [XMR - a for a in got],
                   "rust_adjust_recipients_for_fee": rust_split([XMR] * 3, fee),
                   "rust_subtracted": [XMR - a for a in rust_split([XMR] * 3, fee)]}
            rec["same"] = sorted(rec["wallet2_subtracted"]) == sorted(rec["rust_subtracted"])
            run.record(rec)
            print(json.dumps(rec), flush=True)
            seen.add(fee % 3)
            if seen == {0, 1, 2}:
                break
    finally:
        run.close()


if __name__ == "__main__":
    main()
