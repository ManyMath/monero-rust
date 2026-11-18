#!/usr/bin/env python3
"""Build a regtest chain through block versions 1 to 16 and dump wallet2's view of
it as fixtures for rust/monero-rust/tests/prerct_history_test.rs.

    prerct_chain.py all TAG    up, build, w2dump TAG; ends with the exports the test needs
    prerct_chain.py up | build | stop | reset | rust-sends | restart-daemon [--disable-rpc-ban]
    prerct_chain.py w2dump TAG | rsdump TAG | diff TAG | variants TAG

Needs PRERCT_MONEROD (scripts/prerct/build_monerod.sh) and the w2_extra_send
harness (scripts/prerct/build_w2_extra_send.sh).
"""

import json
import os
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True

import prerct_chain_lib as L  # noqa: E402
from prerct_chain_lib import XMR, fork_at, fork_height  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
EXTRA_SEND = os.environ.get("PRERCT_EXTRA_SEND", os.path.join(L.RUN, "harness", "w2_extra_send"))
TOOL = os.environ.get("PRERCT_TOOL", "")
NAMES = ["miner", "alice", "bob", "carol", "payer"]
DUMPED = ["miner", "alice", "bob", "carol"]
# wallet2 picks the minimum ring size itself; these mirror get_min_ring_size.
RING = {1: 1, 2: 3, 3: 3, 4: 3, 5: 3, 6: 5, 7: 7, 8: 11, 9: 11, 10: 11, 11: 11, 12: 11, 13: 11, 14: 11, 15: 16, 16: 16}
STAGES = ("hf1", "hf2", "hf3", "hf4", "hf5", "later", "tip")


class Chain:
    def __init__(self):
        self.state = L.load_state()
        self.daemon = L.Daemon(self.state)
        self.w = {name: L.Wallet(self.state, name) for name in NAMES if name in self.state["wallets"]}
        self.tx_versions = {}

    def height(self):
        return self.daemon.height()

    def era(self):
        return fork_at(self.height())

    def ring(self):
        return RING[self.era()]

    def log(self, kind, **fields):
        event = {"kind": kind, "height": self.height(), "era": self.era(), **fields}
        self.state["events"].append(event)
        L.save_state(self.state)
        print(json.dumps(event), flush=True)
        return event

    def addr(self, name, account=0, index=0):
        return self.w[name].address(account, index)

    def a(self, account=0, index=0):
        return self.addr("alice", account, index)

    def integrated(self, pid):
        return self.w["alice"].call("make_integrated_address", {"payment_id": pid})["integrated_address"]

    def mine(self, count, to="miner"):
        return self.daemon.mine(count, self.state["wallets"][to]["address"])

    def mine_to(self, target, to="miner"):
        current = self.height()
        if current < target:
            self.mine(target - current, to)

    def done(self, note):
        return note in self.state.setdefault("notes_done", [])

    def mark(self, note):
        self.state.setdefault("notes_done", []).append(note)
        L.save_state(self.state)

    def tx_version(self, txid):
        if txid not in self.tx_versions:
            self.tx_versions[txid] = self.daemon.tx(txid)["json"]["version"]
        return self.tx_versions[txid]

    def transfers(self, name, kind="all"):
        wallet = self.w[name]
        out = []
        for account in wallet.call("get_accounts", {})["subaddress_accounts"]:
            out += wallet.call("incoming_transfers", {"transfer_type": kind, "account_index": account["account_index"]}).get("transfers", [])
        return out

    def thaw_all(self, name):
        for out in self.transfers(name):
            if out["frozen"]:
                self.w[name].call("thaw", {"key_image": out["key_image"]})

    def mixable_amounts(self, amounts, ring):
        need = 3 * ring + 4
        good = set()
        reply = self.daemon.call("get_output_histogram", {"amounts": sorted(amounts), "unlocked": False, "min_count": 0}) if amounts else {}
        for entry in reply.get("histogram", []):
            total = entry["total_instances"]
            if total < need:
                continue
            outs = self.daemon.other("/get_outs", {"outputs": [{"amount": entry["amount"], "index": i} for i in range(total)]})["outs"]
            locked = sum(1 for out in outs if not out["unlocked"])
            if locked <= total * 0.03:
                good.add(entry["amount"])
        return good

    def mixable_prerct(self, name):
        wallet = self.w[name]
        wallet.refresh()
        outs = [o for o in wallet.call("incoming_transfers", {"transfer_type": "available"}).get("transfers", [])
                if self.tx_version(o["tx_hash"]) == 1]
        good = self.mixable_amounts({o["amount"] for o in outs}, self.ring())
        return [o for o in outs if o["unlocked"] and o["amount"] in good]

    def prepare_inputs(self, name, allow_rct):
        wallet = self.w[name]
        wallet.refresh()
        outs = wallet.call("incoming_transfers", {"transfer_type": "available"}).get("transfers", [])
        ring = self.ring()
        prerct = [o for o in outs if self.tx_version(o["tx_hash"]) == 1]
        good = self.mixable_amounts({o["amount"] for o in prerct}, ring) if ring > 1 else set()
        for out in outs:
            if self.tx_version(out["tx_hash"]) == 1:
                usable = ring == 1 or out["amount"] in good
            else:
                usable = allow_rct
            if usable and out["frozen"]:
                wallet.call("thaw", {"key_image": out["key_image"]})
            elif not usable and not out["frozen"]:
                wallet.call("freeze", {"key_image": out["key_image"]})

    def log_tx(self, kind, txid, **fields):
        tx = self.daemon.tx(txid)["json"]
        self.log(kind, txid=txid, version=tx["version"],
                 rct_type=tx.get("rct_signatures", {}).get("type"),
                 inputs=len(tx["vin"]), ring=len(tx["vin"][0]["key"]["key_offsets"]),
                 input_amounts=sorted({vin["key"]["amount"] for vin in tx["vin"]}),
                 outputs=len(tx["vout"]), view_tags="tagged_key" in tx["vout"][0]["target"], **fields)

    def send(self, sender, dests, note, allow_rct=False):
        if self.done(note):
            return
        self.prepare_inputs(sender, allow_rct)
        attempts = 0
        while True:
            attempts += 1
            try:
                reply = self.w[sender].call("transfer_split", {
                    "destinations": [{"address": address, "amount": amount} for address, amount in dests],
                    "account_index": 0, "priority": 1, "ring_size": 0,
                })
                break
            except L.RpcError as error:
                if "not enough outputs to use" not in str(error) or attempts >= 60:
                    raise
        for txid in reply["tx_hash_list"]:
            self.log_tx("send", txid, sender=sender, note=note, amounts=[amount for _, amount in dests], attempts=attempts)
        self.mark(note)

    def era_mode(self):
        era = self.era()
        if era < 4 or era >= 13:
            return "auto", 0
        if era < 8:
            return "borromean", 300000000000
        if era < 10:
            return "bp1", 50000000000
        return "bp2", 50000000000

    def harness_send(self, sender, dest, amount, note, payment_id=None, allow=None, allow_rct=False, min_inputs=1):
        if self.done(note):
            return
        mode, fee = self.era_mode()
        if allow == "any":
            allow_arg = "-"
        else:
            if allow is None:
                allow = [] if allow_rct else sorted({o["amount"] for o in self.mixable_prerct(sender)})
            allow_arg = ",".join(str(amount) for amount in allow) or "0"
        key = self.state["wallets"][sender]["spend_key"]
        attempts = 0
        while True:
            attempts += 1
            out = subprocess.run(
                [EXTRA_SEND, self.daemon.url.replace("http://", ""), os.path.join(L.RUN, "ringdb"), key,
                 dest, str(amount), payment_id or "-", "0", allow_arg,
                 "1" if allow_rct else "0", mode, str(fee), str(min_inputs)],
                capture_output=True, text=True, timeout=900,
            )
            lines = [line for line in out.stdout.splitlines() if line.startswith("{")]
            reply = json.loads(lines[-1]) if lines else {"error": (out.stdout + out.stderr)[-400:]}
            if "not enough outputs to use" not in reply.get("error", "") or attempts >= 80:
                break
        if "error" in reply:
            raise RuntimeError(f"harness send '{note}': {reply['error']}")
        for txid in reply["txids"]:
            self.log_tx("send", txid, sender=sender, via="harness", mode=mode, note=note,
                        payment_id=payment_id, amounts=[amount], attempts=attempts)
        if sender in self.w:
            self.w[sender].refresh()
        self.mark(note)

    def long_pid_send(self, dest, amount, payment_id, note):
        allow = "any" if self.ring() == 1 else [7 * XMR, 10 * XMR]
        self.harness_send("payer", dest, amount, note, payment_id=payment_id, allow=allow)

    def sweep_unmixable(self, name, note):
        if self.done(note):
            return
        wallet = self.w[name]
        self.thaw_all(name)
        wallet.refresh()
        held = []
        for out in self.transfers(name, "available"):
            if out["subaddr_index"]["major"] != 0:
                wallet.call("freeze", {"key_image": out["key_image"]})
                held.append(out["key_image"])
        try:
            reply = wallet.call("sweep_dust", {})
        finally:
            for key_image in held:
                wallet.call("thaw", {"key_image": key_image})
        if not reply.get("tx_hash_list"):
            raise RuntimeError(f"'{note}': wallet2 found nothing unmixable to sweep")
        for txid in reply["tx_hash_list"]:
            tx = self.daemon.tx(txid)["json"]
            self.log_tx("sweep_unmixable", txid, sender=name, note=note,
                        output_amounts=[out["amount"] for out in tx["vout"]])
        self.mark(note)

    def restart_daemon(self, v1_coinbase, disable_rpc_ban=False):
        L.stop_daemon(self.state)
        L.start_daemon(self.state, v1_coinbase=v1_coinbase, disable_rpc_ban=disable_rpc_ban)
        self.daemon = L.Daemon(self.state)
        self.log("daemon_restart", v1_coinbase=v1_coinbase, disable_rpc_ban=disable_rpc_ban)

    def build(self):
        done = self.state.setdefault("stages_done", [])
        for stage in STAGES:
            if stage in done:
                continue
            getattr(self, "stage_" + stage)()
            done.append(stage)
            L.save_state(self.state)
            self.log("stage_done", stage=stage)

    def stage_hf1(self):
        a = self.a
        self.mine_to(66, "miner")
        self.mine_to(69, "alice")
        self.mine_to(71, "carol")
        self.mine_to(73, "bob")
        self.mine_to(81, "payer")
        self.mine_to(85, "miner")
        self.send("miner", [(a(), 12345678901234)], "hf1 v1 tx to primary, digit-split down to piconero")
        self.mine(1)
        self.send("miner", [(a(0, 1), 3300000000000)], "hf1 v1 tx to subaddress 0/1")
        self.send("miner", [(a(1, 0), 2020000000000)], "hf1 v1 tx to account 1")
        self.mine(1)
        self.send("miner", [(self.integrated("0123456789abcdef"), 1100000000000)], "hf1 v1 tx to integrated address")
        self.mine(1)
        self.send("miner", [(self.addr("bob"), 20 * XMR), (self.addr("carol"), 9876543210000)], "hf1 v1 tx to bob and carol")
        self.send("miner", [(self.addr("carol", 0, 2), 4 * XMR)], "hf1 v1 tx to carol subaddress 0/2")
        self.mine(1)
        self.mine_to(100)
        self.send("alice", [(self.addr("bob"), 5500000000000)], "hf1 alice spends v1 outputs (ring size 1)")
        self.mine(1)
        self.mine_to(136)
        self.long_pid_send(a(), 700000000000, "a1" * 32, "hf1 v1 tx with long payment id")
        self.long_pid_send(self.addr("carol"), 300000000000, "c1" * 32, "hf1 v1 tx with long payment id to carol")
        self.mine(1)
        self.mine_to(fork_height(2))

    def stage_hf2(self):
        a = self.a
        self.mine_to(153, "alice")
        self.mine_to(154, "carol")
        self.mine_to(212)
        self.send("miner", [(a(), 4400000000000)], "hf2 v1 tx ring 3 to primary")
        self.send("miner", [(a(0, 3), 600000000000)], "hf2 v1 tx ring 3 to subaddress 0/3")
        self.mine(1)
        self.long_pid_send(a(), 900000000000, "a2" * 32, "hf2 v1 tx ring 3 with long payment id")
        self.mine(1)
        self.mine_to(224)
        self.send("alice", [(self.addr("bob"), 1500000000000)], "hf2 alice spends v1 outputs (ring size 3)")
        self.mine(1)
        self.sweep_unmixable("bob", "hf2 bob sweeps hf1 coinbase dust")
        self.mine(1)
        self.mine_to(fork_height(3))

    def stage_hf3(self):
        a = self.a
        self.mine_to(252, "alice")
        self.mine_to(256)
        self.send("miner", [(a(), 2500000000000)], "hf3 v1 tx to primary")
        self.send("miner", [(self.addr("carol"), 1250000000000)], "hf3 v1 tx to carol")
        self.mine(1)
        self.mine_to(270)
        self.send("alice", [(self.addr("bob"), 750000000000)], "hf3 alice spends v1 outputs")
        self.mine(1)
        self.mine_to(fork_height(4))

    def stage_hf4(self):
        a = self.a
        self.mine_to(351, "alice")
        self.mine_to(fork_height(4) + 32)
        self.harness_send("miner", a(), 6 * XMR, "hf4 RingCT tx (one input) spending a pre-RingCT output, to primary")
        self.mine(1)
        self.harness_send("miner", a(1, 1), 1750000000000, "hf4 RingCT tx (two inputs) to account 1 subaddress 1", min_inputs=2)
        self.mine(1)
        self.mine_to(395)
        self.harness_send("alice", self.addr("bob"), 2250000000000, "hf4 alice spends pre-RingCT outputs in a RingCT tx", min_inputs=2)
        self.mine(1)
        self.long_pid_send(a(), 800000000000, "a4" * 32, "hf4 RingCT tx with long payment id")
        self.mine(1)
        self.mine_to(400)
        self.sweep_unmixable("alice", "hf4 alice sweeps hf1 coinbase dust (v1 tx in a v4 block)")
        self.harness_send("miner", a(), 1230000000000, "hf4 RingCT tx mined beside the v1 sweep")
        self.mine(1)
        self.mine_to(455)
        self.harness_send("miner", a(), 3 * XMR, "hf4 RingCT tx spending RingCT inputs", allow_rct=True)
        self.mine(1)
        self.mine_to(fork_height(5))

    def stage_hf5(self):
        a = self.a
        self.mine_to(473)
        if not self.done("hf5 v1 coinbase blocks"):
            self.restart_daemon(True)
            first = self.height()
            self.mine(1, "alice")
            self.harness_send("miner", a(), 1110000000000, "hf5 RingCT tx mined in a block with a v1 coinbase")
            self.mine(1, "alice")
            self.log("v1_coinbase_blocks", blocks=[first, first + 1])
            self.restart_daemon(False)
            self.mark("hf5 v1 coinbase blocks")
        self.mine_to(480)
        self.harness_send("miner", a(), 1500000000000, "hf5 RingCT tx")
        self.mine(1)
        self.mine_to(fork_height(6))

    def stage_later(self):
        a = self.a
        plan = {
            6: [("primary", 1600000000000), ("integrated", 660000000000)],
            7: [("sub04", 1700000000000)],
            8: [("primary", 1800000000000)],
            9: [("primary", 1900000000000)],
            10: [("primary", 2 * XMR), ("longpid", 500000000000)],
            11: [("sub11", 2100000000000)],
            12: [("primary", 2200000000000), ("longpid", 400000000000), ("coinbase", 0)],
            13: [("primary", 2300000000000)],
            14: [("primary", 2400000000000), ("alice_spend", 3210000000000)],
            15: [("primary", 2500000000000), ("late_sub", 2550000000000)],
            16: [("primary", 2600000000000), ("coinbase", 0)],
        }
        for version in range(6, 17):
            start = fork_height(version)
            self.mine_to(start + 5)

            def pay(dest, amount, note):
                if version >= 13:
                    self.send("miner", [(dest, amount)], note, allow_rct=True)
                else:
                    self.harness_send("miner", dest, amount, note)

            for kind, amount in plan[version]:
                if kind == "primary":
                    pay(a(), amount, f"hf{version} RingCT tx to primary")
                elif kind == "integrated":
                    pay(self.integrated("fedcba9876543210"), amount, f"hf{version} RingCT tx to integrated address")
                elif kind == "sub04":
                    pay(a(0, 4), amount, f"hf{version} RingCT tx to subaddress 0/4")
                elif kind == "sub11":
                    pay(a(1, 1), amount, f"hf{version} RingCT tx to account 1 subaddress 1")
                elif kind == "longpid":
                    self.long_pid_send(a(), amount, f"{version:02x}" * 32, f"hf{version} RingCT tx with long payment id")
                elif kind == "coinbase":
                    if not self.done(f"hf{version} coinbase to alice"):
                        self.mine(1, "alice")
                        self.mark(f"hf{version} coinbase to alice")
                elif kind == "alice_spend":
                    self.send("alice", [(self.addr("bob"), amount)], f"hf{version} alice spends (wallet2 picks)", allow_rct=True)
                elif kind == "late_sub":
                    self.mine_to(start + 16)
                    self.send("miner", [(a(0, 5), amount)], f"hf{version} RingCT tx to subaddress 0/5", allow_rct=True)
                self.mine(1)
        self.mine_to(fork_height(16) + 70)

    def stage_tip(self):
        bob = self.addr("bob")
        note = "hf16 wallet2 spends one pre-RingCT output (sweep_single)"
        if not self.done(note):
            alice = self.w["alice"]
            self.thaw_all("alice")
            target = max(self.mixable_prerct("alice"), key=lambda o: o["amount"])
            reply = alice.call("sweep_single", {"address": bob, "key_image": target["key_image"], "priority": 1, "ring_size": 0})
            for txid in reply.get("tx_hash_list") or [reply["tx_hash"]]:
                self.log_tx("send", txid, sender="alice", note=note, key_image=target["key_image"])
            self.mark(note)
            self.mine(1)
        note = "hf16 alice sweeps unmixable outputs (v1 tx in a v16 block)"
        if not self.done(note):
            self.sweep_unmixable("alice", note)
            self.mine(1)
        note = "hf16 carol (pre-RingCT funds only) pays alice"
        if not self.done(note):
            self.send("carol", [(self.addr("alice"), 2 * XMR)], note)
            self.mine(12)
        note = "hf16 locked outputs for the snapshot"
        if not self.done(note):
            self.mine(2, "alice")
            self.send("miner", [(self.addr("alice", 0, 1), 500000000000)], "hf16 RingCT tx under 10 confirmations", allow_rct=True)
            self.mine(3)
            self.mark(note)

    def spend_heights(self):
        heights = {}
        for height in range(1, self.height()):
            hashes = self.daemon.block(height).get("tx_hashes", [])
            if not hashes:
                continue
            reply = self.daemon.other("/get_transactions", {"txs_hashes": hashes, "decode_as_json": True})
            for entry in reply["txs"]:
                for vin in json.loads(entry["as_json"])["vin"]:
                    heights[vin["key"]["k_image"]] = height
        return heights

    def w2dump(self, tag):
        out_dir = os.path.join(L.RUN, "dumps", tag)
        os.makedirs(out_dir, exist_ok=True)
        height = self.height()
        spent_at = self.spend_heights()
        chain = {
            "forks": L.FORKS, "height": height,
            "anchor": {"height": height - 1, "hash": self.daemon.block_hash(height - 1)},
            "events": self.state["events"],
            "wallets": {name: {key: record[key] for key in ("seed", "address")}
                        for name, record in self.state["wallets"].items()},
        }
        with open(os.path.join(out_dir, "chain.json"), "w") as handle:
            json.dump(chain, handle, indent=1)
        for name in DUMPED:
            wallet = self.w[name]
            self.thaw_all(name)
            wallet.refresh()
            transfers = self.transfers(name)
            for transfer in transfers:
                transfer["tx_version"] = self.tx_version(transfer["tx_hash"])
                transfer["spent_height"] = spent_at.get(transfer["key_image"])
            accounts = wallet.call("get_accounts", {})
            balances = {}
            for account in accounts["subaddress_accounts"]:
                index = account["account_index"]
                balances[index] = wallet.call("get_balance", {"account_index": index, "all_accounts": False})
            history = wallet.call("get_transfers", {"in": True, "out": True, "pending": True, "pool": True, "all_accounts": True})
            dump = {
                "name": name, "daemon_height": height, "wallet_height": wallet.height(),
                "transfers": transfers, "accounts": accounts, "balances": balances, "history": history,
            }
            with open(os.path.join(out_dir, f"w2-{name}.json"), "w") as handle:
                json.dump(dump, handle, indent=1)
            print(name, "wallet2 outputs", len(transfers), "balance", accounts["total_balance"],
                  "unlocked", accounts["total_unlocked_balance"], flush=True)
        return out_dir

    def rust(self, name, request, out_path):
        request = {"rpc": self.daemon.url, "seed": self.state["wallets"][name]["seed"], **request}
        req_path = out_path + ".req"
        with open(req_path, "w") as handle:
            json.dump(request, handle)
        env = dict(os.environ, PRERCT_REQUEST=req_path, PRERCT_RESPONSE=out_path)
        if os.path.exists(out_path):
            os.remove(out_path)
        try:
            if TOOL:
                run = subprocess.run([TOOL, "--nocapture"], env=env, capture_output=True, text=True, timeout=3600)
            else:
                run = subprocess.run(
                    ["cargo", "test", "-p", "monero-rust", "--test", "prerct_scan_tool", "--", "--nocapture"],
                    cwd=os.path.join(HERE, "..", "rust"), env=env, capture_output=True, text=True, timeout=3600,
                )
        finally:
            os.remove(req_path)   # it holds the seed
        if run.returncode != 0 or not os.path.exists(out_path):
            raise RuntimeError(f"prerct_scan_tool failed for {name}:\n{run.stdout[-3000:]}\n{run.stderr[-3000:]}")
        with open(out_path) as handle:
            return json.load(handle)

    def rsdump(self, tag):
        out_dir = os.path.join(L.RUN, "dumps", tag)
        os.makedirs(out_dir, exist_ok=True)
        for name in DUMPED:
            reply = self.rust(name, {"op": "scan"}, os.path.join(out_dir, f"rs-{name}.json"))
            print(name, "monero-rust outputs", len(reply["outputs"]), "confirmed", reply["balance_confirmed"],
                  "unconfirmed", reply["balance_unconfirmed"], "scanned_to", reply["scanned_to"],
                  "error", reply["scan_error"], flush=True)

    def rust_step(self, name, request, label):
        out_dir = os.path.join(L.RUN, "dumps", "rust-sends")
        os.makedirs(out_dir, exist_ok=True)
        reply = self.rust(name, request, os.path.join(out_dir, f"{label}.json"))
        build = reply.get("build") or {}
        result = build.get("result") or {}
        self.log("rust_step", label=label, wallet=name, op=request["op"], scan_error=reply["scan_error"],
                 outputs=len(reply["outputs"]), balance=reply["balance_confirmed"] + reply["balance_unconfirmed"],
                 prepare_error=reply["prepare_error"], build_error=reply["build_error"],
                 broadcast=reply["broadcast"], txid=result.get("tx_id"), fee=result.get("fee"),
                 inputs=build.get("inputs"))
        return reply

    def rust_sends(self):
        bob = self.addr("bob")
        sent = []
        sent.append(self.rust_step("alice", {"op": "send", "recipients": [[bob, 1 * XMR]], "broadcast": True}, "alice-send"))
        self.mine(1)
        scan = self.rust_step("alice", {"op": "scan"}, "alice-scan")
        picked = {}
        for out in scan["outputs"]:
            key = f"{out['tx_hash']}:{out['output_index']}"
            if out["spent"] or key not in scan["spendable"] or (out["subaddress_index"] or [0, 0])[0] != 0:
                continue
            picked.setdefault(f"hf{fork_at(out['block_height'])}{'-coinbase' if out['is_coinbase'] else ''}", key)
        self.log("era_inputs", picked=picked)
        sent.append(self.rust_step("alice", {"op": "sweep", "recipients": [[bob, 0]], "inputs": sorted(picked.values()), "broadcast": True}, "alice-era-sweep"))
        self.mine(1)
        sent.append(self.rust_step("alice", {"op": "sweep", "source_account": 1, "recipients": [[bob, 0]], "broadcast": True}, "alice-account1-sweep"))
        sent.append(self.rust_step("carol", {"op": "sweep", "recipients": [[bob, 0]], "broadcast": True}, "carol-sweep"))
        self.mine(1)
        for reply in sent:
            txid = ((reply.get("build") or {}).get("result") or {}).get("tx_id")
            if reply["broadcast"] != "OK" or self.daemon.tx(txid).get("in_pool", True):
                raise RuntimeError(f"a monero-rust transaction was not mined: {reply['prepare_error'] or reply['build_error'] or reply['broadcast']}")
        print("all monero-rust transactions were relayed and mined")

    def variants(self, tag):
        out_dir = os.path.join(L.RUN, "dumps", tag)
        os.makedirs(out_dir, exist_ok=True)
        self.restart_daemon(False, disable_rpc_ban=True)
        try:
            self.run_variants(out_dir)
        finally:
            self.restart_daemon(False)

    def run_variants(self, out_dir):
        fork4 = fork_height(4)
        cases = {
            "from-200": {"start_height": 200},
            "from-fork4": {"start_height": fork4},
            "batch-stop-200-resume": {"start_height": 0, "stop_height": 200, "resume": True},
            "history-from-0": {"start_height": 0, "mode": "history"},
            "history-from-0-stop-200-resume": {"start_height": 0, "mode": "history", "stop_height": 200, "resume": True},
            "history-from-0-stop-600-resume": {"start_height": 0, "mode": "history", "stop_height": 600, "resume": True},
            "history-from-fork4-stop-600-resume": {"start_height": fork4, "mode": "history", "stop_height": 600, "resume": True},
            "history-seeded-from-1-stop-200-resume": {"start_height": 1, "mode": "history", "stop_height": 200, "resume": True, "seed_genesis": True},
            "history-seeded-from-1-stop-600-resume": {"start_height": 1, "mode": "history", "stop_height": 600, "resume": True, "seed_genesis": True},
        }
        for label, request in cases.items():
            reply = self.rust("alice", {"op": "scan", **request}, os.path.join(out_dir, f"rs-alice-{label}.json"))
            print(label, json.dumps({
                "scan_error": reply["scan_error"], "passes": reply["passes"], "rounds": reply["rounds"],
                "scanned_to": reply["scanned_to"], "outputs": len(reply["outputs"]),
                "balance": reply["balance_confirmed"] + reply["balance_unconfirmed"],
            }), flush=True)


def diff(tag):
    out_dir = os.path.join(L.RUN, "dumps", tag)
    summary = {}
    for name in DUMPED:
        w2_path = os.path.join(out_dir, f"w2-{name}.json")
        rs_path = os.path.join(out_dir, f"rs-{name}.json")
        if not (os.path.exists(w2_path) and os.path.exists(rs_path)):
            continue
        with open(w2_path) as handle:
            w2 = json.load(handle)
        with open(rs_path) as handle:
            rs = json.load(handle)
        w2_outs = {o["pubkey"]: o for o in w2["transfers"]}
        rs_outs = {o["key"]: o for o in rs["outputs"]}
        w2_pids = {}
        for entry in w2["history"].get("in", []):
            pid = entry.get("payment_id", "")
            w2_pids[entry["txid"]] = None if set(pid) <= {"0"} else pid
        matched, mismatches, missing = 0, [], {}
        for key, o in w2_outs.items():
            r = rs_outs.get(key)
            if r is None:
                cls = f"block v{fork_at(o['block_height'])} tx v{o['tx_version']}"
                entry = missing.setdefault(cls, {"outputs": 0, "amount": 0, "unspent_amount": 0})
                entry["outputs"] += 1
                entry["amount"] += o["amount"]
                if not o["spent"]:
                    entry["unspent_amount"] += o["amount"]
                continue
            sub = r["subaddress_index"] or [0, 0]
            checks = [
                ("amount", o["amount"], r["amount"]),
                ("key_image", o["key_image"], r["key_image"]),
                ("global_index", o["global_index"], r["global_index"]),
                ("spent", o["spent"], r["spent"]),
                ("block_height", o["block_height"], r["block_height"]),
                ("tx_hash", o["tx_hash"], r["tx_hash"]),
                ("subaddress", [o["subaddr_index"]["major"], o["subaddr_index"]["minor"]], list(sub)),
            ]
            if o["tx_hash"] in w2_pids:
                checks.append(("payment_id", w2_pids[o["tx_hash"]], r["payment_id"]))
            if not o["spent"]:
                checks.append(("unlocked", o["unlocked"], f"{r['tx_hash']}:{r['output_index']}" in rs["spendable"]))
            problems = [check for check in checks if check[1] != check[2]]
            if problems:
                mismatches.append({"pubkey": key, "block": o["block_height"], "problems": problems})
            else:
                matched += 1
        w2_balance = w2["accounts"]["total_balance"]
        rs_balance = rs["balance_confirmed"] + rs["balance_unconfirmed"]
        summary[name] = {
            "height": w2["daemon_height"],
            "wallet2_outputs": len(w2_outs), "rust_outputs": len(rs_outs), "matched": matched,
            "missing_in_rust": missing, "field_mismatch": mismatches,
            "unknown_to_wallet2": [key for key in rs_outs if key not in w2_outs],
            "wallet2_balance": w2_balance, "wallet2_unlocked": w2["accounts"]["total_unlocked_balance"],
            "rust_balance": rs_balance, "rust_unlocked": rs["balance_confirmed"],
            "balance_shortfall": w2_balance - rs_balance,
            "rust_scan_error": rs["scan_error"],
        }
    with open(os.path.join(out_dir, "diff.json"), "w") as handle:
        json.dump(summary, handle, indent=1)
    print(json.dumps(summary, indent=1))


def up():
    state = L.load_state()
    daemon = state.get("daemon", {})
    if not (daemon.get("pid") and L.pid_alive(daemon["pid"])):
        L.start_daemon(state)
    staged = L.Daemon(state).call("hard_fork_info", {"version": 2})["earliest_height"]
    if staged != fork_height(2):
        L.stop_daemon(state)
        raise SystemExit(
            f"PRERCT_MONEROD starts block version 2 at height {staged}, not {fork_height(2)}: "
            "it was not built with scripts/prerct/staged-regtest-forks.patch"
        )
    if not os.path.exists(EXTRA_SEND):
        raise SystemExit(f"{EXTRA_SEND} is missing; build it with scripts/prerct/build_w2_extra_send.sh")
    for name in NAMES:
        wallet = L.Wallet(state, name)
        if wallet.open_or_create() and name in ("alice", "carol"):
            wallet.call("create_account", {"label": "second"})
            wallet.call("create_address", {"account_index": 0, "count": 6})
            wallet.call("create_address", {"account_index": 1, "count": 3})
    return state


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    argument = sys.argv[2] if len(sys.argv) > 2 else None
    if command == "up":
        print("export MONERO_REGTEST_RPC=" + up()["daemon"]["url"])
    elif command == "stop":
        L.stop_all(L.load_state())
    elif command == "reset":
        L.stop_all(L.load_state())
        for entry in ("regtest", "wallets", "ringdb", "dumps"):
            shutil.rmtree(os.path.join(L.RUN, entry), ignore_errors=True)
        for entry in os.listdir(L.RUN) if os.path.isdir(L.RUN) else []:
            if entry.startswith("wallet-rpc-") or entry == "state.json":
                os.remove(os.path.join(L.RUN, entry))
    elif command == "diff" and argument:
        diff(argument)
    elif command == "all" and argument:
        state = up()
        chain = Chain()
        chain.build()
        out_dir = chain.w2dump(argument)
        print("export MONERO_REGTEST_RPC=" + state["daemon"]["url"])
        print("export PRERCT_FIXTURES=" + out_dir)
    elif command in ("build", "rust-sends", "restart-daemon") or (command in ("w2dump", "rsdump", "variants") and argument):
        chain = Chain()
        if command == "build":
            chain.build()
        elif command == "w2dump":
            print("export PRERCT_FIXTURES=" + chain.w2dump(argument))
        elif command == "rsdump":
            chain.rsdump(argument)
        elif command == "variants":
            chain.variants(argument)
        elif command == "rust-sends":
            chain.rust_sends()
        else:
            chain.restart_daemon(False, disable_rpc_ban="--disable-rpc-ban" in sys.argv)
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
