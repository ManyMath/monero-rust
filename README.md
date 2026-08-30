# monero-rust

Monero wallet tooling in Rust.

## Workspace

- `rust/monero-rust`: wallet library
- `rust/monero-utils`: local utility crates retained until upstream releases can replace them

The wallet library depends on the published `monero-oxide` and `monero-wallet` crates.
CryptoNight hashing and Epee serialization use pinned upstream Git revisions.

## Related repositories

- [`monero-ffi`](https://git.manymath.com/sneurlax/monero-ffi): Dart/Flutter bindings and browser transport
- [`monero-cli`](https://git.manymath.com/sneurlax/monero-cli): command-line wallet
- [`monero-extension`](https://git.manymath.com/sneurlax/monero-extension): Flutter browser extension built on `monero-ffi`

## Requirements

- Rust 1.89.0+, with the `wasm32-unknown-unknown` target
- `monerod` to run the regtest tests

## Testing

```sh
scripts/test-fast.sh   # check formatting, compile, and run selected unit tests
scripts/test-all.sh    # run the full suite; use --no-regtest to skip regtest
```

The full suite reports skips when `wasm-bindgen-test-runner` or `monerod` is unavailable.

## Acknowledgements

- Thank you Diego "rehrar" Salazar and Cypher Stack for commissioning me to
  prove this concept via the https://github.com/cypherstack/libxmr project.
- Thank you Luke "kayabaNerve" Parker and Serai and Boog900 and the monero-oxide contributors for `monero-serai` and `monero-oxide`, respectively.
- Thank you Cake Wallet for your "Exodus style" BIP39 test vectors and implementation.
