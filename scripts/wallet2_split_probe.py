#!/usr/bin/env python3
"""Ask a real wallet2 how it splits a payment that needs many inputs.

Usage, on the chain and wallet-rpc of wallet2_selection_probe.py, after it:
  WALLET_RPC=http://127.0.0.1:<port> \
    scripts/wallet2_split_probe.py <results.json> <state.json>
"""

import json
import os
import sys
import urllib.request

DAEMON = os.environ["MONERO_REGTEST_RPC"]
WALLET_RPC = os.environ["WALLET_RPC"]
XMR = 10**12


def rpc(base, method, params=None, allow_error=False):
    payload = {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}}
    request = urllib.request.Request(
        base + "/json_rpc",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=900) as response:
        reply = json.loads(response.read())
    if "error" in reply:
        if allow_error:
            return {"error": reply["error"]}
        raise RuntimeError(f"{method}: {reply['error']}")
    return reply["result"]


def wallet(method, params=None, allow_error=False):
    return rpc(WALLET_RPC, method, params, allow_error)


def open_wallet(name):
    wallet("close_wallet", allow_error=True)
    reply = wallet("open_wallet", {"filename": name, "password": ""}, allow_error=True)
    if "error" in reply:
        wallet("create_wallet", {"filename": name, "password": "", "language": "English"})
    wallet("refresh")


def mine(address, blocks):
    rpc(DAEMON, "generateblocks", {"amount_of_blocks": blocks, "wallet_address": address})


def main():
    out_path, state_path = sys.argv[1], sys.argv[2]
    open_wallet("w_many")
    target = wallet("get_address", {"account_index": 0})["address"]
    seed = wallet("query_key", {"key_type": "mnemonic"})["key"]
    open_wallet("miner")
    miner_address = wallet("get_address", {"account_index": 0})["address"]

    marker = os.path.join(os.path.dirname(os.path.abspath(state_path)), "funded_many")
    if not os.path.exists(marker):
        funded_from = rpc(DAEMON, "get_block_count")["count"] - 1
        for _ in range(11):
            destinations = [{"address": target, "amount": XMR // 10}] * 15
            wallet("transfer", {"destinations": destinations, "account_index": 0})
            mine(miner_address, 1)
            wallet("refresh")
        mine(miner_address, 12)
        with open(state_path) as handle:
            state = json.load(handle)
        state["wallets"]["w_many"] = seed
        state["many_from"] = funded_from
        state["many_to"] = rpc(DAEMON, "get_block_count")["count"] - 1
        with open(state_path, "w") as handle:
            json.dump(state, handle, indent=1)
        open(marker, "w").write("done")

    open_wallet("w_many")
    balance = wallet("get_balance", {"account_index": 0})
    outputs = wallet("incoming_transfers", {"transfer_type": "available", "account_index": 0})
    result = {
        "unlocked_balance": balance["unlocked_balance"],
        "outputs": len(outputs.get("transfers", [])),
    }

    destination = [{"address": miner_address, "amount": 31 * XMR // 2}]
    single = wallet(
        "transfer",
        {"destinations": destination, "account_index": 0, "do_not_relay": True},
        allow_error=True,
    )
    result["transfer"] = (
        {"error": single["error"]["message"]}
        if "error" in single
        else {"inputs": len(single["spent_key_images"]["key_images"]), "weight": single["weight"]}
    )

    split = wallet(
        "transfer_split",
        {"destinations": destination, "account_index": 0, "do_not_relay": True},
        allow_error=True,
    )
    result["transfer_split"] = (
        {"error": split["error"]["message"]}
        if "error" in split
        else {
            "inputs_per_tx": [len(entry["key_images"]) for entry in split["spent_key_images_list"]],
            "weights": split["weight_list"],
            "amounts": split["amount_list"],
        }
    )

    sweep = wallet(
        "sweep_all",
        {
            "address": miner_address,
            "account_index": 0,
            "subaddr_indices": [0],
            "do_not_relay": True,
        },
        allow_error=True,
    )
    result["sweep_all"] = (
        {"error": sweep["error"]["message"]}
        if "error" in sweep
        else {
            "inputs_per_tx": [len(entry["key_images"]) for entry in sweep["spent_key_images_list"]],
            "weights": sweep["weight_list"],
        }
    )

    wallet("close_wallet", allow_error=True)
    with open(out_path, "w") as handle:
        json.dump(result, handle, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
