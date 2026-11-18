#!/usr/bin/env python3
"""Scriptable logging proxy in front of a regtest monerod, for comparing fee selection.

Control: POST /__ctl {"label": ..., "overrides": {...}}; GET /__ctl returns the state.
"""

import argparse
import http.client
import json
import struct
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

STATE = {"label": "", "overrides": {}}
LOCK = threading.Lock()
SEQ = [0]
LOG = None
UPSTREAM = None

# epee's JSON writer escapes only these bytes (contrib/epee/src/parserse_base_utils.cpp:42).
EPEE_ESCAPES = {
    0x08: b"\\b",
    0x0C: b"\\f",
    0x0A: b"\\n",
    0x0D: b"\\r",
    0x09: b"\\t",
    0x0B: b"\\v",
    0x22: b'\\"',
    0x5C: b"\\\\",
    0x2F: b"\\/",
}


def epee_escape(blob: bytes) -> bytes:
    out = bytearray()
    for byte in blob:
        out += EPEE_ESCAPES.get(byte, bytes([byte]))
    return bytes(out)


def backlog_blob(entries) -> bytes:
    blob = b""
    for entry in entries:
        weight, fee = entry[0], entry[1]
        time_in_pool = entry[2] if len(entry) > 2 else 0
        blob += struct.pack("<QQQ", weight, fee, time_in_pool)
    return blob


def upstream_call(method, path, body, headers):
    parsed = urlparse(UPSTREAM)
    conn = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=600)
    try:
        send_headers = {"Content-Type": headers.get("Content-Type", "application/json")}
        conn.request(method, path, body=body, headers=send_headers)
        res = conn.getresponse()
        data = res.read()
        return res.status, res.getheader("Content-Type", "application/json"), data
    finally:
        conn.close()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def respond(self, status, body, content_type="application/json"):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def record(self, entry):
        with LOCK:
            SEQ[0] += 1
            entry["seq"] = SEQ[0]
            entry["label"] = STATE["label"]
            LOG.write(json.dumps(entry) + "\n")
            LOG.flush()

    def do_GET(self):
        self.handle_any("GET")

    def do_POST(self):
        self.handle_any("POST")

    def handle_any(self, http_method):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        path = self.path

        if path == "/__ctl":
            if http_method == "POST":
                new = json.loads(body or b"{}")
                with LOCK:
                    STATE["label"] = new.get("label", "")
                    STATE["overrides"] = new.get("overrides", {})
            with LOCK:
                self.respond(200, json.dumps(STATE).encode())
            return

        request = None
        rpc_method = None
        if path == "/json_rpc":
            try:
                request = json.loads(body)
                rpc_method = request.get("method")
            except Exception:
                request = None
        key = rpc_method if rpc_method else path
        with LOCK:
            override = dict(STATE["overrides"].get(key, {}))

        entry = {
            "t": round(time.time(), 3),
            "port": self.server.server_address[1],
            "http": http_method,
            "path": path,
            "method": rpc_method,
            "params": request.get("params") if isinstance(request, dict) else None,
            "envelope_keys": sorted(request.keys()) if isinstance(request, dict) else None,
            "body_len": len(body),
            "headers": {k: v for k, v in self.headers.items()},
            "override": sorted(override.keys()),
        }
        if not rpc_method and len(body) <= 400:
            try:
                entry["body"] = body.decode()
            except UnicodeDecodeError:
                entry["body_hex"] = body.hex()
        self.record(entry)

        if override.get("delay"):
            time.sleep(float(override["delay"]))

        fail = override.get("fail")
        if fail == "http500":
            self.respond(500, b"")
            return
        if fail == "drop":
            self.close_connection = True
            try:
                self.connection.shutdown(2)
            except OSError:
                pass
            return
        if fail == "garbage":
            self.respond(200, b"<html>not json</html>")
            return
        if fail == "empty":
            self.respond(200, b"")
            return
        req_id = request.get("id", "0") if isinstance(request, dict) else "0"
        if fail == "jsonrpc_error":
            self.respond(
                200,
                json.dumps(
                    {
                        "id": req_id,
                        "jsonrpc": "2.0",
                        "error": {"code": -32601, "message": "Method not found"},
                    }
                ).encode(),
            )
            return

        if not override:
            status, ctype, data = upstream_call(http_method, path, body, self.headers)
            self.respond(status, data, ctype)
            return

        if "result" in override:
            result = dict(override["result"])
        elif rpc_method == "get_txpool_backlog" and "backlog" in override:
            result = {"credits": 0, "status": "OK", "top_hash": "", "untrusted": False}
        else:
            status, ctype, data = upstream_call(http_method, path, body, self.headers)
            try:
                parsed = json.loads(data)
            except Exception:
                self.respond(status, data, ctype)
                return
            result = parsed.get("result", parsed) if rpc_method else parsed
            if rpc_method and "result" not in parsed:
                result = {"credits": 0, "status": "OK", "top_hash": "", "untrusted": False}

        if rpc_method == "getblockheadersrange" and (
            "block_weights" in override or "count" in override or "block_weight_split" in override
        ):
            params = request.get("params") or {}
            start = int(params.get("start_height", 0))
            end = int(params.get("end_height", 0))
            wanted = int(override.get("count", max(0, end - start + 1)))
            weights = override.get("block_weights")
            headers = list(result.get("headers", []))
            template = headers[0] if headers else {"major_version": 16, "minor_version": 16}
            while len(headers) < wanted:
                clone = dict(template)
                clone["height"] = start + len(headers)
                headers.append(clone)
            headers = headers[:wanted]
            if weights is not None:
                for i, header in enumerate(headers):
                    weight = weights[i % len(weights)]
                    header["block_weight"] = weight
                    header["block_size"] = weight
            split = override.get("block_weight_split")
            if split is not None:
                for i, header in enumerate(headers):
                    height = start + i
                    weight = split["below"] if height < split["height"] else split["at_or_above"]
                    header["height"] = height
                    header["block_weight"] = weight
                    header["block_size"] = weight
            result["headers"] = headers

        if "status" in override:
            result["status"] = override["status"]
        for k, v in (override.get("patch") or {}).items():
            result[k] = v
        for k in override.get("remove") or []:
            result.pop(k, None)

        if rpc_method == "get_txpool_backlog" and "backlog" in override:
            entries = override["backlog"]
            if override.get("backlog_format", "blob") == "objects":
                result["backlog"] = [
                    {"weight": e[0], "fee": e[1], "time_in_pool": e[2] if len(e) > 2 else 0}
                    for e in entries
                ]
            elif entries:
                marker = "@@BACKLOG_BLOB@@"
                result["backlog"] = marker
                text = json.dumps({"id": req_id, "jsonrpc": "2.0", "result": result}).encode()
                text = text.replace(marker.encode(), epee_escape(backlog_blob(entries)))
                self.respond(200, text)
                return

        if rpc_method:
            out = {"id": req_id, "jsonrpc": "2.0", "result": result}
        else:
            out = result
        self.respond(200, json.dumps(out).encode())


def main():
    global LOG, UPSTREAM
    ap = argparse.ArgumentParser()
    ap.add_argument("--upstream", required=True)
    ap.add_argument("--port", type=int, action="append", required=True)
    ap.add_argument("--log", required=True)
    args = ap.parse_args()
    UPSTREAM = args.upstream.rstrip("/")
    LOG = open(args.log, "a")

    servers = []
    for port in args.port:
        server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
        server.daemon_threads = True
        servers.append(server)
    for server in servers[1:]:
        threading.Thread(target=server.serve_forever, daemon=True).start()
    print("listening", [s.server_address[1] for s in servers], flush=True)
    try:
        servers[0].serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    sys.exit(main())
