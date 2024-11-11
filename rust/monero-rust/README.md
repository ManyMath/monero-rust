# `monero-rust`
Monero in Rust.  Contains both `monero-wasm`, oriented for use with Flutter Web 
for browser extensions and soon to be merged back into `monero-rust` under 
feature flags, and `wallet2`, a compatibility layer for legacy Monero Project 
file formats.

Uses `monero-serai` (soon to be `monero-wallet` and `monero-oxide` less soon) 
and `cuprate` (soon™).

## Development
- Install `cbindgen`:
  ```sh
  cargo install --force cbindgen
  ```

- To generate `monero-rust.h` C bindings for Rust, use `cbindgen` in the 
  `monero-rust` directory:
  ```sh
  cbindgen --config cbindgen.toml --crate monero-rust --output monero-rust.h
  ```

# Acknowledgements
- Thank you Luke "kayabaNerve" Parker and Serai for `monero-serai`.
- Thank you Diego "rehrar" Salazar and Cypher Stack for commissioning me to 
  prove this concept via the https://github.com/cypherstack/libxmr project.
