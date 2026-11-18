#!/usr/bin/env python3
"""Ask a real wallet2 (monero-wallet-rpc) which inputs it selects.

Usage, on a fresh chain:
  eval "$(scripts/regtest-node.sh start)"
  monero-wallet-rpc --wallet-dir <dir> --disable-rpc-login \
    --rpc-bind-ip 127.0.0.1 --rpc-bind-port <port> \
    --daemon-address "${MONERO_REGTEST_RPC#http://}" --trusted-daemon \
    --allow-mismatched-daemon-version --shared-ringdb-dir <dir>/ringdb &
  WALLET_RPC=http://127.0.0.1:<port> \
    scripts/wallet2_selection_probe.py <results.json> <state.json>

tests/wallet2_selection_regtest_test.rs reads <state.json> through WALLET2_SELECTION_STATE.
"""

import json
import os
import sys
import urllib.request

DAEMON = os.environ["MONERO_REGTEST_RPC"]
WALLET_RPC = os.environ["WALLET_RPC"]
XMR = 10**12
SPACING = 12
REPEAT = 12


def post(url, payload):
    request = urllib.request.Request(
        url, data=json.dumps(payload).encode(), headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(request, timeout=600) as response:
        return json.loads(response.read())


def rpc(base, method, params=None, allow_error=False):
    reply = post(
        base + "/json_rpc", {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}}
    )
    if "error" in reply:
        if allow_error:
            return {"error": reply["error"]}
        raise RuntimeError(f"{method}: {reply['error']}")
    return reply["result"]


def wallet(method, params=None, allow_error=False):
    return rpc(WALLET_RPC, method, params, allow_error)


def daemon(method, params=None):
    return rpc(DAEMON, method, params)


def open_wallet(name):
    wallet("close_wallet", allow_error=True)
    reply = wallet("open_wallet", {"filename": name, "password": ""}, allow_error=True)
    if "error" in reply:
        wallet("create_wallet", {"filename": name, "password": "", "language": "English"})
    wallet("refresh")


def mine(address, blocks):
    daemon("generateblocks", {"amount_of_blocks": blocks, "wallet_address": address})


def height():
    return daemon("get_block_count")["count"]


def main():
    out_path, state_path = sys.argv[1], sys.argv[2]
    result = {"daemon_version": post(DAEMON + "/get_info", {})["version"], "scenarios": {}}

    plan = {
        "same_tx": [(0, [(0, 3 * XMR), (0, XMR // 2)])],
        "adjacent_block": [(0, [(0, 3 * XMR)]), ("0b", [(0, XMR // 2)])],
        "unrelated": [(0, [(0, 3 * XMR)]), (1, [(0, XMR // 2)]), (2, [(0, XMR // 100)])],
        "other_subaddress": [(0, [(0, 3 * XMR)]), (1, [(5, XMR // 10)])],
        "reservation": [(0, [(0, 3 * XMR)]), (1, [(0, 5 * XMR // 2)])],
        "dust": [(0, [(0, 3 * XMR)]), (1, [(0, 5000)])],
        "subaddress_order": [
            (stage, [(1 if stage % 2 == 0 else 2, XMR if stage % 2 == 0 else 9 * XMR // 10)])
            for stage in range(8)
        ],
        "same_tx_pair": [(0, [(0, XMR), (0, XMR)]), (1, [(0, XMR // 10)])],
        "single_threshold": [(0, [(0, XMR)]), ("0b", [(0, 10 * XMR)])],
        "sweep": [(0, [(0, XMR)]), (1, [(3, XMR)]), (2, [(7, XMR)])],
        "sweep_dust": [(0, [(0, XMR)]), (1, [(0, 5000)])],
        "zero_change_two": [(0, [(0, XMR)])],
        "self_subaddress": [(0, [(100, 3 * XMR)])],
    }

    addresses = {}
    seeds = {}
    for name in plan:
        open_wallet("w_" + name)
        seeds["w_" + name] = wallet("query_key", {"key_type": "mnemonic"})["key"]
        wallet("create_address", {"account_index": 0, "count": 8})
        listed = wallet("get_address", {"account_index": 0})["addresses"]
        addresses[name] = {entry["address_index"]: entry["address"] for entry in listed}
        if len(wallet("get_accounts")["subaddress_accounts"]) < 2:
            wallet("create_account", {})
        addresses[name][100] = wallet("get_address", {"account_index": 1})["address"]

    open_wallet("miner")
    miner_address = wallet("get_address", {"account_index": 0})["address"]
    if height() < 200:
        mine(miner_address, 200 - height())

    def fund(stage):
        open_wallet("miner")
        sent = 0
        for name, entries in plan.items():
            for entry_stage, outputs in entries:
                if entry_stage != stage:
                    continue
                destinations = [
                    {"address": addresses[name][minor], "amount": amount}
                    for minor, amount in outputs
                ]
                wallet("transfer", {"destinations": destinations, "account_index": 0})
                sent += 1
        return sent

    marker = os.path.join(os.path.dirname(os.path.abspath(state_path)), "funded")
    if not os.path.exists(marker):
        funded_from = height() - 1
        for stage in range(8):
            fund(stage)
            mine(miner_address, 1)
            if stage == 0:
                fund("0b")
                mine(miner_address, 1)
            mine(miner_address, SPACING)
        mine(miner_address, 12)
        state = {
            "recipient": miner_address,
            "scan_from": funded_from,
            "scan_to": height() - 1,
            "wallets": seeds,
        }
        with open(state_path, "w") as handle:
            json.dump(state, handle, indent=1)
        open(marker, "w").write(str(height()))

    def owned(account=0):
        reply = wallet(
            "incoming_transfers", {"transfer_type": "all", "account_index": account, "verbose": True}
        )
        return {
            transfer["key_image"]: {
                "amount": transfer["amount"],
                "minor": transfer["subaddr_index"]["minor"],
                "height": transfer["block_height"],
                "tx": transfer["tx_hash"][:8],
            }
            for transfer in reply.get("transfers", [])
        }

    def describe(outputs, key_images):
        picked = [outputs[key_image] for key_image in key_images]
        return sorted(
            ([entry["amount"], entry["minor"], entry["height"], entry["tx"]] for entry in picked)
        )

    def transfers(name, amount, extra=None):
        open_wallet("w_" + name)
        outputs = owned()
        runs = []
        for _ in range(REPEAT):
            params = {
                "destinations": [{"address": miner_address, "amount": amount}],
                "account_index": 0,
                "do_not_relay": True,
            }
            params.update(extra or {})
            reply = wallet("transfer", params, allow_error=True)
            if "error" in reply:
                runs.append({"error": reply["error"]["message"]})
                continue
            runs.append(
                {
                    "inputs": describe(outputs, reply["spent_key_images"]["key_images"]),
                    "fee": reply["fee"],
                    "weight": reply["weight"],
                }
            )
        distinct = sorted({json.dumps(run, sort_keys=True) for run in runs})
        return {
            "wallet_outputs": sorted(
                [entry["amount"], entry["minor"], entry["height"], entry["tx"]]
                for entry in outputs.values()
            ),
            "amount": amount,
            "runs": len(runs),
            "distinct_results": [json.loads(entry) for entry in distinct],
        }

    scenarios = result["scenarios"]
    for name in ["same_tx", "adjacent_block", "unrelated", "other_subaddress", "reservation", "dust"]:
        scenarios[name] = transfers(name, XMR)
    scenarios["subaddress_order"] = transfers("subaddress_order", 5 * XMR // 2)
    scenarios["same_tx_pair"] = transfers("same_tx_pair", 3 * XMR // 2)
    scenarios["zero_amount"] = transfers("unrelated", 0)

    probe = transfers("single_threshold", XMR // 10)
    first = probe["distinct_results"][0]
    per_byte = first["fee"] / first["weight"]
    scenarios["single_threshold_probe"] = probe
    scenarios["single_threshold"] = transfers("single_threshold", XMR - int(1850 * per_byte))
    scenarios["single_threshold"]["per_byte"] = per_byte

    def read_varint(blob, position):
        value, shift = 0, 0
        while True:
            byte = blob[position]
            position += 1
            value |= (byte & 0x7F) << shift
            shift += 7
            if byte < 0x80:
                return value, position

    def tx_shape(tx_hex):
        blob = bytes.fromhex(tx_hex)
        _, position = read_varint(blob, 0)  # version
        _, position = read_varint(blob, position)  # unlock time
        inputs, position = read_varint(blob, position)
        for _ in range(inputs):
            assert blob[position] == 0x02
            _, position = read_varint(blob, position + 1)  # amount
            members, position = read_varint(blob, position)
            for _ in range(members):
                _, position = read_varint(blob, position)
            position += 32
        outputs, position = read_varint(blob, position)
        for _ in range(outputs):
            _, position = read_varint(blob, position)  # amount
            kind = blob[position]
            position += 1 + 32 + (1 if kind == 0x03 else 0)
        extra_len, position = read_varint(blob, position)
        extra = blob[position : position + extra_len]
        fields, cursor = [], 0
        while cursor < len(extra):
            tag = extra[cursor]
            if tag == 0x01:
                fields.append("pubkey")
                cursor += 33
            elif tag == 0x02:
                length, after = read_varint(extra, cursor + 1)
                fields.append(f"nonce[{length}]")
                cursor = after + length
            elif tag == 0x04:
                count, after = read_varint(extra, cursor + 1)
                fields.append(f"additional[{count}]")
                cursor = after + 32 * count
            else:
                fields.append(f"tag{tag:#x}")
                break
        return {"inputs": inputs, "outputs": outputs, "extra_len": extra_len, "extra": fields}

    def shape(name, account, destinations, extra=None):
        open_wallet("w_" + name)
        params = {
            "destinations": destinations,
            "account_index": account,
            "do_not_relay": True,
            "get_tx_hex": True,
        }
        params.update(extra or {})
        reply = wallet("transfer", params, allow_error=True)
        if "error" in reply:
            return {"error": reply["error"]["message"]}
        described = tx_shape(reply["tx_blob"])
        described["fee"] = reply["fee"]
        described["amount"] = reply["amount"]
        return described

    other = addresses["dust"][0]
    scenarios["shape_one_destination_with_change"] = shape(
        "zero_change_two", 0, [{"address": miner_address, "amount": XMR // 2}]
    )
    scenarios["shape_two_destinations_zero_change"] = shape(
        "zero_change_two",
        0,
        [{"address": miner_address, "amount": XMR // 2}, {"address": other, "amount": XMR // 2}],
        {"subtract_fee_from_outputs": [0, 1]},
    )
    scenarios["shape_one_destination_zero_change"] = shape(
        "zero_change_two",
        0,
        [{"address": miner_address, "amount": XMR}],
        {"subtract_fee_from_outputs": [0]},
    )
    own_change = addresses["self_subaddress"][100]
    scenarios["shape_pay_own_change_subaddress_and_standard"] = shape(
        "self_subaddress",
        1,
        [{"address": own_change, "amount": XMR // 2}, {"address": miner_address, "amount": XMR // 2}],
    )
    scenarios["shape_same_subaddress_twice"] = shape(
        "self_subaddress",
        1,
        [
            {"address": addresses["dust"][3], "amount": XMR // 2},
            {"address": addresses["dust"][3], "amount": XMR // 2},
        ],
    )
    scenarios["shape_zero_amount_second_destination"] = shape(
        "self_subaddress",
        1,
        [{"address": miner_address, "amount": XMR // 2}, {"address": other, "amount": 0}],
    )

    def sweeps(name, params):
        open_wallet("w_" + name)
        outputs = owned()
        runs = []
        for _ in range(REPEAT):
            request = {"address": miner_address, "account_index": 0, "do_not_relay": True}
            request.update(params)
            reply = wallet("sweep_all", request, allow_error=True)
            if "error" in reply:
                runs.append({"error": reply["error"]["message"]})
                continue
            runs.append(
                {
                    "transactions": [
                        describe(outputs, entry["key_images"])
                        for entry in reply["spent_key_images_list"]
                    ]
                }
            )
        distinct = sorted({json.dumps(run, sort_keys=True) for run in runs})
        return {
            "wallet_outputs": sorted(
                [entry["amount"], entry["minor"], entry["height"], entry["tx"]]
                for entry in outputs.values()
            ),
            "params": params,
            "runs": len(runs),
            "distinct_results": [json.loads(entry) for entry in distinct],
        }

    scenarios["sweep_default"] = sweeps("sweep", {})
    scenarios["sweep_all_indices"] = sweeps("sweep", {"subaddr_indices_all": True})
    scenarios["sweep_dust"] = sweeps("sweep_dust", {"subaddr_indices": [0]})

    wallet("close_wallet", allow_error=True)
    with open(out_path, "w") as handle:
        json.dump(result, handle, indent=1)
    print(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
