#!/usr/bin/env python3
"""wallet2 <-> monero-rust cold-signing file round trips on regtest.

    scripts/cold_signing_wallet2_interop.py --part static --run-dir <dir> --bin-dir <monero bin dir>
    eval "$(scripts/regtest-node.sh start)"
    scripts/cold_signing_wallet2_interop.py --part live --run-dir <dir> --bin-dir <monero bin dir>
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
import traceback
import urllib.request

SEED = (
    "velvet lymph giddy number token physics poetry unquoted nibs useful sabotage "
    "limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing "
    "fences goat unquoted"
)
STANDARD_ADDRESS = (
    "42ey1afDFnn4886T7196doS9GPMzexD9gXpsZJDwVjeRVdFCSoHnv7KPbBeGpzJBzHRCAs9UxqeoyFQMYbqSWYTfJJQAWDm"
)
XMR = 1_000_000_000_000


def free_port() -> int:
    with contextlib.closing(socket.socket()) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def wait_port(port: int, timeout: float = 60.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with contextlib.closing(socket.socket()) as s:
            s.settimeout(0.2)
            if s.connect_ex(("127.0.0.1", port)) == 0:
                return
        time.sleep(0.1)
    raise RuntimeError(f"port {port} did not open within {timeout}s")


class RpcError(RuntimeError):
    pass


def rpc_url(url: str, method: str, params: dict | None = None, timeout: float = 180) -> dict:
    body = json.dumps({"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}}).encode()
    req = urllib.request.Request(url + "/json_rpc", data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as response:
        payload = json.loads(response.read().decode())
    if "error" in payload:
        raise RpcError(f"{method}: {payload['error']}")
    return payload.get("result", {})


class Ctx:
    def __init__(self, args):
        self.bin = Path(args.bin_dir)
        self.run = Path(args.run_dir)
        self.tool_bin = Path(args.tool) if args.tool else build_tool()
        self.daemon = args.daemon
        self.procs: list[subprocess.Popen] = []
        self.logs = []
        self.transcript: list[dict] = []
        self.failed: list[str] = []
        self.view = None
        self.spend = None

    def record(self, step: str, **data):
        entry = {"step": step, **data}
        self.transcript.append(entry)
        print(json.dumps(entry)[:1500], flush=True)

    def check(self, step: str, ok: bool, **data):
        self.record(step, passed=bool(ok), **data)
        if not ok:
            self.failed.append(step if "case" not in data else f"{step}[{data['case']}]")

    def start_wallet_rpc(self, name: str, offline: bool) -> int:
        port = free_port()
        wallet_dir = self.run / "wallets" / name
        wallet_dir.mkdir(parents=True, exist_ok=True)
        log = (self.run / f"wallet-rpc-{name}.stdout").open("wb")
        self.logs.append(log)
        cmd = [
            str(self.bin / "monero-wallet-rpc"),
            "--wallet-dir", str(wallet_dir),
            "--rpc-bind-ip", "127.0.0.1",
            "--rpc-bind-port", str(port),
            "--rpc-ssl", "disabled",
            "--daemon-ssl", "disabled",
            "--log-level", "1",
            "--log-file", str(self.run / f"wallet-rpc-{name}.log"),
            "--allow-mismatched-daemon-version",
            # Keep wallet2 ring records out of the user's real ~/.shared-ringdb.
            "--shared-ringdb-dir", str(self.run / "ringdb"),
            "--disable-rpc-login",
        ]
        if offline:
            cmd.append("--offline")
        else:
            cmd += ["--daemon-address", self.daemon.replace("http://", ""), "--trusted-daemon"]
        self.procs.append(subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT))
        wait_port(port)
        return port

    def stop_all(self):
        for proc in reversed(self.procs):
            if proc.poll() is None:
                proc.terminate()
        for proc in reversed(self.procs):
            try:
                proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                proc.kill()
        for log in self.logs:
            log.close()

    def tool(self, cmd: str, **env) -> dict:
        full_env = dict(os.environ)
        full_env["COLD_SIGNING_CMD"] = cmd
        for key, value in env.items():
            full_env["COLD_SIGNING_" + key.upper()] = str(value)
        out = subprocess.run(
            [str(self.tool_bin), "--ignored", "--nocapture", "--exact", "tool"],
            env=full_env, capture_output=True, text=True, timeout=900,
        )
        for line in out.stdout.splitlines():
            if "COLD_SIGNING_JSON " in line:
                return json.loads(line.split("COLD_SIGNING_JSON ", 1)[1])
        return {"ok": False, "tool_failed": True, "stdout": out.stdout[-2000:], "stderr": out.stderr[-2000:]}

    def cli(self, wallet_file: Path, password: str, offline: bool, command: list[str],
            stdin: str = "y\ny\ny\n", cwd: Path | None = None, extra: list[str] | None = None) -> dict:
        cmd = [
            str(self.bin / "monero-wallet-cli"),
            "--wallet-file", str(wallet_file),
            "--password", password,
            "--log-file", str(self.run / "wallet-cli.log"),
            "--allow-mismatched-daemon-version",
            # Keep wallet2 ring records out of the user's real ~/.shared-ringdb.
            "--shared-ringdb-dir", str(self.run / "ringdb"),
        ]
        if offline:
            cmd.append("--offline")
        else:
            cmd += ["--daemon-address", self.daemon.replace("http://", ""), "--trusted-daemon",
                    "--daemon-ssl", "disabled"]
        cmd += (extra or []) + command
        out = subprocess.run(cmd, input=stdin, capture_output=True, text=True, timeout=300,
                             cwd=str(cwd or self.run))
        return {"rc": out.returncode, "stdout": out.stdout[-3000:], "stderr": out.stderr[-1500:]}


REPO = Path(__file__).resolve().parents[1]
CRATE = REPO / "rust" / "monero-rust"
VECTORS = CRATE / "tests" / "vectors" / "cold_signing_regtest_v0_18_5_0"


def build_tool() -> Path:
    out = subprocess.run(
        ["cargo", "test", "--release", "--features", "mock-rpc,test-helpers",
         "--test", "cold_signing_wallet2_tool", "--no-run", "--message-format=json"],
        cwd=str(CRATE), capture_output=True, text=True, check=True,
    )
    for line in out.stdout.splitlines():
        with contextlib.suppress(ValueError):
            message = json.loads(line)
            if (message.get("reason") == "compiler-artifact" and message.get("executable")
                    and message.get("target", {}).get("name") == "cold_signing_wallet2_tool"):
                return Path(message["executable"])
    raise SystemExit("could not build cold_signing_wallet2_tool")


def part_static(ctx: Ctx):
    cold_port = ctx.start_wallet_rpc("static-cold", offline=True)
    cold = f"http://127.0.0.1:{cold_port}"
    rpc_url(cold, "restore_deterministic_wallet",
            {"filename": "cold", "password": "", "seed": SEED, "restore_height": 0})
    outputs_hex = (VECTORS / "outputs").read_bytes().hex()
    imported = rpc_url(cold, "import_outputs", {"outputs_data_hex": outputs_hex})
    ctx.check("static.import_outputs", imported.get("num_imported") == 80, result=imported)

    crafted = ctx.run / "crafted"
    ctx.record("static.craft", **ctx.tool("craft", out=crafted))
    cold_keys = ctx.tool("read-keys", keys_file=VECTORS / "cold_full.keys")
    spend, view = cold_keys["spend_secret"], cold_keys["view_secret"]

    for name in ["control", "unlock_time", "original_mismatch", "split_mismatch",
                 "duplicate_input", "no_view_tags"]:
        data = (crafted / name).read_bytes().hex()
        try:
            signed = rpc_url(cold, "sign_transfer", {"unsigned_txset": data})
            wallet2 = {"ok": True, "tx_hash_list": signed.get("tx_hash_list")}
        except Exception as error:  # noqa: BLE001
            wallet2 = {"ok": False, "error": str(error)[:400]}
        signed = ctx.tool("sign", unsigned=crafted / name, spend=spend, view=view, network="mainnet",
                          outputs=VECTORS / "outputs", out=ctx.run / f"crafted-{name}.signed")
        crate = {k: signed.get(k) for k in ("ok", "stage", "error", "tx_ids")}
        ctx.check("static.signer_parity", wallet2["ok"] == bool(crate["ok"]) and
                  (name != "control" or wallet2["ok"]), case=name, wallet2=wallet2, crate=crate)


def part_live(ctx: Ctx):
    daemon = ctx.daemon
    hot_port = ctx.start_wallet_rpc("hot", offline=False)
    cold_port = ctx.start_wallet_rpc("cold", offline=True)
    recv_port = ctx.start_wallet_rpc("recv", offline=False)
    hot, cold, recv = (f"http://127.0.0.1:{p}" for p in (hot_port, cold_port, recv_port))
    wallets = ctx.run / "wallets"
    art = ctx.run / "artifacts"
    art.mkdir(exist_ok=True)

    rpc_url(cold, "restore_deterministic_wallet",
            {"filename": "cold", "password": "", "seed": SEED, "restore_height": 0})
    view = rpc_url(cold, "query_key", {"key_type": "view_key"})["key"]
    spend = rpc_url(cold, "query_key", {"key_type": "spend_key"})["key"]
    rpc_url(hot, "generate_from_keys",
            {"filename": "hot", "password": "", "address": STANDARD_ADDRESS, "viewkey": view,
             "restore_height": 0})
    rpc_url(recv, "create_wallet", {"filename": "recv", "password": "", "language": "English"})
    recv_main = rpc_url(recv, "get_address", {"account_index": 0})["address"]
    recv_sub = rpc_url(recv, "create_address", {"account_index": 0})["address"]
    recv_integrated = rpc_url(recv, "make_integrated_address", {})
    for wallet in (hot, cold):
        rpc_url(wallet, "create_account", {})
        for _ in range(3):
            rpc_url(wallet, "create_address", {"account_index": 0})
        for _ in range(2):
            rpc_url(wallet, "create_address", {"account_index": 1})

    def own(account: int, index: int) -> str:
        return rpc_url(hot, "get_address", {"account_index": account, "address_index": [index]})["addresses"][0]["address"]

    own_0_3, own_1_0, own_1_2 = own(0, 3), own(1, 0), own(1, 2)

    def mine(blocks: int):
        rpc_url(daemon, "generateblocks", {"wallet_address": STANDARD_ADDRESS, "amount_of_blocks": blocks})

    def sync_wallet2():
        rpc_url(hot, "refresh")
        outputs_hex = rpc_url(hot, "export_outputs", {"all": True})["outputs_data_hex"]
        rpc_url(cold, "import_outputs", {"outputs_data_hex": outputs_hex})
        images = rpc_url(cold, "export_key_images", {"all": True})
        rpc_url(hot, "import_key_images",
                {"signed_key_images": images["signed_key_images"], "offset": images.get("offset", 0)})
        path = art / "outputs"
        path.write_bytes(bytes.fromhex(outputs_hex))
        return path, images

    def with_closed(wallet_url: str, name: str, fn):
        rpc_url(wallet_url, "close_wallet")
        try:
            return fn()
        finally:
            rpc_url(wallet_url, "open_wallet", {"filename": name, "password": ""})

    height = rpc_url(daemon, "get_block_count")["count"]
    if height < 100:
        mine(110 - height)
    outputs_path, _ = sync_wallet2()

    unsigned = rpc_url(hot, "transfer", {"destinations": [
        {"address": own_0_3, "amount": 2 * XMR},
        {"address": own_1_0, "amount": 3 * XMR},
        {"address": own_1_2, "amount": 3 * XMR // 2},
    ], "ring_size": 16})["unsigned_txset"]
    signed = rpc_url(cold, "sign_transfer", {"unsigned_txset": unsigned})
    (art / "wallet2_signed").write_bytes(bytes.fromhex(signed["signed_txset"]))
    (art / "wallet2_unsigned").write_bytes(bytes.fromhex(unsigned))
    parsed = ctx.tool("parse-signed", signed=art / "wallet2_signed", view=view,
                      unsigned=art / "wallet2_unsigned")
    ctx.check("live.wallet2_signed_txset_read_by_crate",
              parsed.get("ok") and parsed.get("tx_hashes") == signed.get("tx_hash_list"),
              wallet2_tx=signed.get("tx_hash_list"), crate=parsed)
    rpc_url(hot, "submit_transfer", {"tx_data_hex": signed["signed_txset"]})
    mine(12)
    outputs_path, images = sync_wallet2()
    wallet2_images = [e["key_image"] for e in images["signed_key_images"]]

    wallet2_count = sum(
        len(rpc_url(hot, "incoming_transfers", {"transfer_type": "all", "account_index": a}).get("transfers", []))
        for a in (0, 1))
    parsed = ctx.tool("parse-outputs", outputs=outputs_path, view=view)
    ctx.check("live.output_export_read_by_crate",
              parsed.get("ok") and parsed.get("count") == wallet2_count,
              crate_count=parsed.get("count"), wallet2_count=wallet2_count, error=parsed.get("error"))

    crate_ki = ctx.tool("export-key-images", outputs=outputs_path, view=view, spend=spend,
                        out=art / "crate_key_images")
    ctx.check("live.key_images_equal_wallet2s", crate_ki.get("key_images") == wallet2_images,
              count=crate_ki.get("count"))
    imported = with_closed(hot, "hot", lambda: ctx.cli(
        wallets / "hot" / "hot", "", False, ["import_key_images", str(art / "crate_key_images")]))
    ctx.check("live.crate_key_image_file_imported_by_wallet2_cli",
              "Signed key images imported" in imported["stdout"], tail=imported["stdout"][-200:])

    def export_from_cold():
        full = ctx.cli(wallets / "cold" / "cold", "", True,
                       ["export_key_images", "all", str(art / "wallet2_key_images")], stdin="\n\n")
        part = ctx.cli(wallets / "cold" / "cold", "", True,
                       ["export_key_images", str(art / "wallet2_key_images_partial")], stdin="\n\n")
        return full, part
    with_closed(cold, "cold", export_from_cold)
    for name, full in (("wallet2_key_images", True), ("wallet2_key_images_partial", False)):
        result = ctx.tool("import-key-images", outputs=outputs_path, view=view, key_images=art / name)
        ok = bool(result.get("ok")) and (not full or result.get("key_images") == wallet2_images)
        ctx.check("live.wallet2_key_image_file_verified_by_crate", ok, case=name,
                  offset=result.get("offset"), assigned=result.get("assigned"), error=result.get("error"))

    def sign_and_submit(label: str, unsigned_path: Path, expect_recv: int | None):
        nonlocal outputs_path
        entry: dict = {}
        ok = True
        try:
            described = rpc_url(cold, "describe_transfer", {"unsigned_txset": unsigned_path.read_bytes().hex()})
            summary = described.get("summary") or {}
        except Exception as error:  # noqa: BLE001
            summary = {}
            entry["wallet2_describe_error"] = str(error)[:300]
        signed_path = art / f"{label}.signed"
        signed = ctx.tool("sign", unsigned=unsigned_path, spend=spend, view=view, network="mainnet",
                          outputs=outputs_path, out=signed_path)
        entry["crate_sign"] = {k: signed.get(k) for k in ("ok", "stage", "error", "tx_ids", "fees")}
        ok = ok and bool(signed.get("ok"))
        if signed.get("ok"):
            preview = signed["preview"]
            if summary:
                entry["fee"] = [preview["fee"], summary.get("fee")]
                entry["inputs"] = [preview["input_total"], summary.get("amount_in")]
                ok = ok and preview["fee"] == summary.get("fee") and preview["input_total"] == summary.get("amount_in")
                change = sum(d["amount"] for t in preview["transactions"] for d in t["destinations"] if d["is_change"])
                entry["change"] = [change, summary.get("change_amount")]
                ok = ok and change == summary.get("change_amount")
            try:
                submitted = rpc_url(hot, "submit_transfer", {"tx_data_hex": signed_path.read_bytes().hex()})
                entry["wallet2_submit"] = submitted.get("tx_hash_list")
                ok = ok and submitted.get("tx_hash_list") == signed.get("tx_ids")
            except Exception as error:  # noqa: BLE001
                entry["wallet2_submit_error"] = str(error)[:400]
                ok = False
        mine(12)
        if expect_recv is not None:
            rpc_url(recv, "refresh")
            got = 0
            for tx_id in signed.get("tx_ids") or []:
                with contextlib.suppress(Exception):
                    info = rpc_url(recv, "get_transfer_by_txid", {"txid": tx_id})
                    for transfer in info.get("transfers", [info.get("transfer")]):
                        got += transfer["amount"]
                        entry.setdefault("recv_payment_ids", []).append(transfer.get("payment_id"))
            entry["recv_amount"] = [got, expect_recv]
            ok = ok and got == expect_recv
        with contextlib.suppress(Exception):
            outputs_path, _ = sync_wallet2()
        ctx.check("live.wallet2_unsigned_signed_by_crate_relayed_by_wallet2", ok, case=label, **entry)

    def round_trip(label: str, method: str, params: dict, expect_recv: int | None = None):
        try:
            unsigned_hex = rpc_url(hot, method, params)["unsigned_txset"]
        except Exception as error:  # noqa: BLE001
            ctx.check("live.wallet2_unsigned_signed_by_crate_relayed_by_wallet2", False, case=label,
                      wallet2_unsigned_error=str(error)[:400])
            return
        unsigned_path = art / f"{label}.unsigned"
        unsigned_path.write_bytes(bytes.fromhex(unsigned_hex))
        sign_and_submit(label, unsigned_path, expect_recv)

    round_trip("standard", "transfer",
               {"destinations": [{"address": recv_main, "amount": XMR}], "ring_size": 16}, XMR)
    round_trip("subaddress", "transfer",
               {"destinations": [{"address": recv_sub, "amount": 7 * XMR // 10}], "ring_size": 16}, 7 * XMR // 10)
    round_trip("integrated", "transfer",
               {"destinations": [{"address": recv_integrated["integrated_address"], "amount": XMR // 2}],
                "ring_size": 16}, XMR // 2)
    round_trip("three_destinations", "transfer",
               {"destinations": [{"address": recv_main, "amount": 3 * XMR // 10},
                                 {"address": recv_sub, "amount": 2 * XMR // 10},
                                 {"address": own_0_3, "amount": XMR // 10}], "ring_size": 16}, 5 * XMR // 10)
    round_trip("account_1", "transfer",
               {"destinations": [{"address": recv_main, "amount": 4 * XMR}], "account_index": 1,
                "ring_size": 16}, 4 * XMR)
    round_trip("sweep_subaddress_0_3", "sweep_all",
               {"address": recv_main, "account_index": 0, "subaddr_indices": [3], "ring_size": 16}, None)
    many = [{"address": recv_main if i % 2 else recv_sub, "amount": XMR // 100 * (i + 1)} for i in range(20)]
    round_trip("split_20_destinations", "transfer_split", {"destinations": many, "ring_size": 16},
               sum(d["amount"] for d in many))
    unspent = [t for t in rpc_url(hot, "incoming_transfers", {"transfer_type": "available", "account_index": 0}).get("transfers", [])
               if t.get("unlocked")]
    if unspent:
        round_trip("sweep_single_dummy", "sweep_single",
                   {"address": recv_main, "key_image": unspent[0]["key_image"], "ring_size": 16}, None)

    uri_dir = ctx.run / "cli-uri"
    uri_dir.mkdir(exist_ok=True)
    uri = f"monero:{recv_main}?tx_amount=0.25"
    with_closed(hot, "hot", lambda: ctx.cli(wallets / "hot" / "hot", "", False, ["transfer", uri],
                                            cwd=uri_dir, stdin="\ny\ny\ny\n"))
    if (uri_dir / "unsigned_monero_tx").exists():
        sign_and_submit("uri_destination", uri_dir / "unsigned_monero_tx", XMR // 4)
    else:
        ctx.check("live.wallet2_unsigned_signed_by_crate_relayed_by_wallet2", False, case="uri_destination",
                  error="monero-wallet-cli wrote no unsigned_monero_tx")

    unsigned_hex = rpc_url(hot, "transfer", {"destinations": [{"address": recv_main, "amount": XMR // 5}],
                                             "ring_size": 16})["unsigned_txset"]
    (art / "cli.unsigned").write_bytes(bytes.fromhex(unsigned_hex))
    cli_dir = ctx.run / "cli-submit"
    cli_dir.mkdir(exist_ok=True)
    signed = ctx.tool("sign", unsigned=art / "cli.unsigned", spend=spend, view=view, network="mainnet",
                      outputs=outputs_path, out=cli_dir / "signed_monero_tx")
    submitted = with_closed(hot, "hot", lambda: ctx.cli(wallets / "hot" / "hot", "", False,
                                                        ["submit_transfer"], cwd=cli_dir))
    ctx.check("live.crate_signed_txset_file_submitted_by_wallet2_cli",
              "successfully submitted" in submitted["stdout"], crate_tx=signed.get("tx_ids"),
              tail=submitted["stdout"][-200:])
    mine(12)

    def export_ascii():
        ctx.cli(wallets / "hot" / "hot", "", False, ["set", "export-format", "ascii"], stdin="\n\n")
        ctx.cli(wallets / "hot" / "hot", "", False, ["export_outputs", "all", str(art / "outputs_ascii")], stdin="\n\n")
        ctx.cli(wallets / "hot" / "hot", "", False, ["set", "export-format", "binary"], stdin="\n\n")
    with_closed(hot, "hot", export_ascii)
    armoured = (art / "outputs_ascii").read_bytes()[:33] if (art / "outputs_ascii").exists() else b""
    parsed = ctx.tool("parse-outputs", outputs=art / "outputs_ascii", view=view)
    ctx.check("live.ascii_output_export_read_by_crate",
              armoured.startswith(b"-----BEGIN MoneroAsciiDataV1") and bool(parsed.get("ok")),
              count=parsed.get("count"), error=parsed.get("error"))

    keys = ctx.tool("read-keys", keys_file=wallets / "cold" / "cold.keys")
    ctx.check("live.wallet2_keys_file_read_by_crate",
              keys.get("spend_secret") == spend and keys.get("view_secret") == view and keys.get("mnemonic") == SEED,
              case="full", error=keys.get("error"))
    keys = ctx.tool("read-keys", keys_file=wallets / "hot" / "hot.keys")
    ctx.check("live.wallet2_keys_file_read_by_crate",
              bool(keys.get("watch_only")) and keys.get("view_secret") == view, case="view-only",
              error=keys.get("error"))
    kdir = ctx.run / "keys"
    kdir.mkdir(exist_ok=True)
    ctx.tool("rewrite-keys", keys_file=wallets / "cold" / "cold.keys", new_password="new-password",
             out=kdir / "rewritten.keys")
    opened = ctx.cli(kdir / "rewritten", "new-password", True, ["address"])
    wrong = ctx.cli(kdir / "rewritten", "not-the-password", True, ["address"])
    ctx.check("live.crate_keys_file_opened_by_wallet2_cli",
              STANDARD_ADDRESS in opened["stdout"] and STANDARD_ADDRESS not in wrong["stdout"],
              tail=opened["stdout"][-120:])
    shutil.copy(wallets / "cold" / "cold.keys", kdir / "trailing.keys")
    with (kdir / "trailing.keys").open("ab") as f:
        f.write(b"\x00trailing")
    crate = ctx.tool("read-keys", keys_file=kdir / "trailing.keys")
    opened = ctx.cli(kdir / "trailing", "", True, ["address"])
    ctx.check("live.keys_file_trailing_bytes_parity",
              bool(crate.get("ok")) == (STANDARD_ADDRESS in opened["stdout"]),
              crate_ok=crate.get("ok"), wallet2_tail=(opened["stdout"] + opened["stderr"])[-160:])
    cmd = [str(ctx.bin / "monero-wallet-cli"), "--generate-new-wallet", str(kdir / "kdf2"),
           "--password", "new-password", "--kdf-rounds", "2", "--restore-deterministic-wallet",
           "--electrum-seed", SEED, "--restore-height", "0", "--offline",
           "--shared-ringdb-dir", str(ctx.run / "ringdb"),
           "--log-file", str(ctx.run / "wallet-cli.log"), "address"]
    subprocess.run(cmd, input="\n\n\n\n", capture_output=True, text=True, timeout=300, cwd=str(ctx.run))
    crate = ctx.tool("read-keys", keys_file=kdir / "kdf2.keys", password="new-password", kdf_rounds=2)
    ctx.check("live.wallet2_keys_file_read_by_crate", crate.get("spend_secret") == spend, case="kdf-rounds 2",
              error=crate.get("error"))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--part", choices=["static", "live"], required=True)
    parser.add_argument("--run-dir", required=True)
    parser.add_argument("--bin-dir", default=os.environ.get("MONERO_BIN_DIR", ""))
    parser.add_argument("--daemon", default=os.environ.get("MONERO_REGTEST_RPC", ""))
    parser.add_argument("--tool", default="", help="cold_signing_wallet2_tool test binary; built if omitted")
    args = parser.parse_args()
    if not args.bin_dir:
        raise SystemExit("--bin-dir or MONERO_BIN_DIR is required")
    if args.part == "live" and not args.daemon:
        raise SystemExit("--daemon or MONERO_REGTEST_RPC is required for the live part")
    run = Path(args.run_dir).resolve()
    run.mkdir(parents=True, exist_ok=True)
    args.run_dir = str(run)
    ctx = Ctx(args)
    try:
        if args.part == "static":
            part_static(ctx)
        else:
            part_live(ctx)
    except Exception as error:  # noqa: BLE001
        ctx.check("fatal", False, error=str(error)[:800], trace=traceback.format_exc()[-1500:])
    finally:
        ctx.stop_all()
        (run / f"transcript-{args.part}.json").write_text(json.dumps(ctx.transcript, indent=1))
    print("transcript:", run / f"transcript-{args.part}.json")
    if ctx.failed:
        print("FAILED checks:", *ctx.failed, sep="\n  ")
        sys.exit(1)
    print("all checks passed")


if __name__ == "__main__":
    main()
