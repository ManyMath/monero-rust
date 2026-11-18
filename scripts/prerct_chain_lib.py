"""Process and RPC plumbing for scripts/prerct_chain.py."""

import json
import os
import signal
import socket
import subprocess
import time
import urllib.request

XMR = 10**12

FORKS = [
    (1, 0), (2, 150), (3, 250), (4, 350), (5, 470), (6, 520), (7, 570), (8, 620),
    (9, 670), (10, 720), (11, 770), (12, 820), (13, 870), (14, 920), (15, 970), (16, 1020),
]

RUN = os.environ.get("PRERCT_RUN_DIR") or os.path.join(os.environ.get("TMPDIR", "/tmp"), "monero-rust-prerct")
MONEROD = os.environ.get("PRERCT_MONEROD", "")
WALLET_RPC = os.environ.get("MONERO_WALLET_RPC", "monero-wallet-rpc")
STATE = os.path.join(RUN, "state.json")


def fork_height(version):
    return dict(FORKS)[version]


def fork_at(height):
    current = 1
    for version, start in FORKS:
        if height >= start:
            current = version
    return current


def load_state():
    if os.path.exists(STATE):
        with open(STATE) as handle:
            return json.load(handle)
    return {"wallets": {}, "events": []}


def save_state(state):
    os.makedirs(RUN, exist_ok=True)
    tmp = STATE + ".tmp"
    with open(tmp, "w") as handle:
        json.dump(state, handle, indent=1)
    os.replace(tmp, STATE)


def free_port():
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port


class RpcError(Exception):
    pass


def post(url, body, timeout=900):
    request = urllib.request.Request(
        url, json.dumps(body).encode(), {"Content-Type": "application/json"}
    )
    return json.loads(urllib.request.urlopen(request, timeout=timeout).read())


def rpc(url, method, params=None):
    reply = post(url + "/json_rpc", {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}})
    if "error" in reply:
        raise RpcError(f"{method}: {reply['error']}")
    return reply["result"]


def pid_alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def wait_http(url, path, seconds=90):
    for _ in range(seconds * 4):
        try:
            urllib.request.urlopen(url + path, timeout=2).read()
            return
        except Exception:
            time.sleep(0.25)
    raise RuntimeError(f"nothing answered at {url}{path}")


def start_daemon(state, v1_coinbase=False, disable_rpc_ban=False):
    if not MONEROD:
        raise SystemExit("PRERCT_MONEROD is unset; build one with scripts/prerct/build_monerod.sh")
    daemon = state.setdefault("daemon", {})
    if daemon.get("pid") and pid_alive(daemon["pid"]):
        raise RuntimeError("daemon already running")
    for key in ("rpc_port", "p2p_port", "zmq_port"):
        daemon.setdefault(key, free_port())
    data = os.path.join(RUN, "regtest", "data")
    os.makedirs(data, exist_ok=True)
    env = dict(os.environ)
    env["MONERO_REGTEST_HARD_FORKS"] = ",".join(f"{v}:{h}" for v, h in FORKS)
    env.pop("MONERO_REGTEST_V1_COINBASE", None)
    if v1_coinbase:
        env["MONERO_REGTEST_V1_COINBASE"] = "1"
    log = open(os.path.join(RUN, "regtest", "monerod.out"), "ab")
    process = subprocess.Popen(
        ([MONEROD, "--disable-rpc-ban"] if disable_rpc_ban else [MONEROD]) + [
            "--regtest", "--keep-fakechain", "--fixed-difficulty", "1",
            "--rpc-bind-ip", "127.0.0.1", "--rpc-bind-port", str(daemon["rpc_port"]),
            "--p2p-bind-ip", "127.0.0.1", "--p2p-bind-port", str(daemon["p2p_port"]),
            "--zmq-rpc-bind-port", str(daemon["zmq_port"]),
            "--data-dir", data, "--non-interactive", "--offline", "--disable-dns-checkpoints",
            "--check-updates", "disabled", "--rpc-ssl", "disabled", "--no-igd", "--hide-my-port",
            "--log-level", "1", "--max-log-file-size", "0",
        ],
        env=env, stdout=log, stderr=log, start_new_session=True,
    )
    daemon["pid"] = process.pid
    daemon["url"] = f"http://127.0.0.1:{daemon['rpc_port']}"
    daemon["v1_coinbase"] = v1_coinbase
    daemon["disable_rpc_ban"] = disable_rpc_ban
    save_state(state)
    wait_http(daemon["url"], "/get_height")
    return daemon["url"]


def stop_pid(pid, seconds=60):
    if not pid or not pid_alive(pid):
        return
    os.kill(pid, signal.SIGTERM)
    for _ in range(seconds * 4):
        if not pid_alive(pid):
            return
        try:
            os.waitpid(pid, os.WNOHANG)
        except OSError:
            pass
        time.sleep(0.25)
    raise RuntimeError(f"pid {pid} did not exit")


def stop_daemon(state):
    stop_pid(state.get("daemon", {}).get("pid"))
    state.get("daemon", {}).pop("pid", None)
    save_state(state)


def start_wallet_rpc(state, name):
    wallet = state["wallets"].setdefault(name, {})
    if wallet.get("pid") and pid_alive(wallet["pid"]):
        return wallet["url"]
    wallet_dir = os.path.join(RUN, "wallets", name)
    os.makedirs(wallet_dir, exist_ok=True)
    os.makedirs(os.path.join(RUN, "ringdb"), exist_ok=True)
    port = free_port()
    log = open(os.path.join(RUN, f"wallet-rpc-{name}.out"), "ab")
    process = subprocess.Popen(
        [
            WALLET_RPC, "--daemon-address", state["daemon"]["url"].replace("http://", ""),
            "--trusted-daemon", "--disable-rpc-login", "--rpc-bind-ip", "127.0.0.1",
            "--rpc-bind-port", str(port), "--wallet-dir", wallet_dir,
            "--shared-ringdb-dir", os.path.join(RUN, "ringdb"),
            "--log-file", os.path.join(RUN, f"wallet-rpc-{name}.log"), "--log-level", "1",
            "--max-log-file-size", "0",
            "--allow-mismatched-daemon-version",
        ],
        stdout=log, stderr=log, start_new_session=True,
    )
    wallet["pid"] = process.pid
    wallet["url"] = f"http://127.0.0.1:{port}"
    save_state(state)
    wait_http(wallet["url"], "/json_rpc")
    return wallet["url"]


def stop_wallet_rpc(state, name):
    wallet = state["wallets"].get(name, {})
    if wallet.get("pid") and pid_alive(wallet["pid"]):
        try:
            rpc(wallet["url"], "close_wallet")
        except Exception:
            pass
    stop_pid(wallet.get("pid"))
    wallet.pop("pid", None)
    save_state(state)


def stop_all(state):
    for name in list(state["wallets"]):
        stop_wallet_rpc(state, name)
    stop_daemon(state)


class Wallet:
    def __init__(self, state, name):
        self.state = state
        self.name = name
        self.url = start_wallet_rpc(state, name)

    def call(self, method, params=None):
        return rpc(self.url, method, params)

    def open_or_create(self):
        record = self.state["wallets"][self.name]
        created = False
        try:
            self.call("open_wallet", {"filename": self.name, "password": ""})
        except RpcError:
            self.call("create_wallet", {"filename": self.name, "password": "", "language": "English"})
            created = True
        record["seed"] = self.call("query_key", {"key_type": "mnemonic"})["key"]
        record["spend_key"] = self.call("query_key", {"key_type": "spend_key"})["key"]
        record["address"] = self.address()
        save_state(self.state)
        return created

    def address(self, account=0, index=0):
        return self.call("get_address", {"account_index": account, "address_index": [index]})["addresses"][0]["address"]

    def refresh(self):
        return self.call("refresh", {"start_height": 0})

    def height(self):
        return self.call("get_height")["height"]


class Daemon:
    def __init__(self, state):
        self.url = state["daemon"]["url"]

    def call(self, method, params=None):
        return rpc(self.url, method, params)

    def other(self, path, params=None):
        return post(self.url + path, params or {})

    def height(self):
        return self.other("/get_height")["height"]

    def mine(self, count, address):
        done = 0
        while done < count:
            step = min(25, count - done)
            self.call("generateblocks", {"amount_of_blocks": step, "wallet_address": address})
            done += step
        return self.height()

    def tx(self, txid):
        reply = self.other("/get_transactions", {"txs_hashes": [txid], "decode_as_json": True})
        entry = reply["txs"][0]
        entry["json"] = json.loads(entry["as_json"])
        return entry

    def block(self, height):
        result = self.call("get_block", {"height": height})
        result["json_parsed"] = json.loads(result["json"])
        return result

    def block_hash(self, height):
        return self.call("get_block_header_by_height", {"height": height})["block_header"]["hash"]

    def block_version(self, height):
        return self.call("get_block_header_by_height", {"height": height})["block_header"]["major_version"]
