# monero-rust

Monero wallet tooling in Rust.

## Workspace

- `rust/monero-rust`: wallet library
- `rust/monero-epee-bin-serde`: Epee binary serialization
- `rust/cuprate-cryptonight`: legacy CryptoNight hashing used by wallet compatibility code
- `rust/monero-utils`: utility crates awaiting suitable upstream published releases

The wallet core uses the published `monero-oxide` and `monero-wallet` crates.

## Related repositories

- [`monero-ffi`](https://git.manymath.com/sneurlax/monero-ffi): Dart/Flutter bindings and browser transport
- [`monero-cli`](https://git.manymath.com/sneurlax/monero-cli): command-line wallet
- [`monero-extension`](https://git.manymath.com/sneurlax/monero-extension): Flutter browser extension built on `monero-ffi`

## Requirements

- Rust 1.89.0+, with the `wasm32-unknown-unknown` target
- `monerod` to run the regtest tests

## Testing

```sh
scripts/test-all.sh    # --no-regtest skips the tier that needs monerod
```

A missing optional toolchain is reported as a skip, not a pass.

## Acknowledgements

- Thank you Diego "rehrar" Salazar and Cypher Stack for commissioning me to
  prove this concept via the https://github.com/cypherstack/libxmr project.
- Thank you Luke "kayabaNerve" Parker and Serai and Boog900 and the monero-oxide contributors for `monero-serai` and `monero-oxide`, respectively.
- Thank you Cake Wallet for your "Exodus style" BIP39 test vectors and implementation.
