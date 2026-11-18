# Statistical comparison with wallet2

Large-sample comparison of monero-rust with wallet2: decoy ages, ring
construction and input selection. The quick checks are in
`rust/monero-rust/tests/stat_parity_harness.rs`.

## Environment

| variable | meaning |
| --- | --- |
| `STAT_PARITY_DIR` | scratch directory, default `$TMPDIR/monero-rust-stat-parity` |
| `STAT_PARITY_SAMPLES`, `STAT_PARITY_PICKS`, `STAT_PARITY_RESULTS` | default `samples`, `picks`, `results` under `STAT_PARITY_DIR` |
| `MONEROD`, `MONERO_WALLET_RPC` | wallet2 reference binaries, default from `PATH` |
| `MONERO_SRC`, `MONERO_BUILD` | monero source tree and its cmake build directory, for `build_w2harness.sh` |
| `W2HARNESS_DIR` | harness binaries, default `$STAT_PARITY_DIR/w2build` |

Python needs numpy and scipy.

## Reproduce

From the repository root, with the variables above exported:

```sh
S=scripts/stat-parity
R=$STAT_PARITY_DIR
(cd rust && cargo test -p monero-rust --release --test stat_parity_harness --no-run)
BIN=$(ls -t ${CARGO_TARGET_DIR:-rust/target}/release/deps/stat_parity_harness-* | grep -v '\.d$' | head -1)
$S/build_w2harness.sh
python $S/build_chain.py                           # about an hour

# picker level; reply.json: get_output_distribution, amount 0, cumulative false
python $S/make_distributions.py reply.json stagenet $R/dist
for d in stagenet stagenet_zeros dense; do $S/run_picker.sh $d 5000; done
for s in T0 T1 S L; do python $S/run_regtest.py node $s; done
for d in regtest_T1 regtest_S regtest_L; do $S/run_picker.sh $d 5000; done
for k in 3 4 5; do $R/w2build/w2harness-seeded pick $R/dist/dense.u64 40000000 $k $R/picks/dense.w2.seed$k.u64; done
for k in $(seq 201 212); do STATP_DIST=$R/dist/dense.u64 STATP_OUT=$R/picks/dense.rust.seed$k STATP_SEED=$k \
    STATP_BUILDS=5000 STATP_INPUTS=8 $BIN picker_offline --exact; done

# whole pipeline on regtest
python $S/run_regtest.py rings T0 100 4
for h in 90 100 120 150; do python $S/run_regtest.py pop T1 P$h $h; python $S/run_regtest.py rings P$h 50 4; done
python $S/run_regtest.py rings T1 200 8 wallet2; python $S/run_regtest.py rings T1 600 8 rust
python $S/run_regtest.py rings S 600 12 rust;    python $S/run_regtest.py rings S 600 12 wallet2
python $S/run_regtest.py rings L 200 12 rust;    python $S/run_regtest.py rings L 200 12 wallet2
python $S/run_regtest.py multi S 250
python $S/run_regtest.py multi L 250
python $S/run_regtest.py select 300 5000
python $S/run_walletrpc.py select 150
python $S/run_walletrpc.py rings 12
python $S/run_regtest.py stop

# analysis
for d in stagenet stagenet_zeros dense regtest_T1 regtest_S regtest_L; do python $S/analyze_picker.py $d; done
python $S/analyze_replication.py
for s in T0 P90 P100 P120 P150 T1 S L; do python $S/analyze_regtest.py rings $s; done
for s in S L; do python $S/analyze_regtest.py multi $s; done
python $S/analyze_regtest.py rpccheck
python $S/analyze_select.py select
python $S/analyze_select.py rpccheck
python $S/report_tables.py > $R/results/tables.md
python $S/report_summary.py > $R/results/summary.md
```

Seeds are fixed in the scripts, but the `w2harness-seeded wallet` runs do not
reproduce sample for sample: wallet2's other threads draw from the same
generator.

On a busy machine, run the wallet2 and monero-rust halves of a `rings`
experiment one after the other: with both on one monerod, connections get
dropped. Both samplers count retries per sample; a clean run has none.

`WALLET2_AGE_SHARES` in `stat_parity_harness.rs` comes from
`w2harness-seeded pick` on a chain of 100,000 blocks with 10 outputs each,
seeds 1 and 2, 40 million draws each, grouped by `analyze_picker.group`.
