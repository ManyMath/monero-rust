#!/usr/bin/env python3
"""Compare monero-rust sends with a real wallet2 on a private regtest chain.

    eval "$(scripts/regtest-node.sh start)"
    (cd rust && cargo test --release -p monero-rust --test wallet2_interop_tool --no-run)
    scripts/wallet2_interop.py --run-dir DIR start
    scripts/wallet2_interop.py --run-dir DIR fund
    scripts/wallet2_interop.py --run-dir DIR shapes      # also: sample, priorities, spend-back
    scripts/wallet2_interop.py --run-dir DIR stop

Needs pycryptodome. `shapes` exits nonzero when a shape differs from wallet2.
"""

import argparse
import collections
import glob
import json
import math
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from wallet2_interop_lib import (  # noqa: E402
    JsonRpc,
    RpcError,
    attribute_outputs,
    decode_address,
    fee_for_weight,
    layout,
    parse_tx,
)

XMR = 10**12
WALLETS = ["sender", "recip", "recip2", "ref", "miner"]
REPO = Path(__file__).resolve().parent.parent


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Run:
    def __init__(self, run_dir, evidence="evidence", tool=None):
        self.dir = Path(run_dir)
        self.evidence_name = evidence
        self.tool_path = tool
        self.state_path = self.dir / "state.json"
        self.state = json.loads(self.state_path.read_text()) if self.state_path.exists() else {}
        self._wallets = {}

    def save(self):
        self.state_path.write_text(json.dumps(self.state, indent=2))

    @property
    def daemon(self):
        return JsonRpc(self.state["daemon"])

    def wallet(self, name):
        if name not in self._wallets:
            self._wallets[name] = JsonRpc(f"http://127.0.0.1:{self.state['wallets'][name]['port']}")
        return self._wallets[name]

    def evidence(self, name):
        path = self.dir / self.evidence_name / name
        path.mkdir(parents=True, exist_ok=True)
        return path

    def height(self):
        return self.daemon.other("/get_info")["height"]

    def mine(self, count, to="miner"):
        address = self.state["wallets"][to]["address"]
        self.daemon.call("generateblocks", {"amount_of_blocks": count, "wallet_address": address})

    def pool(self):
        pool = self.daemon.other("/get_transaction_pool")
        return [tx["id_hash"] for tx in pool.get("transactions", [])]

    def chain_tx(self, txid):
        reply = self.daemon.other("/get_transactions", {"txs_hashes": [txid], "decode_as_json": True})
        return reply["txs"][0]

    def refresh(self, *names):
        for name in names:
            self.wallet(name).call("refresh")

    def tool(self):
        if self.tool_path:
            return self.tool_path
        pattern = str(REPO / "rust/target/*/deps/wallet2_interop_tool-*")
        found = [p for p in glob.glob(pattern) if os.access(p, os.X_OK) and not p.endswith(".d")]
        if not found:
            raise SystemExit("build the tool first: cargo test --test wallet2_interop_tool --no-run")
        return max(found, key=os.path.getmtime)

    def rust(self, where, request):
        request = {
            "rpc": self.state["daemon"],
            "seed": self.state["wallets"]["sender"]["seed"],
            "lookahead_account": 2,
            "lookahead_subaddress": 5,
            **request,
        }
        where = Path(where)
        request_path = where.with_suffix(".request.json")
        response_path = where.with_suffix(".response.json")
        request_path.write_text(json.dumps(request, indent=2))
        if response_path.exists():
            response_path.unlink()
        env = dict(os.environ, INTEROP_REQUEST=str(request_path), INTEROP_RESPONSE=str(response_path))
        done = subprocess.run(
            [self.tool(), "--nocapture", "--test-threads", "1"], env=env, capture_output=True, text=True
        )
        if done.returncode != 0 or not response_path.exists():
            where.with_suffix(".tool.log").write_text(done.stdout + done.stderr)
            raise RuntimeError(f"tool failed, see {where.with_suffix('.tool.log')}\n{done.stdout[-2000:]}")
        return json.loads(response_path.read_text())

    def viewers(self, *names):
        result = []
        for name in names or WALLETS:
            wallet = self.state["wallets"][name]
            spends = {label: decode_address(address)["spend"] for label, address in wallet["addresses"].items()}
            result.append({"name": name, "view": wallet["view_key"], "spends": spends})
        return result



def cmd_start(run, args):
    run.dir.mkdir(parents=True, exist_ok=True)
    daemon = args.daemon or os.environ.get("MONERO_REGTEST_RPC")
    if not daemon:
        raise SystemExit("pass --daemon or set MONERO_REGTEST_RPC")
    run.state["daemon"] = daemon
    wallet_rpc = args.wallet_rpc or os.environ.get("MONERO_WALLET_RPC", "monero-wallet-rpc")
    info = JsonRpc(daemon).other("/get_info")
    if info.get("nettype") != "fakechain":
        raise SystemExit(f"{daemon} is not a regtest daemon")
    run.state["daemon_version"] = info["version"]
    wallets = run.state.setdefault("wallets", {})
    wallet_dir = run.dir / "wallets"
    wallet_dir.mkdir(exist_ok=True)
    # Keep wallet2's ring database with the run instead of in the home directory.
    ringdb = run.dir / "ringdb"
    ringdb.mkdir(exist_ok=True)
    authority = daemon.split("//", 1)[1]
    for name in WALLETS:
        entry = wallets.setdefault(name, {})
        port = free_port()
        log = run.dir / f"wallet-rpc-{name}.log"
        process = subprocess.Popen(
            [
                wallet_rpc,
                "--allow-mismatched-daemon-version",
                "--daemon-address", authority,
                "--trusted-daemon",
                "--daemon-ssl", "disabled",
                "--wallet-dir", str(wallet_dir),
                "--shared-ringdb-dir", str(ringdb),
                "--rpc-bind-ip", "127.0.0.1",
                "--rpc-bind-port", str(port),
                "--disable-rpc-login",
                "--log-level", "1",
                "--log-file", str(log),
                "--max-log-files", "1",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        entry["port"] = port
        entry["pid"] = process.pid
        run.save()
        rpc = run.wallet(name)
        for _ in range(120):
            try:
                rpc.call("get_version")
                break
            except (OSError, RpcError):
                time.sleep(0.5)
        else:
            raise SystemExit(f"wallet-rpc for {name} did not come up, see {log}")
        if (wallet_dir / f"{name}.keys").exists():
            rpc.call("open_wallet", {"filename": name, "password": ""})
        else:
            rpc.call("create_wallet", {"filename": name, "password": "", "language": "English"})
        entry["seed"] = rpc.call("query_key", {"key_type": "mnemonic"})["key"]
        entry["view_key"] = rpc.call("query_key", {"key_type": "view_key"})["key"]
        entry["address"] = rpc.call("get_address", {"account_index": 0})["address"]
        entry["wallet_rpc_version"] = rpc.call("get_version")["version"]
        run.save()

    for name in WALLETS:
        rpc = run.wallet(name)
        entry = wallets[name]
        while len(rpc.call("get_accounts")["subaddress_accounts"]) < 3:
            rpc.call("create_account")
        addresses = {}
        for account in range(3):
            have = rpc.call("get_address", {"account_index": account})["addresses"]
            if len(have) < 4:
                rpc.call("create_address", {"account_index": account, "count": 4 - len(have)})
            for item in rpc.call("get_address", {"account_index": account})["addresses"]:
                addresses[f"{account},{item['address_index']}"] = item["address"]
        entry["addresses"] = addresses
        integrated = rpc.call("make_integrated_address", {"payment_id": "0123456789abcdef"})
        entry["integrated"] = integrated["integrated_address"]
        entry["integrated_payment_id"] = integrated["payment_id"]
        rpc.call("store")
    run.save()
    print(json.dumps({name: wallets[name]["pid"] for name in WALLETS}))


def cmd_stop(run, args):
    for name, entry in run.state.get("wallets", {}).items():
        pid = entry.get("pid")
        if not pid:
            continue
        try:
            run.wallet(name).call("store")
            run.wallet(name).call("stop_wallet")
        except Exception:  # noqa: BLE001
            pass
        for _ in range(40):
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                break
            time.sleep(0.25)
        else:
            os.kill(pid, 15)
        entry["pid"] = None
    run.save()



def cmd_fund(run, args):
    if run.height() < 700:
        run.mine(45, "sender")
        run.mine(45, "ref")
        run.mine(args.filler, "miner")
    run.refresh("ref", "sender")
    funding = []
    for name in ("sender", "ref"):
        addresses = run.state["wallets"][name]["addresses"]
        destinations = []
        for amount in (1 * XMR, 1 * XMR, 2 * XMR, 3 * XMR):
            destinations.append({"address": addresses["0,0"], "amount": amount})
        for amount in (2 * XMR, 3 * XMR, 5 * XMR, 7 * XMR):
            destinations.append({"address": addresses["1,0"], "amount": amount})
        if name == "ref" and run.wallet("ref").call("get_balance", {"account_index": 2})["balance"] == 0:
            destinations.append({"address": addresses["2,0"], "amount": 1 * XMR})
        reply = run.wallet("ref").call(
            "transfer", {"destinations": destinations, "account_index": 0, "get_tx_hex": True, "get_tx_key": True}
        )
        funding.append(reply)
    (run.evidence("fund") / "wallet2_funding_transfers.json").write_text(json.dumps(funding, indent=2))
    run.mine(12, "miner")
    run.refresh(*WALLETS)
    print("height", run.height(), "funding txs", [reply["tx_hash"] for reply in funding])



def address_of(run, target, me="sender"):
    wallet, label = target
    entry = run.state["wallets"][me if wallet == "self" else wallet]
    return entry["integrated"] if label == "integrated" else entry["addresses"][label]


def describe(run, blob_hex, viewer_names):
    tx = parse_tx(bytes.fromhex(blob_hex))
    owned = attribute_outputs(tx, run.viewers(*viewer_names))
    return tx, layout(tx), owned


def fee_facts(tx, estimate):
    tiers = [
        index
        for index, per_byte in enumerate(estimate["fees"])
        if fee_for_weight(tx["weight"], per_byte, estimate["quantization_mask"]) == tx["fee"]
    ]
    return {
        "fee": tx["fee"],
        "size": tx["size"],
        "weight": tx["weight"],
        "fee_varint_len": tx["fee_varint_len"],
        "fee_per_weight": tx["fee"] / tx["weight"],
        "quantized": tx["fee"] % estimate["quantization_mask"] == 0,
        "tiers_matching_actual_weight": tiers,
    }


def wallet2_view_of(run, name, txid):
    rpc = run.wallet(name)
    transfers = rpc.call(
        "get_transfers",
        {"in": True, "out": True, "pending": True, "pool": True, "failed": True, "all_accounts": True},
    )
    entries = [
        dict(entry, list=kind) for kind, items in transfers.items() for entry in items if entry["txid"] == txid
    ]
    outputs = []
    for account in range(3):
        incoming = rpc.call("incoming_transfers", {"transfer_type": "all", "account_index": account, "verbose": True})
        outputs += [item for item in incoming.get("transfers", []) if item["tx_hash"] == txid]
    return {"transfers": entries, "outputs": outputs}


def wallet2_build(run, shape, recipients, account):
    ref = run.wallet("ref")
    if shape.get("sweep"):
        params = {"address": recipients[0][0], "account_index": account, "do_not_relay": True,
                  "get_tx_hex": True, "get_tx_keys": True}
        if shape.get("ref_key_image"):
            reply = ref.try_call("sweep_single", dict(params, key_image=shape["ref_key_image"]))
        else:
            reply = ref.try_call("sweep_all", params)
        if "ok" in reply:
            raw = reply["ok"]

            def first(key):
                return raw.get(key) if key in raw else (raw.get(key + "_list") or [None])[0]

            reply = {"ok": {key: first(key) for key in ("tx_hash", "tx_blob", "tx_key", "fee", "amount")}}
        return reply
    return ref.try_call(
        "transfer",
        {"destinations": [{"address": address, "amount": amount} for address, amount in recipients],
         "account_index": account, "do_not_relay": True, "get_tx_hex": True, "get_tx_key": True},
    )


def settle(recipients, total, fee, leave):
    recipients[-1][1] = total - sum(amount for _, amount in recipients[:-1]) - fee - leave


def run_shape(run, shape):
    name = shape["name"]
    ev = run.evidence(name)
    account = shape.get("account", 0)
    sweep = shape.get("sweep", False)
    targets = shape["to"]
    viewer_names = ["recip", "recip2", "ref", "sender"]
    if shape.get("fund_first"):
        run.wallet("ref").call(
            "transfer",
            {"destinations": [{"address": address_of(run, target), "amount": amount}
                              for target, amount in shape["fund_first"]], "account_index": 0},
        )
    if shape.get("mine_before"):
        run.mine(shape["mine_before"])
        run.refresh("ref", "sender")
    if shape.get("pick"):
        scan = run.rust(ev / "rust_pick", {"op": "scan"})
        shape = dict(shape, **shape["pick"](run, scan))
    estimate = run.daemon.call("get_fee_estimate")
    summary = {"shape": name, "height": run.height(), "fee_estimate": estimate, "checks": [], "mismatches": []}

    def check(label, ok, detail=None):
        summary["checks"].append({"check": label, "ok": bool(ok), "detail": detail})
        if not ok:
            summary["mismatches"].append({"check": label, "detail": detail})

    def finish():
        (ev / "summary.json").write_text(json.dumps(summary, indent=2, default=sorted))
        return summary

    request = {"op": "sweep" if sweep else "send", "source_account": account}
    if shape.get("inputs"):
        request["inputs"] = shape["inputs"]
    recipients = [[address_of(run, target, "sender"), amount] for target, amount in targets]
    ref_recipients = [[address_of(run, target, "ref"), amount] for target, amount in targets]
    ref_account = account

    leave = shape.get("leave")
    dry_fee = ref_dry_fee = None
    if leave is not None:
        ref_account = 2
        dry = wallet2_build(run, shape, ref_recipients, ref_account)
        ref_dry_fee = dry["ok"]["fee"]
        holding = run.wallet("ref").call("get_balance", {"account_index": ref_account})["balance"]
        settle(ref_recipients, holding, ref_dry_fee, leave)
        dry = run.rust(ev / "rust_dry", dict(request, recipients=recipients))
        if not dry["builds"]:
            summary["rust_error"] = dry["build_errors"]
            check("monero-rust builds the request with change", False, dry["build_errors"])
            return finish()
        dry_fee = dry["builds"][0]["result"]["fee"]
        settle(recipients, dry["builds"][0]["input_total"], dry_fee, leave)
        request["rings"] = dry["builds"][0]["result"]["input_rings"]

    reply = wallet2_build(run, shape, ref_recipients, ref_account)
    (ev / "wallet2_reference_build.json").write_text(json.dumps(reply, indent=2))
    reference = None
    if "ok" in reply:
        ref_tx, ref_layout, ref_owned = describe(run, reply["ok"]["tx_blob"], viewer_names)
        reference = {"layout": ref_layout, "owned": ref_owned, "fee": fee_facts(ref_tx, estimate)}
        (ev / "wallet2_reference_decoded.json").write_text(json.dumps(ref_tx, indent=2))
        summary["wallet2_reference"] = reference
    else:
        summary["wallet2_reference_error"] = reply["error"]

    response = run.rust(ev / "rust", dict(request, recipients=recipients, broadcast=True))
    if not response["builds"]:
        summary["rust_error"] = response["build_errors"]
        if reference:
            check("monero-rust builds a request wallet2 builds", False, response["build_errors"])
        else:
            check("both wallets refuse the request", True)
        return finish()
    if not reference:
        check("wallet2 builds a request monero-rust builds", False, summary["wallet2_reference_error"])
    build = response["builds"][0]
    result = build["result"]
    txid = result["tx_id"]
    (ev / "rust_tx.hex").write_text(result["tx_blob"])
    tx, rust_layout, owned = describe(run, result["tx_blob"], viewer_names)
    (ev / "rust_decoded.json").write_text(json.dumps(tx, indent=2))
    summary["txid"] = txid
    summary["rust"] = {
        "layout": rust_layout,
        "owned": owned,
        "fee": fee_facts(tx, estimate),
        "inputs": build["inputs"],
        "input_total": build["input_total"],
        "additional_tx_keys": len(result["tx_key_additional"]),
        "change_outputs": [
            {k: o[k] for k in ("output_index", "amount", "subaddress_index")} for o in result["change_outputs"]
        ],
    }
    check("parser txid equals monero-rust txid", tx["txid"] == txid)
    check("daemon accepted relay", response["broadcast"] == "OK", response["broadcast"])
    check("in pool", txid in run.pool())
    check("fee reported equals fee on chain", result["fee"] == tx["fee"])

    if sweep:
        paid = [(targets[0][0], build["input_total"] - tx["fee"])]
    else:
        paid = [(target, amount) for (target, _), (_, amount) in zip(targets, recipients)]
    expected = [(("sender" if wallet == "self" else wallet, label), amount) for (wallet, label), amount in paid]
    summary["paid"] = [[f"{wallet}:{label}", amount] for (wallet, label), amount in expected]
    wanted = {}
    for (wallet, label), amount in expected:
        if wallet != "sender":
            key = (wallet, "0,0" if label == "integrated" else label)
            wanted[key] = wanted.get(key, 0) + amount

    def credits(views, kind):
        got = {}
        for wallet in ("recip", "recip2"):
            for entry in views[wallet]["transfers"]:
                index = entry["subaddr_index"]
                slot = got.setdefault((wallet, f"{index['major']},{index['minor']}"),
                                      {"amount": 0, "payment_ids": set()})
                slot["amount"] += entry["amount"]
                slot["payment_ids"].add(entry["payment_id"])
                check(f"{wallet} entry is {kind}", entry["type"] == kind, entry["type"])
                check(f"{wallet} entry unlock_time 0", entry["unlock_time"] == 0)
                check(f"{wallet} entry fee equals tx fee", entry["fee"] == tx["fee"], entry["fee"])
        return got

    if shape.get("check_pool"):
        run.refresh("recip", "recip2")
        pending = credits({wallet: wallet2_view_of(run, wallet, txid) for wallet in ("recip", "recip2")}, "pool")
        check("wallet2 recipients see the unconfirmed amounts",
              {key: slot["amount"] for key, slot in pending.items()} == wanted,
              {"wallet2": sorted(map(str, pending.items())), "expected": sorted(map(str, wanted.items()))})

    run.mine(1)
    run.refresh("recip", "recip2", "sender")
    chain = run.chain_tx(txid)
    (ev / "daemon_get_transactions.json").write_text(json.dumps(chain, indent=2))
    check("mined", chain.get("block_height") is not None and not chain.get("in_pool"))
    decoded = json.loads(chain["as_json"])
    check("daemon decodes the same extra", bytes(decoded["extra"]).hex() == tx["extra"])
    check("daemon decodes the same fee", decoded["rct_signatures"]["txnFee"] == tx["fee"])

    views = {wallet: wallet2_view_of(run, wallet, txid) for wallet in ("recip", "recip2", "sender")}
    (ev / "wallet2_views.json").write_text(json.dumps(views, indent=2))

    got = credits(views, "in")
    for key, amount in wanted.items():
        slot = got.get(key)
        check(f"wallet2 {key[0]} credits {key[1]}", slot is not None and slot["amount"] == amount,
              {"expected": amount, "wallet2": slot["amount"] if slot else None})
    for key in got:
        check(f"wallet2 {key[0]} credit at {key[1]} was intended", key in wanted)
    summary["wallet2_credited"] = {
        f"{key[0]}:{key[1]}": {"expected": wanted.get(key), "wallet2": slot["amount"],
                               "payment_ids": sorted(slot["payment_ids"])}
        for key, slot in got.items()
    }
    for (wallet, label), slot in got.items():
        integrated = any(w == wallet and l == "integrated" for (w, l), _ in expected)
        want_id = run.state["wallets"][wallet]["integrated_payment_id"] if integrated else "0000000000000000"
        check(f"wallet2 {wallet} reports the payment ID of {label}", slot["payment_ids"] == {want_id},
              sorted(slot["payment_ids"]))

    derived = {}
    for item in owned:
        derived[(item["viewer"], item["label"])] = derived.get((item["viewer"], item["label"]), 0) + item["amount"]
    for key, amount in wanted.items():
        check(f"independent derivation pays {key[0]} {key[1]}", derived.get(key) == amount)

    proof_key = result["tx_key"] + "".join(result["tx_key_additional"])
    change_label = f"{account},0"
    destinations = {target for target, _ in expected if target != ("sender", change_label)}
    key_on_subaddress = len(destinations) == 1 and all(label not in ("0,0", "integrated") for _, label in destinations)
    proofs = {}
    for wallet, label in sorted({target for target, _ in expected}):
        entry = run.state["wallets"][wallet]
        address = entry["integrated"] if label == "integrated" else entry["addresses"][label]
        proved = run.wallet("miner").try_call("check_tx_key", {"txid": txid, "tx_key": proof_key, "address": address})
        proofs[f"{wallet}:{label}"] = proved
        if (wallet, label) == ("sender", change_label) and (key_on_subaddress or account != 0):
            total = 0
        elif wallet == "sender":
            total = sum(
                o["amount"] for o in result["change_outputs"]
                if "{},{}".format(*(o["subaddress_index"] or (0, 0))) == label
            )
        else:
            total = sum(amount for target, amount in expected if target == (wallet, label))
        check(f"wallet2 check_tx_key {wallet}:{label}", proved.get("ok", {}).get("received") == total, proved)
    (ev / "wallet2_check_tx_key.json").write_text(json.dumps(proofs, indent=2))

    out_entries = [entry for entry in views["sender"]["transfers"] if entry["type"] == "out"]
    sent = sum(amount for (wallet, _), amount in expected if wallet != "sender")
    summary["wallet2_sender_view"] = [
        {k: entry.get(k) for k in ("type", "amount", "fee", "subaddr_index", "payment_id", "unlock_time")}
        for entry in views["sender"]["transfers"]
    ]
    check("wallet2 sender sees one outgoing transfer", len(out_entries) == 1, len(out_entries))
    if out_entries:
        entry = out_entries[0]
        check("wallet2 sender fee equals monero-rust fee", entry["fee"] == result["fee"], entry["fee"])
        check("wallet2 sender amount equals amount sent away", entry["amount"] == sent,
              {"wallet2": entry["amount"], "expected": sent})
        check("wallet2 sender spends from the source account", entry["subaddr_index"]["major"] == account)
    back_wallet2 = sorted(
        (item["amount"], item["subaddr_index"]["major"], item["subaddr_index"]["minor"])
        for item in views["sender"]["outputs"]
    )
    back_rust = sorted((o["amount"], *(o["subaddress_index"] or (0, 0))) for o in result["change_outputs"])
    check("wallet2 sender finds the same outputs back as monero-rust", back_wallet2 == back_rust,
          {"wallet2": back_wallet2, "monero_rust": back_rust})
    rescan = run.rust(ev / "rust_rescan", {"op": "scan"})
    balances = {}
    for acct in range(3):
        wallet2_balance = run.wallet("sender").call("get_balance", {"account_index": acct})["balance"]
        rust_balance = sum(
            o["amount"] for o in rescan["outputs"] if not o["spent"] and (o["subaddress_index"] or [0, 0])[0] == acct
        )
        balances[acct] = {"wallet2": wallet2_balance, "monero_rust": rust_balance}
        check(f"account {acct} balance agrees", wallet2_balance == rust_balance, balances[acct])
    summary["sender_balances"] = balances

    if reference:
        diff = {
            key: {"monero_rust": rust_layout[key], "wallet2": reference["layout"][key]}
            for key in rust_layout
            if rust_layout[key] != reference["layout"][key]
        }
        summary["input_count_diff"] = {key: diff.pop(key) for key in ("inputs", "pseudo_outs") if key in diff}
        summary["layout_diff"] = diff
        check("layout equals wallet2", not diff, diff)
        check(
            "fee rule equals wallet2",
            summary["rust"]["fee"]["tiers_matching_actual_weight"]
            == reference["fee"]["tiers_matching_actual_weight"],
            {"monero_rust": summary["rust"]["fee"], "wallet2": reference["fee"]},
        )
    if leave is None:
        check("fee is a daemon tier applied to the actual weight",
              summary["rust"]["fee"]["tiers_matching_actual_weight"], summary["rust"]["fee"])
    else:
        check("fee is the fee of the build with change", tx["fee"] == dry_fee, {"fee": tx["fee"], "dry": dry_fee})
        if reference:
            check("wallet2 fee is the fee of its build with change", reference["fee"]["fee"] == ref_dry_fee,
                  {"fee": reference["fee"]["fee"], "dry": ref_dry_fee})
    return finish()


def spendable(outputs, height, account=0, coinbase=False):
    return sorted(
        (
            o
            for o in outputs
            if not o["spent"]
            and o["is_coinbase"] == coinbase
            and (o["subaddress_index"] or [0, 0])[0] == account
            and o["block_height"] + 10 <= height
        ),
        key=lambda o: (o["amount"], o["tx_hash"]),
    )


def pick_single(run, scan):
    ours = next(o for o in spendable(scan["outputs"], scan["height"]) if o["amount"] >= XMR // 2)
    theirs = run.wallet("ref").call("incoming_transfers", {"transfer_type": "available", "account_index": 0})
    theirs = sorted(
        (t for t in theirs["transfers"] if t["unlocked"] and t["amount"] <= 3 * XMR), key=lambda t: t["amount"]
    )[0]
    return {"inputs": [f"{ours['tx_hash']}:{ours['output_index']}"], "ref_key_image": theirs["key_image"]}


def shapes():
    r, r2, me = "recip", "recip2", "self"
    sixteen = [
        (r, "0,0"), (r2, "0,0"), (r, "0,1"), (r, "0,2"), (r, "0,3"), (r, "1,0"), (r, "1,1"), (r, "1,2"),
        (r, "1,3"), (r, "2,0"), (r, "2,1"), (r2, "0,1"), (r2, "0,2"), (r2, "1,0"), (r2, "1,1"),
    ]
    return [
        {"name": "01_standard", "to": [((r, "0,0"), 1 * XMR)]},
        {"name": "02_subaddress", "to": [((r, "0,1"), 1 * XMR)]},
        {"name": "03_subaddress_plus_standard", "to": [((r, "0,2"), 1 * XMR), ((r2, "0,0"), 2 * XMR)]},
        {"name": "04_two_standard", "to": [((r, "0,0"), 1 * XMR), ((r2, "0,0"), 2 * XMR)]},
        {"name": "05_five_outputs_mixed", "to": [
            ((r, "0,0"), 1 * XMR), ((r2, "0,0"), 2 * XMR), ((r, "1,1"), 3 * XMR), ((r2, "0,1"), 4 * XMR)]},
        {"name": "06_two_subaddresses_one_wallet", "to": [((r, "0,1"), 1 * XMR), ((r, "1,0"), 2 * XMR)]},
        {"name": "07_integrated", "to": [((r, "integrated"), 1 * XMR)]},
        {"name": "08_multi_input_account1", "account": 1, "to": [((r, "0,0"), 11 * XMR)]},
        {"name": "09_account1_to_subaddress", "account": 1, "to": [((r, "0,3"), 1 * XMR)]},
        {"name": "10_account1_mixed", "account": 1, "mine_before": 10,
         "to": [((r, "2,0"), XMR // 10), ((r2, "0,0"), XMR // 10)]},
        {"name": "11_sixteen_outputs", "to": [(target, (i + 1) * XMR // 10) for i, target in enumerate(sixteen)]},
        {"name": "12_same_subaddress_twice", "to": [((r, "0,1"), 1 * XMR), ((r, "0,1"), 2 * XMR)]},
        {"name": "13_same_standard_twice", "to": [((r, "0,0"), 1 * XMR), ((r, "0,0"), 2 * XMR)]},
        {"name": "14_integrated_plus_subaddress", "to": [((r, "integrated"), 1 * XMR), ((r2, "0,1"), 2 * XMR)]},
        {"name": "15_integrated_plus_standard", "to": [((r, "integrated"), 1 * XMR), ((r2, "0,0"), 2 * XMR)]},
        {"name": "16_self_subaddress", "to": [((me, "0,1"), 1 * XMR)]},
        {"name": "17_self_primary", "to": [((me, "0,0"), 1 * XMR)]},
        {"name": "18_account1_self_change_address", "account": 1, "mine_before": 10, "to": [((me, "1,0"), XMR // 10)]},
        {"name": "19_sweep_single_standard", "sweep": True, "pick": pick_single, "to": [((r, "0,0"), 0)]},
        {"name": "20_sweep_single_subaddress", "sweep": True, "pick": pick_single, "to": [((r, "0,1"), 0)]},
        {"name": "21_sweep_account1", "sweep": True, "account": 1, "mine_before": 10, "to": [((r, "0,2"), 0)]},
        {"name": "22_pool_view_mixed", "check_pool": True, "to": [((r, "0,1"), 1 * XMR), ((r2, "0,0"), 2 * XMR)]},
        {"name": "23_pool_view_integrated", "check_pool": True, "to": [((r, "integrated"), 1 * XMR)]},
        {"name": "24_sweep_single_integrated", "sweep": True, "pick": pick_single, "to": [((r, "integrated"), 0)]},
        {"name": "25_account1_minor2_source", "account": 1, "fund_first": [(("sender", "1,2"), 1 * XMR)],
         "mine_before": 11, "to": [((r, "0,0"), 3 * XMR // 10)]},
        {"name": "26_subaddress_plus_own_change_address", "to": [((me, "0,0"), XMR // 10), ((r, "0,1"), XMR // 5)]},
        {"name": "27_exact_one_destination", "pick": pick_single, "leave": 0, "to": [((r, "0,0"), XMR // 10)]},
        {"name": "28_exact_two_destinations", "pick": pick_single, "leave": 0,
         "to": [((r, "0,0"), XMR // 10), ((r2, "0,0"), XMR // 10)]},
        {"name": "29_exact_subaddress_and_standard", "pick": pick_single, "leave": 0,
         "to": [((r, "0,1"), XMR // 10), ((r2, "0,0"), XMR // 10)]},
        {"name": "30_exact_same_subaddress_twice", "pick": pick_single, "leave": 0,
         "to": [((r, "0,1"), XMR // 10), ((r, "0,1"), XMR // 10)]},
        {"name": "31_change_below_fee_bound_slack", "pick": pick_single, "leave": 100_000_000,
         "to": [((r, "0,0"), XMR // 10)]},
    ]


def matrix_row(summary):
    def side(layout_of, fee):
        if not layout_of:
            return None
        nonce = [field for field in layout_of["extra_layout"] if field.startswith("nonce")]
        additional = [field for field in layout_of["extra_layout"] if field.startswith("additional_pubkeys")]
        return {
            "inputs": layout_of["inputs"],
            "outputs": layout_of["outputs"],
            "extra": "+".join(layout_of["extra_layout"]),
            "extra_len": layout_of["extra_len"],
            "additional_keys": int(additional[0].split("x")[1]) if additional else 0,
            "payment_id": nonce[0].split(":")[1] if nonce else None,
            "fee": fee["fee"],
            "fee_tiers": fee["tiers_matching_actual_weight"],
        }

    rust = summary.get("rust")
    reference = summary.get("wallet2_reference")
    return {
        "shape": summary["shape"],
        "monero_rust": side(rust["layout"], rust["fee"]) if rust else {"refused": summary.get("rust_error")},
        "wallet2": side(reference["layout"], reference["fee"]) if reference
        else {"refused": summary.get("wallet2_reference_error")},
        "checks": len(summary["checks"]),
        "differs": [item["check"] for item in summary["mismatches"]],
        "input_count_differs": bool(summary.get("input_count_diff")),
        "verdict": "differs" if summary["mismatches"] else "equal",
    }


def cmd_shapes(run, args):
    wanted = set(args.only or [])
    path = run.dir / run.evidence_name / "matrix.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    matrix = {row["shape"]: row for row in (json.loads(path.read_text()) if path.exists() else [])}
    differing = []
    for shape in shapes():
        if wanted and shape["name"] not in wanted:
            continue
        summary = run_shape(run, shape)
        row = matrix_row(summary)
        matrix[row["shape"]] = row
        path.write_text(json.dumps([matrix[name] for name in sorted(matrix)], indent=2))
        print(f"{row['shape']}: {row['verdict']} ({row['checks']} checks)"
              + (" input count differs" if row["input_count_differs"] else ""))
        for label in ("monero_rust", "wallet2"):
            print(f"   {label:11}", json.dumps(row[label])[:300])
        for item in summary["mismatches"]:
            print("   DIFFERS", item["check"], json.dumps(item["detail"], default=sorted)[:500])
        if row["verdict"] != "equal":
            differing.append(row["shape"])
    print("shapes differing from wallet2:", differing or "none")
    if differing:
        raise SystemExit(1)



def chi_square_p(statistic, df):
    half = statistic / 2
    if df % 2 == 0:
        term, total = 1.0, 1.0
        for i in range(1, df // 2):
            term *= half / i
            total += term
        return math.exp(-half) * total
    total = math.erfc(math.sqrt(half))
    term = math.sqrt(half) * math.exp(-half) / math.gamma(1.5)
    for i in range(1, (df + 1) // 2):
        total += term
        term *= half / (i + 0.5)
    return total


def uniformity(counts, categories):
    n = sum(counts.values())
    expected = n / categories
    statistic = sum((counts.get(key, 0) - expected) ** 2 / expected for key in counts)
    statistic += (categories - len(counts)) * expected
    return {"n": n, "chi_square": statistic, "df": categories - 1, "p": chi_square_p(statistic, categories - 1)}


def observe(run, blob_hex, change_wallet, estimate):
    tx, shape, owned = describe(run, blob_hex, sorted({change_wallet, "recip", "recip2"}))
    by_index = {}
    for item in owned:
        owner = "change" if item["viewer"] == change_wallet else item["viewer"]
        by_index.setdefault(item["index"], set()).add(owner)
    order = []
    for index in range(len(tx["vout"])):
        owners = by_index.get(index, {"none"})
        order.append("/".join(sorted(owners)))
    nonce = [f for f in tx["extra_fields"] if f["tag"] == "nonce" and f.get("kind") == "encrypted_payment_id"]
    decrypted = None
    if nonce:
        masks = [item["payment_id_mask"] for item in owned if item["viewer"] != change_wallet]
        if masks:
            raw = bytes.fromhex(nonce[0]["payment_id"])
            decrypted = bytes(a ^ b for a, b in zip(raw, bytes.fromhex(masks[0]))).hex()
    facts = fee_facts(tx, estimate)
    return {
        "order": ",".join(order),
        "layout": shape,
        "payment_id_for_recipient": decrypted,
        "fee_tiers": facts["tiers_matching_actual_weight"],
        "weight": tx["weight"],
        "fee": tx["fee"],
        "first_offsets": [vin["key_offsets"][0] for vin in tx["vin"]],
    }


def summarize(observations):
    orders = collections.Counter(item["order"] for item in observations)
    layouts = collections.Counter(json.dumps(item["layout"], sort_keys=True) for item in observations)
    outputs = len(observations[0]["order"].split(","))
    return {
        "builds": len(observations),
        "orders": dict(sorted(orders.items())),
        "order_uniformity": uniformity(orders, math.factorial(outputs)),
        "layouts": [{"count": count, "layout": json.loads(text)} for text, count in layouts.most_common()],
        "payment_id_for_recipient": dict(
            collections.Counter(str(item["payment_id_for_recipient"]) for item in observations)
        ),
        "fee_tiers": dict(collections.Counter(str(item["fee_tiers"]) for item in observations)),
        "weights": dict(sorted(collections.Counter(item["weight"] for item in observations).items())),
    }


def sample_shapes():
    r, r2 = "recip", "recip2"
    return [
        {"name": "sample_two_outputs_standard", "to": [((r, "0,0"), 1 * XMR)]},
        {"name": "sample_two_outputs_subaddress", "to": [((r, "0,1"), 1 * XMR)]},
        {"name": "sample_three_outputs_mixed", "to": [((r, "0,2"), 1 * XMR), ((r2, "0,0"), 2 * XMR)]},
        {"name": "sample_three_outputs_standard", "to": [((r, "0,0"), 1 * XMR), ((r2, "0,0"), 2 * XMR)]},
        {"name": "sample_sweep_dummy", "sweep": True, "pick": pick_single, "to": [((r, "0,0"), 0)]},
        {"name": "sample_four_inputs", "to": [((r, "0,0"), 110 * XMR)]},
    ]


def cmd_sample(run, args):
    wanted = set(args.only or [])
    for shape in sample_shapes():
        name = shape["name"]
        if wanted and name not in wanted:
            continue
        ev = run.evidence(name)
        estimate = run.daemon.call("get_fee_estimate")
        recipients = [[address_of(run, target), amount] for target, amount in shape["to"]]
        sweep = shape.get("sweep", False)
        if shape.get("pick"):
            shape = dict(shape, **shape["pick"](run, run.rust(ev / "rust_pick", {"op": "scan"})))

        request = {"op": "sweep" if sweep else "send", "recipients": recipients, "repeat": args.count}
        if shape.get("inputs"):
            request["inputs"] = shape["inputs"]
        response = run.rust(ev / "rust", request)
        rust = [observe(run, build["result"]["tx_blob"], "sender", estimate) for build in response["builds"]]
        for build, item in zip(response["builds"], rust):
            reported = sorted(o["output_index"] for o in build["result"]["change_outputs"])
            derived = [i for i, owner in enumerate(item["order"].split(",")) if "change" in owner]
            item["change_report_agrees"] = reported == derived

        wallet2 = []
        with open(ev / "wallet2_builds.jsonl", "w") as log:
            for _ in range(args.count):
                if sweep:
                    reply = run.wallet("ref").call(
                        "sweep_single",
                        {"address": recipients[0][0], "key_image": shape["ref_key_image"], "do_not_relay": True,
                         "get_tx_hex": True},
                    )
                    blob = reply["tx_blob"]
                else:
                    reply = run.wallet("ref").call(
                        "transfer",
                        {"destinations": [{"address": a, "amount": v} for a, v in recipients], "account_index": 0,
                         "do_not_relay": True, "get_tx_hex": True},
                    )
                    blob = reply["tx_blob"]
                log.write(json.dumps({"tx_hash": reply["tx_hash"], "fee": reply["fee"], "tx_blob": blob}) + "\n")
                wallet2.append(observe(run, blob, "ref", estimate))

        stats = {
            "shape": name,
            "height": run.height(),
            "fee_estimate": estimate,
            "rust_build_errors": response["build_errors"][:5],
            "monero_rust": summarize(rust),
            "wallet2": summarize(wallet2),
            "monero_rust_change_report_agrees": all(item["change_report_agrees"] for item in rust),
        }
        (ev / "stats.json").write_text(json.dumps(stats, indent=2))
        print(name)
        for side in ("monero_rust", "wallet2"):
            data = stats[side]
            print(f"  {side}: n={data['builds']} orders={data['orders']} p={data['order_uniformity']['p']:.4f} "
                  f"layouts={len(data['layouts'])} pid={data['payment_id_for_recipient']} tiers={data['fee_tiers']}")
        print("  same layout set:", [l["layout"] for l in stats["monero_rust"]["layouts"]]
              == [l["layout"] for l in stats["wallet2"]["layouts"]])



def cmd_priorities(run, args):
    ev = run.evidence("priorities")
    estimate = run.daemon.call("get_fee_estimate")
    recipients = [[address_of(run, ("recip", "0,0")), XMR]]
    report = {"height": run.height(), "fee_estimate": estimate, "priorities": []}
    for name, number in (("automatic", 0), ("low", 1), ("normal", 2), ("elevated", 3), ("priority", 4)):
        row = {"monero_rust_priority": name, "wallet2_priority": number}
        try:
            response = run.rust(ev / f"rust_{name}", {"op": "send", "recipients": recipients, "priority": name})
            if response["builds"]:
                tx = parse_tx(bytes.fromhex(response["builds"][0]["result"]["tx_blob"]))
                row["monero_rust"] = fee_facts(tx, estimate)
            else:
                row["monero_rust_error"] = response["build_errors"]
        except RuntimeError as error:
            row["monero_rust_error"] = str(error)[-600:]
        reply = run.wallet("ref").try_call(
            "transfer",
            {"destinations": [{"address": a, "amount": v} for a, v in recipients], "account_index": 0,
             "priority": number, "do_not_relay": True, "get_tx_hex": True},
        )
        if "ok" in reply:
            row["wallet2"] = fee_facts(parse_tx(bytes.fromhex(reply["ok"]["tx_blob"])), estimate)
        else:
            row["wallet2_error"] = reply["error"]
        report["priorities"].append(row)
        print(name, "monero-rust", row.get("monero_rust", {}).get("tiers_matching_actual_weight"),
              row.get("monero_rust_error"), "| wallet2", row.get("wallet2", {}).get("tiers_matching_actual_weight"),
              row.get("wallet2_error"))
    (ev / "report.json").write_text(json.dumps(report, indent=2))



def cmd_spend_back(run, args):
    ev = run.evidence("spend_back")
    built = set()
    for summary in (run.dir / run.evidence_name).glob("*/summary.json"):
        txid = json.loads(summary.read_text()).get("txid")
        if txid:
            built.add(txid)
    run.mine(10)
    run.refresh(*WALLETS)
    sender_address = run.state["wallets"]["sender"]["address"]
    report = {"monero_rust_txids": len(built), "sweeps": [], "checks": []}

    def check(label, ok, detail=None):
        report["checks"].append({"check": label, "ok": bool(ok), "detail": detail})
        print("ok  " if ok else "FAIL", label, "" if ok else json.dumps(detail)[:400])

    relayed = []
    for wallet in ("recip", "recip2"):
        rpc = run.wallet(wallet)
        for account in range(3):
            before = rpc.call("incoming_transfers", {"transfer_type": "available", "account_index": account})
            ours = [t for t in before.get("transfers", []) if t["tx_hash"] in built]
            if not ours:
                continue
            reply = rpc.try_call(
                "sweep_all",
                {"address": sender_address, "account_index": account, "subaddr_indices_all": True,
                 "get_tx_hex": True},
            )
            report["sweeps"].append({"wallet": wallet, "account": account,
                                     "outputs_from_monero_rust": len(ours),
                                     "amount_from_monero_rust": sum(t["amount"] for t in ours), "reply": reply})
            check(f"wallet2 {wallet} account {account} sweeps {len(ours)} monero-rust outputs", "ok" in reply, reply)
            if "ok" in reply:
                relayed += reply["ok"]["tx_hash_list"]

    mirror = run.wallet("sender")
    available = mirror.call("incoming_transfers", {"transfer_type": "available", "account_index": 0})
    change = [t for t in available["transfers"] if t["tx_hash"] in built and t["unlocked"]]
    change_spend = None
    if change:
        target = change[0]
        reply = mirror.try_call("sweep_single", {"address": address_of(run, ("recip", "0,0")),
                                                 "key_image": target["key_image"], "get_tx_hex": True})
        report["change_spend"] = {"output": target, "reply": reply}
        check("wallet2 twin spends a change output monero-rust created", "ok" in reply, reply)
        if "ok" in reply:
            change_spend = target
            relayed.append(reply["ok"]["tx_hash"])

    pool = run.pool()
    check("daemon holds every wallet2 spend", all(txid in pool for txid in relayed), relayed)
    run.mine(1)
    check("every wallet2 spend was mined", not run.pool())
    run.refresh(*WALLETS)

    for wallet in ("recip", "recip2"):
        rpc = run.wallet(wallet)
        left = []
        for account in range(3):
            incoming = rpc.call("incoming_transfers", {"transfer_type": "all", "account_index": account})
            left += [t for t in incoming.get("transfers", []) if t["tx_hash"] in built and not t["spent"]]
        check(f"{wallet} has spent everything monero-rust paid it", not left, left[:3])

    scan = run.rust(ev / "rust_scan", {"op": "scan"})
    rust_outputs = {
        o["key_image"]: (o["tx_hash"], o["amount"], tuple(o["subaddress_index"] or (0, 0)), o["global_index"], o["spent"])
        for o in scan["outputs"]
    }
    wallet2_outputs = {}
    for account in range(3):
        incoming = mirror.call("incoming_transfers", {"transfer_type": "all", "account_index": account})
        for t in incoming.get("transfers", []):
            wallet2_outputs[t["key_image"]] = (
                t["tx_hash"], t["amount"], (t["subaddr_index"]["major"], t["subaddr_index"]["minor"]),
                t["global_index"], t["spent"],
            )
    report["sender_outputs"] = {"monero_rust": len(rust_outputs), "wallet2": len(wallet2_outputs)}
    only_rust = sorted(set(rust_outputs) - set(wallet2_outputs))
    only_wallet2 = sorted(set(wallet2_outputs) - set(rust_outputs))
    differing = [k for k in rust_outputs if k in wallet2_outputs and rust_outputs[k] != wallet2_outputs[k]]
    check("sender output sets agree by key image", not only_rust and not only_wallet2,
          {"only_monero_rust": [rust_outputs[k] for k in only_rust][:5],
           "only_wallet2": [wallet2_outputs[k] for k in only_wallet2][:5]})
    check("sender outputs agree on txid, amount, subaddress, global index and spent state", not differing,
          [(rust_outputs[k], wallet2_outputs[k]) for k in differing[:5]])
    if change_spend:
        check("monero-rust sees the wallet2 change spend", rust_outputs[change_spend["key_image"]][4])
    for account in range(3):
        wallet2_balance = mirror.call("get_balance", {"account_index": account})["balance"]
        rust_balance = sum(v[1] for v in rust_outputs.values() if not v[4] and v[2][0] == account)
        check(f"account {account} balance agrees", wallet2_balance == rust_balance,
              {"wallet2": wallet2_balance, "monero_rust": rust_balance})
    (ev / "report.json").write_text(json.dumps(report, indent=2))


COMMANDS = {"start": cmd_start, "stop": cmd_stop, "fund": cmd_fund, "shapes": cmd_shapes, "sample": cmd_sample,
            "priorities": cmd_priorities, "spend-back": cmd_spend_back}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--run-dir", required=True, help="where wallets, logs and evidence are kept")
    parser.add_argument("--evidence", default="evidence", help="subdirectory of the run dir for this pass")
    parser.add_argument("--tool", default=os.environ.get("INTEROP_TOOL"),
                        help="wallet2_interop_tool test binary (default: the newest one under rust/target)")
    sub = parser.add_subparsers(dest="command", required=True)
    start = sub.add_parser("start")
    start.add_argument("--daemon", default=None, help="regtest daemon RPC URL (default: MONERO_REGTEST_RPC)")
    start.add_argument("--wallet-rpc", default=None, help="monero-wallet-rpc binary (default: MONERO_WALLET_RPC)")
    sub.add_parser("stop")
    fund = sub.add_parser("fund")
    fund.add_argument("--filler", type=int, default=720)
    shapes_parser = sub.add_parser("shapes")
    shapes_parser.add_argument("--only", nargs="*")
    sub.add_parser("priorities")
    sub.add_parser("spend-back")
    sample_parser = sub.add_parser("sample")
    sample_parser.add_argument("--only", nargs="*")
    sample_parser.add_argument("--count", type=int, default=200)
    args = parser.parse_args()
    run = Run(args.run_dir, args.evidence, args.tool)
    COMMANDS[args.command](run, args)


if __name__ == "__main__":
    main()
