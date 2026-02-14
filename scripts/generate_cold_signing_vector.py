#!/usr/bin/env python3
"""Generate a Monero regtest cold-signing vector bundle.

This follows the core flow from Monero's `tests/functional_tests/cold_signing.py`:

1. Restore a full offline wallet from Monero's deterministic test seed.
2. Create an online view-only wallet from the address and private view key.
3. Mine regtest outputs to the wallet.
4. Export/import outputs, export/import key images, create an unsigned txset,
   sign it offline, and submit the signed txset online.

The script expects official Monero CLI binaries to already exist. A typical setup:

    mkdir -p /tmp/monero-vector-tools
    cd /tmp/monero-vector-tools
    curl -fLO https://downloads.getmonero.org/cli/monero-linux-x64-v0.18.5.0.tar.bz2
    sha256sum monero-linux-x64-v0.18.5.0.tar.bz2
    tar -xf monero-linux-x64-v0.18.5.0.tar.bz2
    scripts/generate_cold_signing_vector.py \
      --monero-bin-dir /tmp/monero-vector-tools/monero-x86_64-linux-gnu-v0.18.5.0

Add `--verify-rust-rebuilt-submit` to sign the generated wallet2 unsigned txset
through `monero-rust`, repackage it as `signed_monero_tx`, and verify Monero's
own wallet RPC accepts that rebuilt container via `submit_transfer`.

Add `--verify-rust-rebuilt-cli-submit` to submit the rebuilt `signed_monero_tx`
file through `monero-wallet-cli submit_transfer` instead of wallet RPC.
"""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request


SEED = (
    "velvet lymph giddy number token physics poetry unquoted nibs useful sabotage "
    "limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing "
    "fences goat unquoted"
)
STANDARD_ADDRESS = (
    "42ey1afDFnn4886T7196doS9GPMzexD9gXpsZJDwVjeRVdFCSoHnv7KPbBeGpzJBzHRCAs9UxqeoyFQMYbqSWYTfJJQAWDm"
)
TRANSFER_AMOUNT_ATOMIC = 1_000_000_000_000
BLOCKS_TO_MINE = 80
RING_SIZE = 16


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def free_port() -> int:
    with contextlib.closing(socket.socket()) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def wait_port(port: int, timeout: float = 30.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with contextlib.closing(socket.socket()) as s:
            s.settimeout(0.2)
            if s.connect_ex(("127.0.0.1", port)) == 0:
                return
        time.sleep(0.1)
    raise RuntimeError(f"port {port} did not open within {timeout}s")


def rpc(port: int, method: str, params: dict | None = None) -> dict:
    body = json.dumps(
        {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}}
    ).encode("utf-8")
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/json_rpc",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=90) as response:
        payload = json.loads(response.read().decode("utf-8"))
    if "error" in payload:
        raise RuntimeError(f"{method} failed: {payload['error']}")
    return payload.get("result", {})


def run_version(binary: Path) -> str:
    out = subprocess.check_output([str(binary), "--version"], text=True)
    return out.splitlines()[0].strip()


def write_hex_file(path: Path, hex_value: str) -> None:
    path.write_bytes(bytes.fromhex(hex_value))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--monero-bin-dir",
        type=Path,
        required=True,
        help="Directory containing monerod and monero-wallet-rpc.",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=Path("rust/monero-rust/tests/vectors/cold_signing_regtest_v0_18_5_0"),
    )
    parser.add_argument(
        "--verify-rust-rebuilt-submit",
        action="store_true",
        help=(
            "Rebuild the signed_monero_tx with monero-rust and submit that "
            "container through monero-wallet-rpc instead of the wallet-rpc "
            "signed container."
        ),
    )
    parser.add_argument(
        "--verify-rust-rebuilt-cli-submit",
        action="store_true",
        help=(
            "Rebuild the signed_monero_tx with monero-rust and submit the "
            "rebuilt file through monero-wallet-cli submit_transfer."
        ),
    )
    parser.add_argument("--cargo-bin", default="cargo")
    parser.add_argument("--rust-toolchain", default="1.86")
    parser.add_argument("--keep-temp", action="store_true")
    args = parser.parse_args()

    monerod = args.monero_bin_dir / "monerod"
    wallet_rpc = args.monero_bin_dir / "monero-wallet-rpc"
    wallet_cli = args.monero_bin_dir / "monero-wallet-cli"
    if not monerod.exists() or not wallet_rpc.exists():
        raise SystemExit("monerod and monero-wallet-rpc are required")
    if args.verify_rust_rebuilt_cli_submit and not wallet_cli.exists():
        raise SystemExit("monero-wallet-cli is required for CLI submit verification")

    work = Path(tempfile.mkdtemp(prefix="monero-cold-signing-vector-"))
    processes: list[subprocess.Popen] = []
    logs: list[object] = []
    daemon_rpc = free_port()
    daemon_p2p = free_port()
    daemon_zmq = free_port()
    hot_rpc = free_port()
    cold_rpc = free_port()

    try:
        daemon_log = (work / "monerod.log").open("wb")
        logs.append(daemon_log)
        processes.append(
            subprocess.Popen(
                [
                    str(monerod),
                    "--regtest",
                    "--fixed-difficulty",
                    "1",
                    "--p2p-bind-port",
                    str(daemon_p2p),
                    "--rpc-bind-port",
                    str(daemon_rpc),
                    "--zmq-rpc-bind-port",
                    str(daemon_zmq),
                    "--non-interactive",
                    "--offline",
                    "--disable-dns-checkpoints",
                    "--check-updates",
                    "disabled",
                    "--rpc-ssl",
                    "disabled",
                    "--data-dir",
                    str(work / "daemon"),
                    "--log-level",
                    "1",
                    "--no-igd",
                    "--hide-my-port",
                ],
                stdout=daemon_log,
                stderr=subprocess.STDOUT,
            )
        )
        wait_port(daemon_rpc)

        for name, port, extra in [
            ("hot", hot_rpc, ["--daemon-address", f"127.0.0.1:{daemon_rpc}"]),
            ("cold", cold_rpc, ["--offline"]),
        ]:
            wallet_dir = work / name
            wallet_dir.mkdir()
            log = (work / f"{name}.log").open("wb")
            logs.append(log)
            processes.append(
                subprocess.Popen(
                    [
                        str(wallet_rpc),
                        "--wallet-dir",
                        str(wallet_dir),
                        "--rpc-bind-ip",
                        "127.0.0.1",
                        "--rpc-bind-port",
                        str(port),
                        "--rpc-ssl",
                        "disabled",
                        "--daemon-ssl",
                        "disabled",
                        "--log-level",
                        "1",
                        "--allow-mismatched-daemon-version",
                        "--disable-rpc-login",
                        *extra,
                    ],
                    stdout=log,
                    stderr=subprocess.STDOUT,
                )
            )
        wait_port(hot_rpc)
        wait_port(cold_rpc)

        cold_restore = rpc(
            cold_rpc,
            "restore_deterministic_wallet",
            {
                "filename": "cold",
                "password": "",
                "seed": SEED,
                "restore_height": 0,
                "autosave_current": True,
            },
        )
        view_key = rpc(cold_rpc, "query_key", {"key_type": "view_key"})["key"]
        spend_key = rpc(cold_rpc, "query_key", {"key_type": "spend_key"})["key"]
        rpc(
            hot_rpc,
            "generate_from_keys",
            {
                "filename": "hot",
                "password": "",
                "address": STANDARD_ADDRESS,
                "viewkey": view_key,
                "restore_height": 0,
                "autosave_current": True,
            },
        )

        mining = rpc(
            daemon_rpc,
            "generateblocks",
            {"wallet_address": STANDARD_ADDRESS, "amount_of_blocks": BLOCKS_TO_MINE},
        )
        hot_refresh = rpc(hot_rpc, "refresh")
        incoming = rpc(hot_rpc, "incoming_transfers", {"transfer_type": "all"})

        outputs_export = rpc(hot_rpc, "export_outputs", {"all": True})
        outputs_data_hex = outputs_export["outputs_data_hex"]
        outputs_import = rpc(cold_rpc, "import_outputs", {"outputs_data_hex": outputs_data_hex})

        key_images = rpc(cold_rpc, "export_key_images", {"all": True})
        key_image_import = rpc(
            hot_rpc,
            "import_key_images",
            {
                "signed_key_images": key_images["signed_key_images"],
                "offset": key_images["offset"],
            },
        )

        transfer = rpc(
            hot_rpc,
            "transfer",
            {
                "destinations": [
                    {"address": STANDARD_ADDRESS, "amount": TRANSFER_AMOUNT_ATOMIC}
                ],
                "ring_size": RING_SIZE,
                "get_tx_key": False,
            },
        )
        description = rpc(
            cold_rpc, "describe_transfer", {"unsigned_txset": transfer["unsigned_txset"]}
        )
        signed = rpc(cold_rpc, "sign_transfer", {"unsigned_txset": transfer["unsigned_txset"]})
        signed_txset_for_submit = signed["signed_txset"]
        rebuilt_signed_sha256 = None
        rebuilt_path = work / "monero_rust_rebuilt_signed_monero_tx"
        if args.verify_rust_rebuilt_submit or args.verify_rust_rebuilt_cli_submit:
            repo_root = Path(__file__).resolve().parents[1]
            unsigned_path = work / "unsigned_monero_tx"
            reference_signed_path = work / "monero_wallet_rpc_signed_monero_tx"
            write_hex_file(unsigned_path, transfer["unsigned_txset"])
            write_hex_file(reference_signed_path, signed["signed_txset"])
            cargo_cmd = [
                args.cargo_bin,
                f"+{args.rust_toolchain}",
                "run",
                "-p",
                "monero-rust",
                "--example",
                "rebuild_wallet2_signed_txset",
                "--",
                "--unsigned-txset",
                str(unsigned_path),
                "--reference-signed-txset",
                str(reference_signed_path),
                "--spend-key-hex",
                spend_key,
                "--view-key-hex",
                view_key,
                "--network",
                "mainnet",
                "--out",
                str(rebuilt_path),
            ]
            subprocess.run(cargo_cmd, cwd=repo_root / "rust", check=True)
            signed_txset_for_submit = rebuilt_path.read_bytes().hex()
            rebuilt_signed_sha256 = sha256_file(rebuilt_path)
        cli_submit_stdout = None
        if args.verify_rust_rebuilt_cli_submit:
            rpc(hot_rpc, "close_wallet")
            shutil.copy2(rebuilt_path, work / "signed_monero_tx")
            cli_cmd = [
                str(wallet_cli),
                "--wallet-file",
                str(work / "hot" / "hot"),
                "--password",
                "",
                "--daemon-address",
                f"127.0.0.1:{daemon_rpc}",
                "--daemon-ssl",
                "disabled",
                "--allow-mismatched-daemon-version",
                "--trusted-daemon",
                "submit_transfer",
            ]
            completed = subprocess.run(
                cli_cmd,
                input="Y\n",
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                cwd=work,
            )
            submitted_hashes = sorted(set(re.findall(r"\b[0-9a-f]{64}\b", completed.stdout)))
            if completed.returncode != 0 or "Error:" in completed.stdout or not submitted_hashes:
                raise RuntimeError(
                    "monero-wallet-cli submit_transfer failed:\n" + completed.stdout
                )
            cli_submit_stdout = completed.stdout
            submitted = {"tx_hash_list": submitted_hashes}
        else:
            submitted = rpc(hot_rpc, "submit_transfer", {"tx_data_hex": signed_txset_for_submit})

        out_dir = args.out_dir
        if out_dir.exists():
            shutil.rmtree(out_dir)
        out_dir.mkdir(parents=True)

        shutil.copy2(work / "hot" / "hot.keys", out_dir / "hot_view_only.keys")
        shutil.copy2(work / "cold" / "cold.keys", out_dir / "cold_full.keys")
        write_hex_file(out_dir / "outputs", outputs_data_hex)
        write_hex_file(out_dir / "unsigned_monero_tx", transfer["unsigned_txset"])
        write_hex_file(out_dir / "signed_monero_tx", signed_txset_for_submit)
        if args.verify_rust_rebuilt_submit or args.verify_rust_rebuilt_cli_submit:
            write_hex_file(out_dir / "monero_wallet_rpc_signed_monero_tx", signed["signed_txset"])
        (out_dir / "key_images_rpc.json").write_text(
            json.dumps(key_images, indent=2, sort_keys=True) + "\n"
        )
        (out_dir / "transfer_description.json").write_text(
            json.dumps(description, indent=2, sort_keys=True) + "\n"
        )

        files = sorted(
            p.name for p in out_dir.iterdir() if p.is_file() and p.name != "metadata.json"
        )
        metadata = {
            "schema": "monero-rust cold signing vector v1",
            "generated_at_utc": dt.datetime.now(dt.timezone.utc)
            .replace(microsecond=0)
            .isoformat(),
            "source_flow": "monero-project/monero tests/functional_tests/cold_signing.py",
            "network": "regtest",
            "monero": {
                "monerod_version": run_version(monerod),
                "wallet_rpc_version": run_version(wallet_rpc),
                "monerod_sha256": sha256_file(monerod),
                "wallet_rpc_sha256": sha256_file(wallet_rpc),
            },
            "wallet": {
                "seed": SEED,
                "standard_address": STANDARD_ADDRESS,
                "view_key": view_key,
                "spend_key": spend_key,
                "cold_restore_address": cold_restore["address"],
            },
            "flow": {
                "blocks_mined": BLOCKS_TO_MINE,
                "chain_height_after_mining": mining["height"],
                "hot_refresh_blocks_fetched": hot_refresh["blocks_fetched"],
                "incoming_transfer_count": len(incoming.get("transfers", [])),
                "outputs_imported": outputs_import["num_imported"],
                "key_image_offset": key_images["offset"],
                "key_image_count": len(key_images["signed_key_images"]),
                "key_image_import": key_image_import,
                "transfer_amount_atomic": TRANSFER_AMOUNT_ATOMIC,
                "ring_size": RING_SIZE,
                "unsigned_tx_hash": transfer["tx_hash"],
                "signed_tx_hash_list": signed["tx_hash_list"],
                "signed_txset_source": (
                    "monero-rust rebuilt from app signer"
                    if args.verify_rust_rebuilt_submit or args.verify_rust_rebuilt_cli_submit
                    else "monero-wallet-rpc sign_transfer"
                ),
                "submit_method": (
                    "monero-wallet-cli submit_transfer file"
                    if args.verify_rust_rebuilt_cli_submit
                    else "monero-wallet-rpc submit_transfer hex"
                ),
                "cli_submit_stdout": cli_submit_stdout,
                "rebuilt_signed_txset_sha256": rebuilt_signed_sha256,
                "submitted_tx_hash_list": submitted["tx_hash_list"],
            },
            "artifacts": {
                name: {"bytes": (out_dir / name).stat().st_size, "sha256": sha256_file(out_dir / name)}
                for name in files
            },
        }
        (out_dir / "metadata.json").write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n"
        )

        print(f"wrote {out_dir}")
        print(json.dumps(metadata["flow"], indent=2, sort_keys=True))
    finally:
        for process in reversed(processes):
            with contextlib.suppress(Exception):
                process.terminate()
        for process in reversed(processes):
            with contextlib.suppress(Exception):
                process.wait(timeout=10)
            if process.poll() is None:
                with contextlib.suppress(Exception):
                    process.kill()
        for log in logs:
            with contextlib.suppress(Exception):
                log.close()
        if args.keep_temp:
            print(f"kept temporary directory {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
