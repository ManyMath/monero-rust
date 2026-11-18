"""Shared pieces of the stagenet harness.

Environment: MONERO_STAGENET_RPC (node URL), MONERO_STAGENET_DIR (run directory),
MONERO_WALLET_RPC, MONERO_STAGENET_TOOL (built stagenet_tool example) and
MONERO_STAGENET_NETWORK (default stagenet).
"""
import http.client
import http.server
import json
import os
import socket
import struct
import subprocess
import threading
import time
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NETWORK = os.environ.get("MONERO_STAGENET_NETWORK", "stagenet")
RUN = os.environ.get("MONERO_STAGENET_DIR") or os.path.join(os.environ.get("TMPDIR", "/tmp"),
                                                             "monero-rust-stagenet")
WALLET_RPC = os.environ.get("MONERO_WALLET_RPC", "monero-wallet-rpc")
TOOL = os.environ.get("MONERO_STAGENET_TOOL") or os.path.join(
    os.environ.get("CARGO_TARGET_DIR") or os.path.join(REPO, "rust", "target"),
    "release", "examples", "stagenet_tool")
OFFLINE_OPS = ("addr", "seed")


def rpc_url():
    url = os.environ.get("MONERO_STAGENET_RPC")
    if not url:
        raise SystemExit("set MONERO_STAGENET_RPC to the daemon RPC URL, for example http://127.0.0.1:38081")
    return url.rstrip("/")


def ensure_dirs():
    for name in ("seeds", "w2", "mr", "req", "logs", "evidence", "ringdb"):
        os.makedirs(os.path.join(RUN, name), exist_ok=True)
    os.chmod(os.path.join(RUN, "seeds"), 0o700)


def daemon_json(method, params=None, rpc=None, timeout=60):
    body = json.dumps({"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}}).encode()
    url = (rpc or rpc_url()) + "/json_rpc"
    with urllib.request.urlopen(urllib.request.Request(url, body), timeout=timeout) as r:
        return json.load(r)["result"]


def daemon_other(path, params=None, rpc=None, timeout=60):
    body = json.dumps(params or {}).encode()
    url = (rpc or rpc_url()) + "/" + path
    with urllib.request.urlopen(urllib.request.Request(url, body), timeout=timeout) as r:
        return json.load(r)


def daemon_height(rpc=None):
    return daemon_other("get_height", rpc=rpc)["height"]


def pool_hashes(rpc=None):
    return daemon_other("get_transaction_pool_hashes", rpc=rpc).get("tx_hashes", [])


def get_tx(txid, rpc=None):
    r = daemon_other("get_transactions", {"txs_hashes": [txid], "decode_as_json": False}, rpc=rpc)
    txs = r.get("txs", [])
    return txs[0] if txs else None


class Reader:
    def __init__(self, data):
        self.d = data
        self.p = 0

    def varint(self):
        value, shift = 0, 0
        while True:
            b = self.d[self.p]
            self.p += 1
            value |= (b & 0x7F) << shift
            if not b & 0x80:
                return value
            shift += 7

    def take(self, n):
        if self.p + n > len(self.d):
            raise ValueError("truncated")
        out = self.d[self.p:self.p + n]
        self.p += n
        return out

    def byte(self):
        return self.take(1)[0]


def parse_extra(extra):
    r = Reader(extra)
    fields = []
    try:
        while r.p < len(extra):
            tag = r.byte()
            if tag == 0x00:
                rest = extra[r.p:]
                fields.append({"tag": "padding", "len": 1 + len(rest)})
                r.p = len(extra)
            elif tag == 0x01:
                fields.append({"tag": "pubkey", "key": r.take(32).hex()})
            elif tag == 0x02:
                n = r.varint()
                nonce = r.take(n)
                f = {"tag": "nonce", "len": n, "hex": nonce.hex()}
                if n == 33 and nonce[0] == 0x00:
                    f["payment_id"] = nonce[1:].hex()
                elif n == 9 and nonce[0] == 0x01:
                    f["encrypted_payment_id"] = nonce[1:].hex()
                fields.append(f)
            elif tag == 0x03:
                n = r.varint()
                fields.append({"tag": "merge_mining", "hex": r.take(n).hex()})
            elif tag == 0x04:
                n = r.varint()
                fields.append({"tag": "additional_pubkeys", "keys": [r.take(32).hex() for _ in range(n)]})
            elif tag == 0xDE:
                n = r.varint()
                fields.append({"tag": "minergate", "hex": r.take(n).hex()})
            else:
                fields.append({"tag": "unknown", "value": tag, "rest": extra[r.p:].hex()})
                r.p = len(extra)
    except (ValueError, IndexError):
        fields.append({"tag": "malformed"})
    return fields


def decode_tx(blob_hex):
    data = bytes.fromhex(blob_hex)
    r = Reader(data)
    tx = {"size": len(data)}
    tx["version"] = r.varint()
    tx["unlock_time"] = r.varint()
    inputs = []
    for _ in range(r.varint()):
        tag = r.byte()
        if tag == 0xFF:
            inputs.append({"type": "gen", "height": r.varint()})
            continue
        if tag != 0x02:
            raise ValueError("unsupported input tag %d" % tag)
        amount = r.varint()
        offsets = [r.varint() for _ in range(r.varint())]
        absolute, total = [], 0
        for o in offsets:
            total += o
            absolute.append(total)
        inputs.append({"type": "key", "amount": amount, "offsets": offsets,
                       "indexes": absolute, "key_image": r.take(32).hex()})
    tx["inputs"] = inputs
    outputs = []
    for _ in range(r.varint()):
        amount = r.varint()
        tag = r.byte()
        if tag == 0x02:
            outputs.append({"amount": amount, "key": r.take(32).hex(), "view_tag": None})
        elif tag == 0x03:
            outputs.append({"amount": amount, "key": r.take(32).hex(), "view_tag": "%02x" % r.byte()})
        else:
            raise ValueError("unsupported output tag %d" % tag)
    tx["outputs"] = outputs
    extra = r.take(r.varint())
    tx["extra_hex"] = extra.hex()
    tx["extra"] = parse_extra(extra)
    tx["prefix_size"] = r.p
    if tx["version"] < 2:
        return tx
    rct_type = r.byte()
    tx["rct_type"] = rct_type
    if rct_type == 0:
        return tx
    tx["fee"] = r.varint()
    tx["ecdh"] = [r.take(8).hex() for _ in outputs]
    tx["out_pk"] = [r.take(32).hex() for _ in outputs]
    tx["rct_base_size"] = r.p - tx["prefix_size"]
    key_inputs = [i for i in inputs if i["type"] == "key"]
    if rct_type in (5, 6):
        plus = rct_type == 6
        nbp = r.varint()
        tx["bp_count"] = nbp
        lr = []
        for _ in range(nbp):
            r.take(32 * (6 if plus else 9) if plus else 32 * 4 + 0)
            if not plus:
                r.take(32 * 2)
            nl = r.varint()
            r.take(32 * nl)
            nr = r.varint()
            r.take(32 * nr)
            if not plus:
                r.take(32 * 3)
            lr.append([nl, nr])
        tx["bp_lr"] = lr
        for i in key_inputs:
            r.take(32 * len(i["offsets"]))
            r.take(64)
        tx["pseudo_outs"] = [r.take(32).hex() for _ in key_inputs]
        tx["trailing_bytes"] = len(data) - r.p
        n_out = len(outputs)
        weight = len(data)
        if n_out > 2:
            padded = 1
            while padded < n_out:
                padded *= 2
            bp_base = (32 * ((6 if plus else 9) + 7 * 2)) // 2
            nlr = 0
            while (1 << nlr) < padded:
                nlr += 1
            nlr += 6
            bp_size = 32 * ((6 if plus else 9) + 2 * nlr)
            weight += (bp_base * padded - bp_size) * 4 // 5
        tx["weight"] = weight
        tx["fee_per_byte"] = tx["fee"] / weight
    return tx


def observer_view(tx):
    extra = tx["extra"]
    pub = [f["key"] for f in extra if f["tag"] == "pubkey"]
    add = [f["keys"] for f in extra if f["tag"] == "additional_pubkeys"]
    nonces = [f for f in extra if f["tag"] == "nonce"]
    return {
        "version": tx["version"],
        "unlock_time": tx["unlock_time"],
        "rct_type": tx.get("rct_type"),
        "n_inputs": len(tx["inputs"]),
        "ring_sizes": [len(i.get("offsets", [])) for i in tx["inputs"]],
        "key_images": [i.get("key_image") for i in tx["inputs"]],
        "key_images_sorted_desc": [i.get("key_image") for i in tx["inputs"]]
        == sorted((i.get("key_image") for i in tx["inputs"]), reverse=True),
        "n_outputs": len(tx["outputs"]),
        "view_tags": [o["view_tag"] is not None for o in tx["outputs"]],
        "extra_layout": [f["tag"] + (":%d" % len(f["keys"]) if f["tag"] == "additional_pubkeys" else "")
                         + (":%d" % f["len"] if f["tag"] == "nonce" else "") for f in extra],
        "extra_len": len(tx["extra_hex"]) // 2,
        "tx_pubkey": pub[0] if pub else None,
        "n_tx_pubkeys": len(pub),
        "additional_pubkeys": len(add[0]) if add else 0,
        "payment_id_fields": [("encrypted" if "encrypted_payment_id" in f else
                               "unencrypted" if "payment_id" in f else "other") for f in nonces],
        "fee": tx.get("fee"),
        "size": tx["size"],
        "weight": tx.get("weight"),
        "fee_per_byte": tx.get("fee_per_byte"),
        "bp_count": tx.get("bp_count"),
        "trailing_bytes": tx.get("trailing_bytes"),
    }


class Epee:
    def __init__(self, data):
        self.d = data
        self.p = 0

    def take(self, n):
        out = self.d[self.p:self.p + n]
        if len(out) != n:
            raise ValueError("truncated epee")
        self.p += n
        return out

    def varint(self):
        first = self.d[self.p]
        size = 1 << (first & 3)
        value = int.from_bytes(self.take(size), "little") >> 2
        return value

    def value(self, t):
        fmt = {1: "<q", 2: "<i", 3: "<h", 4: "<b", 5: "<Q", 6: "<I", 7: "<H", 8: "<B", 9: "<d"}
        if t in fmt:
            size = struct.calcsize(fmt[t])
            return struct.unpack(fmt[t], self.take(size))[0]
        if t == 10:
            return self.take(self.varint())
        if t == 11:
            return self.take(1)[0] != 0
        if t == 12:
            return self.section()
        raise ValueError("epee type %d" % t)

    def section(self):
        out = {}
        for _ in range(self.varint()):
            name = self.take(self.take(1)[0]).decode()
            t = self.take(1)[0]
            if t & 0x80:
                t &= 0x7F
                out[name] = [self.value(t) for _ in range(self.varint())]
            else:
                out[name] = self.value(t)
        return out


def epee_decode(data):
    if data[:8] != bytes.fromhex("0111010101010201"):
        raise ValueError("not epee")
    e = Epee(data)
    e.p = 9
    return e.section()


def summarize_bin_request(path, body):
    try:
        req = epee_decode(body)
    except Exception as exc:  # noqa: BLE001
        return {"undecoded": repr(exc), "bytes": len(body)}
    out = {}
    for key, value in req.items():
        if key == "block_ids" and isinstance(value, bytes):
            ids = [value[i:i + 32].hex() for i in range(0, len(value), 32)]
            out["block_ids_count"] = len(ids)
            out["block_ids_first"] = ids[:2]
            out["block_ids_last"] = ids[-1:] if ids else []
        elif key == "outputs" and isinstance(value, list):
            out["outputs_count"] = len(value)
            out["outputs_amounts"] = sorted({o.get("amount") for o in value})
            out["outputs_indexes"] = [o.get("index") for o in value]
        elif key == "txid" and isinstance(value, bytes):
            out["txid"] = value.hex()
        elif isinstance(value, bytes):
            out[key] = value.hex() if len(value) <= 64 else "<%d bytes>" % len(value)
        elif isinstance(value, list) and len(value) > 64:
            out[key] = "<%d items>" % len(value)
        else:
            out[key] = value
    return out


def summarize_json_request(body):
    try:
        req = json.loads(body) if body else {}
    except Exception:  # noqa: BLE001
        return {"undecoded": True, "bytes": len(body)}

    def trim(v):
        if isinstance(v, str) and len(v) > 200:
            return "<%d chars>" % len(v)
        if isinstance(v, list):
            if len(v) > 40:
                return {"count": len(v), "first": [trim(x) for x in v[:3]]}
            return [trim(x) for x in v]
        if isinstance(v, dict):
            return {k: trim(x) for k, x in v.items()}
        return v
    return trim(req)


class LoggingProxy:
    def __init__(self, upstream, log_path, label):
        host, port = upstream.replace("http://", "").split(":")
        self.upstream = (host, int(port))
        self.log_path = log_path
        self.label = label
        self.lock = threading.Lock()
        self.seq = 0
        self.phase = "start"
        proxy = self

        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *args):
                pass

            def _serve(self):
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length) if length else b""
                if self.path.startswith("/__phase/"):
                    proxy.phase = self.path[len("/__phase/"):]
                    self.send_response(200)
                    self.send_header("Content-Length", "2")
                    self.end_headers()
                    self.wfile.write(b"ok")
                    return
                started = time.time()
                conn = getattr(self, "_up", None)
                headers = {k: v for k, v in self.headers.items()
                           if k.lower() not in ("host", "connection", "proxy-connection", "accept-encoding")}
                headers["Host"] = "%s:%d" % proxy.upstream
                status, resp, resp_headers = 502, b"", []
                for attempt in range(2):
                    try:
                        if conn is None:
                            conn = http.client.HTTPConnection(*proxy.upstream, timeout=600)
                        conn.request(self.command, self.path, body=body or None, headers=headers)
                        r = conn.getresponse()
                        resp = r.read()
                        status = r.status
                        resp_headers = r.getheaders()
                        break
                    except Exception:  # noqa: BLE001
                        try:
                            conn.close()
                        except Exception:  # noqa: BLE001
                            pass
                        conn = None
                self._up = conn
                self.send_response(status)
                for k, v in resp_headers:
                    if k.lower() in ("content-length", "transfer-encoding", "connection"):
                        continue
                    self.send_header(k, v)
                self.send_header("Content-Length", str(len(resp)))
                self.end_headers()
                self.wfile.write(resp)
                proxy.record(self, body, status, resp, time.time() - started)

            do_POST = _serve
            do_GET = _serve

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.port = self.server.server_address[1]
        self.url = "http://127.0.0.1:%d" % self.port
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def record(self, handler, body, status, resp, seconds):
        path = handler.path.lstrip("/")
        entry = {"label": self.label, "phase": self.phase, "http": handler.command, "path": path,
                 "status": status, "req_bytes": len(body), "resp_bytes": len(resp),
                 "ms": round(seconds * 1000, 1),
                 "user_agent": handler.headers.get("User-Agent"),
                 "headers": sorted(k.lower() for k in handler.headers.keys())}
        if path.endswith(".bin"):
            entry["params"] = summarize_bin_request(path, body)
            if path == "getblocks.bin":
                try:
                    r = epee_decode(resp)
                    status_text = r.get("status")
                    if isinstance(status_text, bytes):
                        status_text = status_text.decode(errors="replace")
                    blocks = r.get("blocks", [])
                    entry["resp"] = {"start_height": r.get("start_height"),
                                     "current_height": r.get("current_height"),
                                     "blocks": len(blocks), "status": status_text,
                                     "pruned_flags": sorted({bool(b.get("pruned")) for b in blocks
                                                             if isinstance(b, dict)}),
                                     "pool_info_extent": r.get("pool_info_extent"),
                                     "added_pool_txs": len(r.get("added_pool_txs", []) or []),
                                     "fields": sorted(r.keys())}
                except Exception as exc:  # noqa: BLE001
                    entry["resp"] = {"undecoded": repr(exc)}
        else:
            req = summarize_json_request(body)
            if path == "json_rpc" and isinstance(req, dict):
                entry["method"] = req.get("method")
                entry["params"] = req.get("params")
            else:
                entry["params"] = req
            if path in ("send_raw_transaction", "sendrawtransaction") or entry.get("method") in ("get_fee_estimate",):
                try:
                    entry["resp"] = json.loads(resp)
                except Exception:  # noqa: BLE001
                    pass
        with self.lock:
            self.seq += 1
            entry["seq"] = self.seq
            entry["t"] = round(time.time(), 3)
            try:
                line = json.dumps(entry)
            except TypeError:
                line = json.dumps({k: (v if k != "params" and k != "resp" else repr(v)) for k, v in entry.items()})
            with open(self.log_path, "a") as f:
                f.write(line + "\n")

    def stop(self):
        self.server.shutdown()
        self.server.server_close()


def read_log(path, phase=None):
    out = []
    if not os.path.exists(path):
        return out
    with open(path) as f:
        for line in f:
            e = json.loads(line)
            if phase is None or e["phase"] == phase:
                out.append(e)
    return out


def call_name(entry):
    return entry.get("method") or entry["path"]


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


class WalletRpc:
    def __init__(self, wallet_dir, ringdb_dir, daemon=None, log_name="wallet-rpc", log_level=0):
        os.makedirs(wallet_dir, exist_ok=True)
        os.makedirs(ringdb_dir, exist_ok=True)
        assert os.path.abspath(ringdb_dir).startswith(os.path.abspath(RUN)), "ringdb must be in the run dir"
        self.port = free_port()
        daemon = daemon or rpc_url().replace("http://", "")
        self.stdout = open(os.path.join(wallet_dir, log_name + ".stdout"), "ab")
        self.proc = subprocess.Popen(
            [WALLET_RPC, "--" + NETWORK, "--daemon-address", daemon, "--trusted-daemon",
             "--shared-ringdb-dir", ringdb_dir,
             "--disable-rpc-login", "--rpc-bind-ip", "127.0.0.1", "--rpc-bind-port", str(self.port),
             "--wallet-dir", wallet_dir, "--log-file", os.path.join(wallet_dir, log_name + ".log"),
             "--log-level", str(log_level), "--non-interactive"],
            stdout=self.stdout, stderr=self.stdout, stdin=subprocess.DEVNULL)
        self.url = "http://127.0.0.1:%d/json_rpc" % self.port
        for _ in range(240):
            try:
                self.call("get_version", timeout=5)
                break
            except Exception:  # noqa: BLE001
                if self.proc.poll() is not None:
                    raise RuntimeError("wallet-rpc exited early")
                time.sleep(0.25)
        else:
            raise RuntimeError("wallet-rpc did not come up")

    def raw(self, method, params=None, timeout=1800):
        body = json.dumps({"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}}).encode()
        with urllib.request.urlopen(urllib.request.Request(self.url, body), timeout=timeout) as r:
            return json.load(r)

    def call(self, method, params=None, timeout=1800):
        r = self.raw(method, params, timeout)
        if "error" in r:
            raise RuntimeError("%s: %s" % (method, r["error"]))
        return r["result"]

    def stop(self, store=True):
        try:
            if store:
                self.call("store", timeout=300)
        except Exception:  # noqa: BLE001
            pass
        try:
            self.raw("stop_wallet", timeout=120)
        except Exception:  # noqa: BLE001
            pass
        try:
            self.proc.wait(timeout=60)
        except Exception:  # noqa: BLE001
            self.proc.terminate()
            try:
                self.proc.wait(timeout=30)
            except Exception:  # noqa: BLE001
                self.proc.kill()
        self.stdout.close()

    def kill(self):
        self.proc.terminate()
        try:
            self.proc.wait(timeout=30)
        except Exception:  # noqa: BLE001
            self.proc.kill()
        self.stdout.close()


def tool(request, name, rpc=None):
    req_dir = os.path.join(RUN, "req")
    out_dir = os.path.join(RUN, "mr")
    os.makedirs(req_dir, exist_ok=True)
    os.makedirs(out_dir, exist_ok=True)
    request = dict(request)
    if request.get("op") not in OFFLINE_OPS:
        request.setdefault("rpc", rpc or rpc_url())
    request.setdefault("network", NETWORK)
    req_path = os.path.join(req_dir, name + ".json")
    out_path = os.path.join(out_dir, name + ".json")
    with open(req_path, "w") as f:
        json.dump(request, f, indent=1)
    if os.path.exists(out_path):
        os.remove(out_path)
    os.makedirs(os.path.join(RUN, "logs"), exist_ok=True)
    if not os.path.exists(TOOL):
        raise SystemExit("stagenet_tool is not built at %s; run "
                         "`cargo build --release -p monero-rust --example stagenet_tool` in rust/ "
                         "or set MONERO_STAGENET_TOOL" % TOOL)
    log = open(os.path.join(RUN, "logs", name + ".log"), "ab")
    code = subprocess.call([TOOL, req_path, out_path], stdout=log, stderr=log)
    log.close()
    if code != 0 or not os.path.exists(out_path):
        raise RuntimeError("stagenet_tool %s failed (exit %d), see logs/%s.log" % (name, code, name))
    with open(out_path) as f:
        return json.load(f)


def seed_file(label):
    return os.path.join(RUN, "seeds", label + ".txt")


def state_file(label, tag=""):
    return os.path.join(RUN, "mr", (tag + "-" if tag else "") + label + "-state.json")


def proxy_url(label):
    with open(os.path.join(RUN, "proxy-%s.port" % label)) as f:
        return "http://127.0.0.1:%s" % f.read().strip()


def set_phase(label, phase):
    urllib.request.urlopen(proxy_url(label) + "/__phase/" + phase, timeout=10).read()


def proxy_log(label):
    return os.path.join(RUN, "evidence", "proxy-%s.jsonl" % label)
