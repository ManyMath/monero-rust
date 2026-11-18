#!/usr/bin/env python3
"""Cross-check of w2harness against the monero-wallet-rpc binary (snapshot L).

    run_walletrpc.py select <n> | rings <batches>
"""
import json
import os
import shutil
import sys

from mrpc import RUN, start_wallet_rpc
from run_regtest import SAMPLES, SELECT_CELLS, XMR, node, wallets

CHECK_CELLS = {
    "many_small": ["one0.03", "pair0.3", "multi0.45"],
    "mixed": ["one1", "pair20"],
    "subaddr": ["multi3", "multi8"],
    "few_large": ["one1"],
    "burst": ["pair0.6"],
}


def varint(blob, pos):
    value, shift = 0, 0
    while True:
        byte = blob[pos]
        pos += 1
        value |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return value, pos
        shift += 7


def rings_of(tx_hex):
    blob = bytes.fromhex(tx_hex)
    _, pos = varint(blob, 0)  # version
    _, pos = varint(blob, pos)  # unlock time
    count, pos = varint(blob, pos)
    out = {}
    for _ in range(count):
        assert blob[pos] == 0x02
        pos += 1
        _, pos = varint(blob, pos)  # amount
        n, pos = varint(blob, pos)
        ring, total = [], 0
        for _ in range(n):
            offset, pos = varint(blob, pos)
            total += offset
            ring.append(total)
        out[blob[pos:pos + 32].hex()] = ring
        pos += 32
    return out


def transfers_of(w):
    reply = w.call("incoming_transfers", {"transfer_type": "all", "account_index": 0, "verbose": True})
    rows = []
    for account in (0, 1):
        reply = w.call("incoming_transfers", {"transfer_type": "all", "account_index": account, "verbose": True})
        rows += reply.get("transfers", [])
    rows.sort(key=lambda t: (t["block_height"], t["global_index"]))
    return [{"ev": "transfer", "i": i, "amount": t["amount"], "height": t["block_height"], "gidx": t["global_index"],
             "major": t["subaddr_index"]["major"], "minor": t["subaddr_index"]["minor"], "txid": t["tx_hash"],
             "ki": t["key_image"], "spent": int(t["spent"]), "frozen": int(t["frozen"]), "unlocked": int(t["unlocked"])}
            for i, t in enumerate(rows)]


def open_wallet(batch, name):
    ringdb = f"{RUN}/ringdb-rpc/{batch}"
    shutil.rmtree(ringdb, ignore_errors=True)
    os.makedirs(ringdb)
    wallet_dir = f"{RUN}/wallets-rpc"
    if not os.path.exists(wallet_dir):
        shutil.copytree(f"{RUN}/wallets", wallet_dir)
    proc, w = start_wallet_rpc(wallet_dir, node("L"), f"{RUN}/logs/wallet-rpc-check.log",
                               ["--shared-ringdb-dir", ringdb])
    try:
        w.call("open_wallet", {"filename": name, "password": ""})
        w.call("refresh", {"start_height": 0})
    except Exception:
        proc.terminate()
        proc.wait()
        raise
    return proc, w


def close(proc, w):
    try:
        w.call("close_wallet")
    finally:
        proc.terminate()
        proc.wait()


def sample_row(tag, k, transfers, reply):
    by_ki = {t["ki"]: t["i"] for t in transfers}
    if "tx_hash_list" in reply:
        blobs, images = reply["tx_blob_list"], reply["spent_key_images_list"]
    else:
        blobs, images = [reply["tx_blob"]], [reply["spent_key_images"]]
    txs = []
    for blob, kis in zip(blobs, images):
        rings = rings_of(blob)
        txs.append({"sel": [by_ki[ki] for ki in kis["key_images"]],
                    "inputs": [{"t": by_ki[ki], "ring": rings[ki]} for ki in kis["key_images"]]})
    return {"ev": "sample", "tag": tag, "k": k, "ok": True, "err": "", "txs": txs}


def select(n):
    w_all = wallets()
    dests = [w_all["sink"]["main"], w_all["miner"]["main"], w_all["ring"]["main"]]
    for name, tags in CHECK_CELLS.items():
        cells = {c[0]: c for c in SELECT_CELLS[name]}
        with open(f"{SAMPLES}/rpccheck.{name}.wallet2.jsonl", "w") as out:
            batch = 0
            proc, w = open_wallet(f"select-{name}-{batch}", name)
            transfers = transfers_of(w)
            height = w.call("get_height")["height"]
            out.write(json.dumps({"ev": "restored", "height": height, "n_transfers": len(transfers)}) + "\n")
            for t in transfers:
                out.write(json.dumps(t) + "\n")
            for tag in tags:
                _, amounts, prio, account = cells[tag]
                for k in range(n):
                    try:
                        reply = w.call("transfer", {
                            "destinations": [{"address": dests[i], "amount": int(round(a * XMR))} for i, a in enumerate(amounts)],
                            "account_index": account, "priority": prio, "do_not_relay": True, "get_tx_hex": True})
                        out.write(json.dumps(sample_row(tag, k, transfers, reply)) + "\n")
                    except RuntimeError as err:
                        out.write(json.dumps({"ev": "sample", "tag": tag, "k": k, "ok": False, "err": str(err), "txs": []}) + "\n")
            close(proc, w)
        with open(f"{SAMPLES}/select.{name}.wallet2.jsonl") as src, open(f"{SAMPLES}/rpccheck.{name}.rust.jsonl", "w") as dst:
            for line in src:
                row = json.loads(line)
                if row.get("ev") != "sample" or row["tag"] in tags:
                    row.pop("trace", None)
                    dst.write(json.dumps(row) + "\n")


def rings(batches):
    w_all = wallets()
    with open(f"{SAMPLES}/rpccheck-rings.walletrpc.jsonl", "w") as out:
        for batch in range(batches):
            proc, w = open_wallet(f"rings-{batch}", "ring")
            try:
                transfers = transfers_of(w)
                for t in transfers:
                    if not t["unlocked"] or t["spent"]:
                        continue
                    reply = w.call("sweep_single", {"address": w_all["sink"]["main"], "key_image": t["ki"],
                                                    "do_not_relay": True, "get_tx_hex": True})
                    ring = rings_of(reply["tx_blob"])[t["ki"]]
                    out.write(json.dumps({"ev": "sample", "batch": batch, "gidx": t["gidx"], "ring": ring}) + "\n")
            finally:
                close(proc, w)
            print("batch", batch, flush=True)


if __name__ == "__main__":
    os.makedirs(SAMPLES, exist_ok=True)
    {"select": select, "rings": rings}[sys.argv[1]](int(sys.argv[2]))
