#!/usr/bin/env python3
"""Run Ledger app-monero Speculos transaction-flow tests and capture output.

This is intentionally host-only. It expects a local LedgerHQ/app-monero checkout
that has already been built for the requested device in Ledger's app-builder
environment. The script records enough provenance for the captured run to be
reviewed later from this repository.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path


DEFAULT_TESTS = (
    "tests/test_sig.py",
    "tests/test_sig_real_simple.py",
    "tests/test_sig_real_amount_zero.py",
)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def run_text(args: list[str], cwd: Path) -> str | None:
    try:
        return subprocess.check_output(args, cwd=cwd, text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def device_elf(app_dir: Path, device: str) -> Path:
    build_dir_by_device = {
        "nanos": "nanos",
        "nanox": "nanox",
        "nanosp": "nanos2",
        "stax": "stax",
        "flex": "flex",
    }
    build_dir = build_dir_by_device.get(device, device)
    return app_dir / "build" / build_dir / "bin" / "app.elf"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run selected LedgerHQ/app-monero Speculos tx-flow tests and capture a manifest."
    )
    parser.add_argument(
        "--app-monero-dir",
        default=os.environ.get("LEDGER_APP_MONERO_DIR"),
        help="Path to a local LedgerHQ/app-monero checkout. Defaults to LEDGER_APP_MONERO_DIR.",
    )
    parser.add_argument("--device", default="nanosp", help="pytest --device value. Default: nanosp.")
    parser.add_argument(
        "--output-dir",
        default=str(repo_root() / "rust/monero-rust/tests/vectors/ledger_app_speculos/tx_flow_runs"),
        help="Directory where run artifacts are written.",
    )
    parser.add_argument(
        "--test",
        action="append",
        dest="tests",
        help="Upstream pytest path or selector. May be repeated. Defaults to the tx-flow tests.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the command and manifest path without running pytest.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.app_monero_dir:
        print(
            "Set LEDGER_APP_MONERO_DIR or pass --app-monero-dir pointing to LedgerHQ/app-monero.",
            file=sys.stderr,
        )
        return 2

    app_dir = Path(args.app_monero_dir).expanduser().resolve()
    if not app_dir.is_dir():
        print(f"app-monero checkout not found: {app_dir}", file=sys.stderr)
        return 2

    selected_tests = tuple(args.tests or DEFAULT_TESTS)
    missing_tests = [test for test in selected_tests if not (app_dir / test.split("::", 1)[0]).exists()]
    if missing_tests:
        print(f"Missing upstream test files in {app_dir}: {', '.join(missing_tests)}", file=sys.stderr)
        return 2

    elf = device_elf(app_dir, args.device)
    if not elf.exists():
        print(
            f"Built Ledger app ELF not found: {elf}\n"
            "Build app-monero first, for example in Ledger's app-builder image with "
            "`make DEBUG=1 BOLOS_SDK=$NANOSP_SDK`.",
            file=sys.stderr,
        )
        return 2

    output_dir = Path(args.output_dir).expanduser().resolve()
    timestamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = output_dir / f"{timestamp}-{args.device}"
    stdout_path = run_dir / "pytest.stdout.log"
    stderr_path = run_dir / "pytest.stderr.log"
    manifest_path = run_dir / "manifest.json"

    command = [
        sys.executable,
        "-m",
        "pytest",
        *selected_tests,
        "--tb=short",
        "-v",
        "--capture=tee-sys",
        "--device",
        args.device,
    ]

    commit = run_text(["git", "rev-parse", "HEAD"], cwd=app_dir)
    describe = run_text(["git", "describe", "--always", "--dirty"], cwd=app_dir)
    manifest = {
        "schema": "monero-rust ledger app speculos tx-flow run v1",
        "source": "LedgerHQ/app-monero upstream pytest transaction-flow tests",
        "app_monero_dir": str(app_dir),
        "app_monero_commit": commit,
        "app_monero_describe": describe,
        "device": args.device,
        "app_elf": str(elf),
        "app_elf_sha256": sha256_file(elf),
        "selected_tests": list(selected_tests),
        "command": command,
        "started_at_utc": timestamp,
    }

    if args.dry_run:
        print(json.dumps({**manifest, "manifest_path": str(manifest_path)}, indent=2))
        return 0

    run_dir.mkdir(parents=True, exist_ok=False)
    started = dt.datetime.now(dt.timezone.utc)
    proc = subprocess.run(command, cwd=app_dir, text=True, capture_output=True)
    finished = dt.datetime.now(dt.timezone.utc)

    stdout_path.write_text(proc.stdout)
    stderr_path.write_text(proc.stderr)
    manifest.update(
        {
            "finished_at_utc": finished.strftime("%Y%m%dT%H%M%SZ"),
            "duration_seconds": round((finished - started).total_seconds(), 3),
            "returncode": proc.returncode,
            "stdout": str(stdout_path.relative_to(run_dir)),
            "stderr": str(stderr_path.relative_to(run_dir)),
        }
    )
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")

    print(f"Wrote Ledger Speculos tx-flow run artifacts to {run_dir}")
    return proc.returncode


if __name__ == "__main__":
    raise SystemExit(main())
