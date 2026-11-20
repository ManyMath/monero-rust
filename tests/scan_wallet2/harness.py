#!/usr/bin/env python3
"""Diff wallet2 (monero-wallet-rpc on regtest) against monero-rust's scan.

    harness.py setup | history | w2dump F | diff W R

Environment: DAEMON, WRPC_A, WRPC_B. Wallet A mines and sends; wallet B,
restored from SEED, is under test.
"""
import json, os, sys, time, urllib.request

DAEMON = os.environ["DAEMON"]
WRPC_A = os.environ["WRPC_A"]
WRPC_B = os.environ["WRPC_B"]
SEED = ("hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders "
        "cavernous second brunt saved richly lower upgrade hitched launching deepest "
        "mostly playful layout lower eden")
XMR = 10**12


def post(url, body):
    req = urllib.request.Request(url, json.dumps(body).encode(), {"Content-Type": "application/json"})
    return json.loads(urllib.request.urlopen(req, timeout=600).read())


def rpc(url, method, params=None):
    r = post(url + "/json_rpc", {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}})
    if "error" in r:
        raise RuntimeError(f"{method}: {r['error']}")
    return r["result"]


def a(method, params=None): return rpc(WRPC_A, method, params)
def b(method, params=None): return rpc(WRPC_B, method, params)
def d(method, params=None): return rpc(DAEMON, method, params)


def mine(n, address):
    d("generateblocks", {"amount_of_blocks": n, "wallet_address": address})
    a("refresh"); b("refresh")


def height():
    return post(DAEMON + "/get_height", {})["height"]


def addr(w, account=0, index=0):
    return w("get_address", {"account_index": account, "address_index": [index]})["addresses"][0]["address"]


def send(w, dests, account=0):
    r = w("transfer", {"destinations": [{"address": x, "amount": y} for x, y in dests],
                        "account_index": account, "priority": 1})
    return r["tx_hash"]


def setup():
    try:
        a("create_wallet", {"filename": "a", "password": "", "language": "English"})
    except RuntimeError:
        a("open_wallet", {"filename": "a", "password": ""})
    try:
        b("restore_deterministic_wallet", {"filename": "b", "seed": SEED, "password": "", "restore_height": 0})
    except RuntimeError:
        b("open_wallet", {"filename": "b", "password": ""})
    A, B = addr(a), addr(b)
    mine(4, B)
    mine(120, A)
    print("height", height(), "A", A, "B", B)


def history():
    A, B = addr(a), addr(b)
    for _ in range(2):
        b("create_account")
    for acct, n in ((0, 6), (1, 3), (2, 1)):
        for _ in range(n):
            b("create_address", {"account_index": acct})
    send(a, [(B, 3 * XMR)])
    send(a, [(addr(b, 0, 1), 2 * XMR), (addr(b, 0, 5), 1 * XMR)])
    mine(1, A)
    send(a, [(addr(b, 1, 0), 4 * XMR), (addr(b, 1, 3), 5 * XMR), (addr(b, 2, 0), 6 * XMR)])
    send(a, [(B, 7 * XMR), (B, 8 * XMR)])
    mine(12, A)
    send(b, [(A, 1 * XMR)])
    mine(1, A)
    send(b, [(addr(b, 0, 2), int(0.5 * XMR))])
    send(b, [(addr(b, 0, 0), 1 * XMR)], account=1)
    mine(11, A)
    b("sweep_all", {"address": addr(b, 0, 3), "account_index": 2, "priority": 1})
    mine(3, A)
    send(a, [(B, 9 * XMR)])
    send(b, [(A, int(0.25 * XMR))])
    mine(3, A)
    print("before reorg", height(), d("get_block_header_by_height", {"height": height() - 1})["block_header"]["hash"])
    post(DAEMON + "/pop_blocks", {"nblocks": 3})
    d("flush_txpool")
    mine(5, B)
    print("after reorg", height())
    mine(11, A)
    send(a, [(addr(b, 0, 4), 11 * XMR)])
    send(b, [(A, int(0.125 * XMR))])
    time.sleep(1)
    a("refresh"); b("refresh")
    print("final height", height())


def w2dump(path):
    b("refresh")
    out = {"height": b("get_height")["height"], "transfers": [], "balance": {}}
    accounts = b("get_accounts")["subaddress_accounts"]
    for acct in accounts:
        i = acct["account_index"]
        r = b("incoming_transfers", {"transfer_type": "all", "account_index": i, "verbose": True})
        out["transfers"] += r.get("transfers", [])
    for strict in (False, True):
        out["balance"]["strict" if strict else "nonstrict"] = b(
            "get_balance", {"account_index": 0, "all_accounts": True, "strict": strict})
    out["get_transfers"] = b("get_transfers", {"in": True, "out": True, "pending": True, "pool": True,
                                               "failed": True, "all_accounts": True})
    json.dump(out, open(path, "w"), indent=1)
    print("wallet2 height", out["height"], "transfers", len(out["transfers"]))


def diff(w2path, rspath):
    w2 = json.load(open(w2path)); rs = json.load(open(rspath))
    problems = []
    if w2["height"] != rs["chain_length"]:
        problems.append(f"height: wallet2 {w2['height']} rust {rs['chain_length']}")
    w2t = {t["pubkey"]: t for t in w2["transfers"]}
    rst = {o["key"]: o for o in rs["outputs"]}
    for key in sorted(set(w2t) | set(rst)):
        t, o = w2t.get(key), rst.get(key)
        if t is None:
            problems.append(f"only rust: {key} amount {o['amount']} h {o['block_height']} tx {o['tx_hash']}")
            continue
        if o is None:
            problems.append(f"only wallet2: {key} amount {t['amount']} h {t['block_height']} tx {t['tx_hash']}")
            continue
        sub = o["subaddress_index"] or [0, 0]
        checks = [
            ("amount", t["amount"], o["amount"]),
            ("tx_hash", t["tx_hash"], o["tx_hash"]),
            ("block_height", t["block_height"], o["block_height"]),
            ("global_index", t["global_index"], o["global_index"]),
            ("subaddr", [t["subaddr_index"]["major"], t["subaddr_index"]["minor"]], list(sub)),
            ("spent", t["spent"], o["spent"]),
            ("unlocked", t["unlocked"], o["unlocked_at_chain_length"]),
            ("unlocked(tip height)", t["unlocked"], o["unlocked_at_tip_height"]),
            ("frozen", t["frozen"], o["frozen"]),
        ]
        if o["key_image"]:
            checks.append(("key_image", t["key_image"], o["key_image"]))
        for name, x, y in checks:
            if x != y:
                problems.append(f"{name}: {key[:16]} h {t['block_height']} amount {t['amount']}: wallet2 {x} rust {y}")
    strict = w2["balance"]["strict"]
    rs_unspent = sum(o["amount"] for o in rs["outputs"] if not o["spent"])
    rs_unlocked = sum(o["amount"] for o in rs["outputs"] if not o["spent"] and o["unlocked_at_chain_length"])
    if strict["balance"] != rs_unspent:
        problems.append(f"balance(strict): wallet2 {strict['balance']} rust {rs_unspent}")
    if strict["unlocked_balance"] != rs_unlocked:
        problems.append(f"unlocked_balance(strict): wallet2 {strict['unlocked_balance']} rust {rs_unlocked}")
    for acct in strict.get("per_subaddress", []):
        mj, mn = acct["account_index"], acct["address_index"]
        mine_ = sum(o["amount"] for o in rs["outputs"]
                    if not o["spent"] and list(o["subaddress_index"] or [0, 0]) == [mj, mn])
        if mine_ != acct["balance"]:
            problems.append(f"per-subaddress ({mj},{mn}): wallet2 {acct['balance']} rust {mine_}")
    pool_w2 = sorted((t["txid"], t["amount"]) for t in w2["get_transfers"].get("pool", []))
    pool_rs = {}
    for o in rs["pool_outputs"]:
        pool_rs[o["tx_hash"]] = pool_rs.get(o["tx_hash"], 0) + o["amount"]
    print("pool wallet2:", pool_w2)
    print("pool rust   :", sorted(pool_rs.items()), "own spends seen in pool:", len(rs["pool_spent_own_key_images"]))
    print("pending wallet2:", [(t["txid"], t["amount"]) for t in w2["get_transfers"].get("pending", [])])
    print(f"compared {len(w2t)} wallet2 transfers with {len(rst)} rust outputs")
    for p in problems:
        print("DIFF", p)
    print("differences:", len(problems))


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "setup": setup()
    elif cmd == "history": history()
    elif cmd == "w2dump": w2dump(sys.argv[2])
    elif cmd == "diff": diff(sys.argv[2], sys.argv[3])
