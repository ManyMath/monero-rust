"""Shared helpers for the wallet2 fee comparison drivers. Localhost regtest only.

Environment: MONERO_REGTEST_RPC (eval "$(scripts/regtest-node.sh start)"),
MONERO_WALLET_RPC, MONEROD (run_short.py only), WALLET2_FEES_DIR (work
directory) and WALLET2_FEE_PROBE (default: the newest build of
`cd rust && cargo test -p monero-rust --test wallet2_fee_probe --no-run`).
"""

import json
import os
import socket
import subprocess
import tempfile
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
RUN = os.environ.get("WALLET2_FEES_DIR") or os.path.join(tempfile.gettempdir(), "monero-rust-wallet2-fees")
WALLET_RPC = os.environ.get("MONERO_WALLET_RPC", "monero-wallet-rpc")
MONEROD = os.environ.get("MONEROD", "monerod")

SEED_A = (
    "velvet lymph giddy number token physics poetry unquoted nibs useful sabotage limits "
    "benches lifestyle eden nitrogen anvil fewest avoid batch vials washing fences goat unquoted"
)
SEED_B = (
    "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative "
    "pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage "
    "daytime daytime"
)

MIN_HEIGHT = 300


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def post(url, body, timeout=600):
    data = body if isinstance(body, bytes) else json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as res:
        return res.read()


def json_rpc(url, method, params=None, timeout=600):
    body = {"jsonrpc": "2.0", "id": "0", "method": method}
    if params is not None:
        body["params"] = params
    return json.loads(post(url + "/json_rpc", body, timeout))


def daemon_url():
    url = os.environ.get("MONERO_REGTEST_RPC")
    if not url:
        raise SystemExit("MONERO_REGTEST_RPC is unset; start a node with scripts/regtest-node.sh")
    return url.rstrip("/")


def ctl(fake_url, label, overrides=None):
    post(fake_url + "/__ctl", {"label": label, "overrides": overrides or {}})


def start_fake_node(log_path, ports=None, upstream=None):
    ports = ports or [free_port(), free_port()]
    args = ["python3", f"{HERE}/fake_node.py", "--upstream", upstream or daemon_url(), "--log", log_path]
    for port in ports:
        args += ["--port", str(port)]
    proc = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    line = proc.stdout.readline()
    if "listening" not in line:
        raise RuntimeError("fake node did not start: " + line)
    return proc, ports


def start_wallet_rpc(daemon_address, wallet_dir, log_name, log_level=1):
    os.makedirs(wallet_dir, exist_ok=True)
    port = free_port()
    log = open(f"{RUN}/{log_name}.stdout", "w")
    proc = subprocess.Popen(
        [
            WALLET_RPC,
            # Keep the ring database out of ~/.shared-ringdb.
            "--shared-ringdb-dir", f"{RUN}/ringdb",
            "--allow-mismatched-daemon-version",
            "--daemon-ssl", "disabled",
            "--disable-rpc-login",
            "--rpc-bind-ip", "127.0.0.1",
            "--rpc-bind-port", str(port),
            "--daemon-address", daemon_address,
            "--wallet-dir", wallet_dir,
            "--log-file", f"{RUN}/{log_name}.log",
            "--log-level", str(log_level),
            "--non-interactive",
        ],
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    url = f"http://127.0.0.1:{port}"
    for _ in range(120):
        try:
            json_rpc(url, "get_version", timeout=2)
            return proc, url
        except Exception:
            if proc.poll() is not None:
                raise RuntimeError("monero-wallet-rpc exited; see " + log.name)
            time.sleep(0.5)
    raise RuntimeError("monero-wallet-rpc did not answer")


def open_or_restore(wallet_url, name, seed):
    res = json_rpc(wallet_url, "open_wallet", {"filename": name, "password": ""})
    if "error" in res:
        res = json_rpc(
            wallet_url,
            "restore_deterministic_wallet",
            {"filename": name, "password": "", "seed": seed, "restore_height": 0, "autosave_current": False},
        )
        if "error" in res:
            raise RuntimeError(res)
    json_rpc(wallet_url, "auto_refresh", {"enable": False})
    return json_rpc(wallet_url, "get_address", {"account_index": 0})["result"]["address"]


def addresses():
    os.makedirs(RUN, exist_ok=True)
    path = f"{RUN}/addresses.json"
    if not os.path.exists(path):
        proc, url = start_wallet_rpc(daemon_url().split("//", 1)[1], f"{RUN}/wallets", "setup-wallet-rpc")
        try:
            found = {}
            for name, seed in (("A", SEED_A), ("B", SEED_B)):
                found[name] = open_or_restore(url, name, seed)
                json_rpc(url, "close_wallet")
        finally:
            proc.terminate()
            proc.wait()
        with open(path, "w") as fh:
            json.dump(found, fh)
    found = json.load(open(path))
    height = json_rpc(daemon_url(), "get_info")["result"]["height"]
    if height < MIN_HEIGHT:
        mine(found["A"], MIN_HEIGHT - height)
    return found


def mine(address, count):
    daemon = daemon_url()
    while count > 0:
        n = min(count, 500)
        res = json_rpc(daemon, "generateblocks", {"amount_of_blocks": n, "wallet_address": address})
        if "result" not in res:
            raise RuntimeError(res)
        count -= n
    return json_rpc(daemon, "get_info")["result"]["height"]


def probe_binary():
    explicit = os.environ.get("WALLET2_FEE_PROBE")
    if explicit:
        return explicit
    target = os.environ.get("CARGO_TARGET_DIR") or f"{REPO}/rust/target"
    deps = f"{target}/debug/deps"
    found = [
        f for f in os.listdir(deps)
        if f.startswith("wallet2_fee_probe-") and "." not in f
    ]
    if not found:
        raise SystemExit("build the probe: cargo test -p monero-rust --test wallet2_fee_probe --no-run")
    found.sort(key=lambda f: os.path.getmtime(f"{deps}/{f}"))
    return f"{deps}/{found[-1]}"


def probe(test, env, timeout=900):
    full = dict(os.environ)
    full.update({k: str(v) for k, v in env.items() if v is not None})
    out = subprocess.run(
        [probe_binary(), "--ignored", "--nocapture", "--exact", test],
        env=full, capture_output=True, text=True, timeout=timeout,
    )
    for line in out.stdout.splitlines():
        idx = line.find("PROBE_RESULT ")
        if idx >= 0:
            return json.loads(line[idx + len("PROBE_RESULT "):])
    return {"ok": False, "error": "probe produced no result", "stdout": out.stdout[-2000:], "stderr": out.stderr[-2000:]}
