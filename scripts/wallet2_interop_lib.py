"""Transaction parsing, output attribution and RPC clients for scripts/wallet2_interop.py."""

import json
import urllib.request

from Crypto.Hash import keccak

P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = (-121665 * pow(121666, P - 2, P)) % P
SQRT_M1 = pow(2, (P - 1) // 4, P)


def _inv(x):
    return pow(x, P - 2, P)


def point_decompress(data):
    y = int.from_bytes(data, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x2 = (y * y - 1) * _inv(D * y * y + 1) % P
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P != 0:
        x = x * SQRT_M1 % P
    if (x * x - x2) % P != 0:
        raise ValueError("not a curve point")
    if (x & 1) != sign:
        x = P - x
    return (x, y, 1, x * y % P)


def point_compress(point):
    x, y, z, _ = point
    zi = _inv(z)
    x = x * zi % P
    y = y * zi % P
    return (y | ((x & 1) << 255)).to_bytes(32, "little")


def point_add(a, b):
    x1, y1, z1, t1 = a
    x2, y2, z2, t2 = b
    aa = (y1 - x1) * (y2 - x2) % P
    bb = (y1 + x1) * (y2 + x2) % P
    cc = 2 * t1 * t2 * D % P
    dd = 2 * z1 * z2 % P
    e, f, g, h = bb - aa, dd - cc, dd + cc, bb + aa
    return (e * f % P, g * h % P, f * g % P, e * h % P)


IDENTITY = (0, 1, 1, 0)
BASE = point_decompress((4 * _inv(5) % P).to_bytes(32, "little"))


def point_mul(k, point):
    result = IDENTITY
    while k:
        if k & 1:
            result = point_add(result, point)
        point = point_add(point, point)
        k >>= 1
    return result


def point_sub(a, b):
    x, y, z, t = b
    return point_add(a, ((P - x) % P, y, z, (P - t) % P))


def keccak256(data):
    return keccak.new(digest_bits=256, data=data).digest()


def hash_to_scalar(data):
    return int.from_bytes(keccak256(data), "little") % L


def enc_varint(value):
    out = bytearray()
    while value >= 0x80:
        out.append((value & 0x7F) | 0x80)
        value >>= 7
    out.append(value)
    return bytes(out)


def key_derivation(public, secret):
    return point_compress(point_mul(8, point_mul(secret, point_decompress(public))))


def derivation_scalar(derivation, index):
    return hash_to_scalar(derivation + enc_varint(index))


def view_tag(derivation, index):
    return keccak256(b"view_tag" + derivation + enc_varint(index))[0]


def spend_key_of_output(derivation, index, output_key):
    shared = point_mul(derivation_scalar(derivation, index), BASE)
    return point_compress(point_sub(point_decompress(output_key), shared))


def decrypt_amount(derivation, index, encrypted):
    scalar = derivation_scalar(derivation, index).to_bytes(32, "little")
    mask = keccak256(b"amount" + scalar)[:8]
    return int.from_bytes(bytes(a ^ b for a, b in zip(encrypted, mask)), "little")


def payment_id_mask(derivation):
    return keccak256(derivation + b"\x8d")[:8]


_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
_BLOCK_SIZES = [0, 2, 3, 5, 6, 7, 9, 10, 11]


def base58_decode(text):
    out = bytearray()
    for start in range(0, len(text), 11):
        block = text[start : start + 11]
        size = _BLOCK_SIZES.index(len(block))
        value = 0
        for char in block:
            value = value * 58 + _ALPHABET.index(char)
        out += value.to_bytes(size, "big")
    return bytes(out)


def decode_address(address):
    raw = base58_decode(address)
    body, checksum = raw[:-4], raw[-4:]
    if keccak256(body)[:4] != checksum:
        raise ValueError("bad address checksum")
    kind = {18: "standard", 19: "integrated", 42: "subaddress"}[body[0]]
    return {
        "kind": kind,
        "spend": body[1:33],
        "view": body[33:65],
        "payment_id": body[65:73] if kind == "integrated" else None,
    }



class _Reader:
    def __init__(self, data):
        self.data = data
        self.pos = 0

    def varint(self):
        value = shift = 0
        while True:
            byte = self.data[self.pos]
            self.pos += 1
            value |= (byte & 0x7F) << shift
            if not byte & 0x80:
                return value
            shift += 7

    def take(self, count):
        chunk = self.data[self.pos : self.pos + count]
        if len(chunk) != count:
            raise ValueError("truncated transaction")
        self.pos += count
        return chunk

    def byte(self):
        return self.take(1)[0]


def parse_extra(extra):
    reader = _Reader(extra)
    fields = []
    while reader.pos < len(extra):
        tag = reader.byte()
        if tag == 0x01:
            fields.append({"tag": "pubkey", "key": reader.take(32).hex()})
        elif tag == 0x02:
            nonce = reader.take(reader.varint())
            field = {"tag": "nonce", "len": len(nonce), "nonce": nonce.hex()}
            if len(nonce) == 9 and nonce[0] == 0x01:
                field["kind"] = "encrypted_payment_id"
                field["payment_id"] = nonce[1:].hex()
            elif len(nonce) == 33 and nonce[0] == 0x00:
                field["kind"] = "long_payment_id"
            else:
                field["kind"] = "other"
            fields.append(field)
        elif tag == 0x04:
            count = reader.varint()
            fields.append(
                {"tag": "additional_pubkeys", "keys": [reader.take(32).hex() for _ in range(count)]}
            )
        elif tag == 0x00:
            rest = extra[reader.pos :]
            fields.append({"tag": "padding", "len": 1 + len(rest), "all_zero": not any(rest)})
            break
        else:
            fields.append({"tag": "unknown", "byte": tag, "rest": extra[reader.pos :].hex()})
            break
    return fields


def parse_tx(blob):
    reader = _Reader(blob)
    tx = {"version": reader.varint(), "unlock_time": reader.varint(), "vin": [], "vout": []}
    for _ in range(reader.varint()):
        tag = reader.byte()
        if tag != 0x02:
            raise ValueError(f"unsupported input type {tag}")
        amount = reader.varint()
        offsets = [reader.varint() for _ in range(reader.varint())]
        tx["vin"].append({"amount": amount, "key_offsets": offsets, "key_image": reader.take(32).hex()})
    for _ in range(reader.varint()):
        amount = reader.varint()
        tag = reader.byte()
        out = {"amount": amount, "type": tag, "key": reader.take(32).hex()}
        if tag == 0x03:
            out["view_tag"] = reader.byte()
        elif tag != 0x02:
            raise ValueError(f"unsupported output type {tag}")
        tx["vout"].append(out)
    extra = reader.take(reader.varint())
    tx["extra"] = extra.hex()
    tx["extra_fields"] = parse_extra(extra)
    prefix_end = reader.pos

    rct_type = reader.byte()
    tx["rct_type"] = rct_type
    if rct_type not in (5, 6):
        raise ValueError(f"unsupported rct type {rct_type}")
    fee_start = reader.pos
    tx["fee"] = reader.varint()
    tx["fee_varint_len"] = reader.pos - fee_start
    tx["ecdh"] = [reader.take(8).hex() for _ in tx["vout"]]
    tx["out_pk"] = [reader.take(32).hex() for _ in tx["vout"]]
    base_end = reader.pos

    proofs = []
    for _ in range(reader.varint()):
        fixed = [reader.take(32).hex() for _ in range(6)]
        left = [reader.take(32).hex() for _ in range(reader.varint())]
        right = [reader.take(32).hex() for _ in range(reader.varint())]
        tail = [reader.take(32).hex() for _ in range(3)] if rct_type == 5 else []
        proofs.append({"fixed": fixed, "L": len(left), "R": len(right), "tail": len(tail)})
    tx["bulletproofs"] = proofs
    tx["clsags"] = []
    for vin in tx["vin"]:
        s = [reader.take(32) for _ in vin["key_offsets"]]
        reader.take(32)  # c1
        reader.take(32)  # D
        tx["clsags"].append(len(s))
    tx["pseudo_outs"] = [reader.take(32).hex() for _ in tx["vin"]]
    if reader.pos != len(blob):
        raise ValueError(f"{len(blob) - reader.pos} trailing bytes")

    tx["size"] = len(blob)
    tx["prefix_size"] = prefix_end
    tx["base_size"] = base_end - prefix_end
    tx["prunable_size"] = len(blob) - base_end
    tx["weight"] = tx_weight(len(blob), len(tx["vout"]), rct_type == 6)
    tx["txid"] = tx_hash(blob, prefix_end, base_end)
    return tx


def tx_hash(blob, prefix_end, base_end):
    parts = keccak256(blob[:prefix_end]) + keccak256(blob[prefix_end:base_end]) + keccak256(blob[base_end:])
    return keccak256(parts).hex()


def tx_weight(size, outputs, plus=True):
    if outputs <= 2:
        return size
    fields = 6 if plus else 9
    bp_base = (32 * (fields + 7 * 2)) // 2
    log_padded = 2
    while (1 << log_padded) < outputs:
        log_padded += 1
    bp_size = 32 * (fields + 2 * (6 + log_padded))
    return size + (bp_base * (1 << log_padded) - bp_size) * 4 // 5


def fee_for_weight(weight, per_byte, mask):
    fee = weight * per_byte
    return (fee + mask - 1) // mask * mask


def layout(tx):
    images = [bytes.fromhex(vin["key_image"]) for vin in tx["vin"]]
    return {
        "version": tx["version"],
        "unlock_time": tx["unlock_time"],
        "inputs": len(tx["vin"]),
        "ring_sizes": sorted({len(vin["key_offsets"]) for vin in tx["vin"]}),
        "input_amounts": sorted({vin["amount"] for vin in tx["vin"]}),
        "key_images_descending": all(a > b for a, b in zip(images, images[1:])),
        "outputs": len(tx["vout"]),
        "output_types": sorted({out["type"] for out in tx["vout"]}),
        "output_amounts": sorted({out["amount"] for out in tx["vout"]}),
        "extra_len": len(tx["extra"]) // 2,
        "extra_layout": [
            field["tag"]
            + (f":{field['kind']}" if field["tag"] == "nonce" else "")
            + (f"x{len(field['keys'])}" if field["tag"] == "additional_pubkeys" else "")
            for field in tx["extra_fields"]
        ],
        "rct_type": tx["rct_type"],
        "bulletproofs": [(proof["L"], proof["R"]) for proof in tx["bulletproofs"]],
        "clsag_ring_sizes": sorted(set(tx["clsags"])),
        "pseudo_outs": len(tx["pseudo_outs"]),
    }


def attribute_outputs(tx, viewers):
    main = [f["key"] for f in tx["extra_fields"] if f["tag"] == "pubkey"]
    additional = [f["keys"] for f in tx["extra_fields"] if f["tag"] == "additional_pubkeys"]
    additional = additional[0] if additional else []
    owned = []
    for viewer in viewers:
        secret = int.from_bytes(bytes.fromhex(viewer["view"]), "little")
        for index, out in enumerate(tx["vout"]):
            candidates = [("main", key) for key in main]
            if index < len(additional):
                candidates.append(("additional", additional[index]))
            for source, key in candidates:
                derivation = key_derivation(bytes.fromhex(key), secret)
                if "view_tag" in out and view_tag(derivation, index) != out["view_tag"]:
                    continue
                spend = spend_key_of_output(derivation, index, bytes.fromhex(out["key"]))
                for label, known in viewer["spends"].items():
                    if spend == known:
                        owned.append(
                            {
                                "viewer": viewer["name"],
                                "index": index,
                                "label": label,
                                "derivation": source,
                                "amount": decrypt_amount(derivation, index, bytes.fromhex(tx["ecdh"][index])),
                                "payment_id_mask": payment_id_mask(derivation).hex(),
                            }
                        )
    return owned



class RpcError(Exception):
    pass


def _post(url, payload, timeout=600):
    request = urllib.request.Request(
        url, data=json.dumps(payload).encode(), headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


class JsonRpc:
    def __init__(self, url):
        self.url = url.rstrip("/")

    def call(self, method, params=None):
        reply = _post(self.url + "/json_rpc", {"jsonrpc": "2.0", "id": "0", "method": method, "params": params or {}})
        if "error" in reply:
            raise RpcError(f"{method}: {reply['error']}")
        return reply["result"]

    def try_call(self, method, params=None):
        try:
            return {"ok": self.call(method, params)}
        except RpcError as error:
            return {"error": str(error)}

    def other(self, path, params=None):
        return _post(self.url + path, params or {})
