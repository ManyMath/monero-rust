"""Minimal JSON-RPC helpers for monerod and monero-wallet-rpc (localhost only)."""
import json
import os
import socket
import subprocess
import time
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RUN = os.environ.get("STAT_PARITY_DIR") or os.path.join(os.environ.get("TMPDIR", "/tmp"), "monero-rust-stat-parity")
SAMPLES = os.environ.get("STAT_PARITY_SAMPLES") or RUN + "/samples"
PICKS = os.environ.get("STAT_PARITY_PICKS") or RUN + "/picks"
RESULTS = os.environ.get("STAT_PARITY_RESULTS") or RUN + "/results"
MONEROD = os.environ.get("MONEROD", "monerod")
WALLET_RPC = os.environ.get("MONERO_WALLET_RPC", "monero-wallet-rpc")
W2HARNESS = os.environ.get("W2HARNESS_DIR") or RUN + "/w2build"
TARGET = os.environ.get("CARGO_TARGET_DIR") or os.path.join(REPO, "rust", "target")
ATOMIC = 10**12


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def post(url, payload, timeout=600):
    req = urllib.request.Request(
        url, data=json.dumps(payload).encode(), headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


class Rpc:
    def __init__(self, url):
        self.url = url.rstrip("/")

    def call(self, method, params=None, timeout=600):
        out = post(
            self.url + "/json_rpc",
            {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}},
            timeout,
        )
        if "error" in out:
            raise RuntimeError(f"{method}: {out['error']}")
        return out["result"]

    def other(self, path, params=None):
        return post(self.url + "/" + path, params or {})


def start_wallet_rpc(wallet_dir, daemon, log, extra=()):
    port = free_port()
    os.makedirs(os.path.dirname(log), exist_ok=True)
    proc = subprocess.Popen(
        [
            WALLET_RPC,
            # Regtest wallets run as mainnet wallets; --no-dns stops MoneroPulse lookups.
            "--no-dns",
            "--allow-mismatched-daemon-version",
            "--disable-rpc-login",
            "--rpc-bind-ip", "127.0.0.1",
            "--rpc-bind-port", str(port),
            "--daemon-address", daemon.replace("http://", ""),
            "--trusted-daemon",
            "--wallet-dir", wallet_dir,
            "--log-file", log,
            "--log-level", "0",
            *extra,
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    rpc = Rpc(f"http://127.0.0.1:{port}")
    for _ in range(120):
        try:
            rpc.call("get_version")
            return proc, rpc
        except Exception:
            if proc.poll() is not None:
                raise RuntimeError("monero-wallet-rpc exited; see " + log)
            time.sleep(0.25)
    proc.terminate()
    raise RuntimeError("monero-wallet-rpc did not come up")


def start_monerod(state_dir):
    """Start a regtest monerod on <state_dir>/data; --keep-fakechain lets a snapshot be reopened."""
    os.makedirs(state_dir + "/data", exist_ok=True)
    rpc, p2p, zmq = free_port(), free_port(), free_port()
    log = open(state_dir + "/monerod.log", "ab")
    proc = subprocess.Popen(
        [
            MONEROD, "--regtest", "--keep-fakechain", "--fixed-difficulty", "1",
            "--rpc-bind-ip", "127.0.0.1", "--rpc-bind-port", str(rpc),
            "--p2p-bind-port", str(p2p), "--zmq-rpc-bind-port", str(zmq),
            "--data-dir", state_dir + "/data",
            "--non-interactive", "--offline", "--disable-dns-checkpoints",
            "--check-updates", "disabled", "--rpc-ssl", "disabled",
            "--no-igd", "--hide-my-port", "--log-level", "0",
            "--max-log-file-size", "10000000", "--max-log-files", "1",
        ],
        stdout=log, stderr=log,
    )
    with open(state_dir + "/monerod.pid", "w") as f:
        f.write(str(proc.pid))
    with open(state_dir + "/rpc-port", "w") as f:
        f.write(str(rpc))
    url = f"http://127.0.0.1:{rpc}"
    for _ in range(240):
        try:
            Rpc(url).other("get_info")
            return url
        except Exception:
            if proc.poll() is not None:
                raise RuntimeError("monerod exited; see " + state_dir + "/monerod.log")
            time.sleep(0.25)
    raise RuntimeError("monerod did not come up")


def stop_monerod(state_dir):
    import signal

    try:
        with open(state_dir + "/monerod.pid") as f:
            pid = int(f.read().strip())
    except FileNotFoundError:
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    for _ in range(240):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            break
        try:
            os.waitpid(pid, os.WNOHANG)
        except ChildProcessError:
            pass
        time.sleep(0.25)
    os.remove(state_dir + "/monerod.pid")
