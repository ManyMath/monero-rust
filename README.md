# monero-rust

Monero wallet tooling in Rust.

## Workspace

- `rust/monero-rust`: wallet library
- `rust/monero-utils/polyseed`: local Polyseed crate

The wallet library depends on the published `monero-oxide` and `monero-wallet` crates.
CryptoNight hashing and Epee serialization use pinned upstream Git revisions.

## Reduced feature branch

This branch removes the vendored payment-proof implementation and the public
`tx_proof` module. The published `monero-payment-proofs` 0.0.1 package is an
empty placeholder, so OutProofV2 generation and verification are unavailable.
The complete implementation remains on `staging`.

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
