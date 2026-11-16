# monero-rust

A Monero wallet library in Rust using [`monero-oxide`](https://github.com/monero-oxide/monero-oxide) 
and its [`monero-wallet`](https://github.com/monero-oxide/monero-oxide/tree/main/monero-oxide/wallet) 
that runs natively and in the browser (`wasm32-unknown-unknown`).  An RPC 
client, block scanning, wallet state, transaction construction, and the file 
and QR formats other Monero wallets (like `wallet2` and Feather) exchange.

## Synopsis

- Seeds and keys: generates and restores classic 25-word seeds (with seed
  offset passphrases, in all wallet2 languages) and converts 12-word BIP39
  mnemonics using the Cake Wallet "Exodus style" derivation. Derives
  addresses and subaddresses, and restores view-only wallets from keys.
  Polyseed (16-word) support was recently removed but will be re-added. 
- Daemon RPC: a JSON-RPC and binary Epee client over a pluggable connection
  trait, with an HTTP transport (including digest auth) on native targets.
  Untrusted Epee input is structurally validated before deserialization.
- Scanning: batched and pipelined block scanning for one or many wallets,
  subaddress lookahead, mempool scanning, spent detection by key image, and
  reorg detection with rollback.
- Wallet state: outputs, balances with lock and unlock rules, freezing,
  pending spends, tracked transactions, and spend conflict detection.
- Sending: coin selection, fee estimation, decoy selection matching
  `monero-wallet`, transfers, sweeps, and broadcast with classified errors.
- Cold signing and interop: reads and writes wallet2 unsigned and signed
  transaction sets, output exports, and key image exports (v3 format and
  legacy Epee), so view-only and offline signing flows work with
  monero-wallet-cli and Feather. Imported key images are verified before use.
- Files and transport: reads and writes monero-wallet-cli `.keys` files,
  encrypts arbitrary wallet data with Argon2 and ChaCha20-Poly1305, and
  encodes payloads as animated UR QR codes (`xmr-txunsigned`,
  `xmr-txsigned`, `xmr-keyimage`, `xmr-output`).

The crate lives in `rust/monero-rust`. CryptoNight hashing (Cuprate) and Epee
serialization use pinned upstream Git revisions because their crates.io
releases are placeholders or lack needed fixes.

## Requirements

- Rust 1.89.0+, with the `wasm32-unknown-unknown` target
- `monerod` to run the regtest tests

## Testing

```sh
scripts/test-fast.sh   # check formatting, compile, and run selected unit tests
scripts/test-all.sh    # run the full suite; use --no-regtest to skip regtest
```

The full suite reports skips when `wasm-bindgen-test-runner` or `monerod` is
unavailable. Tests check against vectors from upstream Monero, Feather, and
Ledger, and against a regtest `monerod`.

## Acknowledgements

- Thank you Diego "rehrar" Salazar and Cypher Stack for commissioning me to
  prove this concept via the https://github.com/cypherstack/libxmr project.
- Thank you Luke "kayabaNerve" Parker and Serai and Boog900 and the monero-oxide contributors for `monero-serai` and `monero-oxide`, respectively.
- Thank you Cake Wallet for your "Exodus style" BIP39 test vectors and implementation.
