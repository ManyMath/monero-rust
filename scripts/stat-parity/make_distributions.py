#!/usr/bin/env python3
"""Write the cumulative RingCT output distributions <name>, <name>_zeros and dense.

    python make_distributions.py <reply.json> <name> <out_dir>

<reply.json> is a synced daemon's get_output_distribution reply for amount 0, cumulative false.
"""
import json
import sys

import numpy as np


def write(path, per_block):
    np.cumsum(np.asarray(per_block, dtype=np.uint64)).astype("<u8").tofile(path)
    print(path, "blocks", len(per_block), "outputs", int(np.sum(per_block)))


def main():
    source, name, out = sys.argv[1:4]
    with open(source) as f:
        reply = json.load(f)
    entry = reply.get("result", reply)["distributions"][0]
    assert entry["amount"] == 0 and entry["base"] == 0
    stagenet = np.asarray(entry["distribution"], dtype=np.uint64)
    write(f"{out}/{name}.u64", stagenet)

    zeros = stagenet.copy()
    tail = np.arange(len(zeros) - 3000, len(zeros))
    zeros[tail[tail % 3 != 0]] = 0
    write(f"{out}/{name}_zeros.u64", zeros)

    rng = np.random.default_rng(20261118)
    dense = stagenet.copy()
    year = 262800
    mean = np.linspace(40, 120, year)
    dense[-year:] = 1 + rng.poisson(mean)
    write(out + "/dense.u64", dense)


if __name__ == "__main__":
    main()
