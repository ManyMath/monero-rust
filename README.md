# `monero-rust`
A proof-of-concept Monero SDK oriented towards use by Dart.  Seeks to provide 
bindings for Rust crates such as `monero-serai` (soon to be `monero-wallet` and 
`monero-oxide` less soon) and `cuprate` (soon™).

## Development

- Install `cbindgen`: `cargo install --force cbindgen`.
- To generate `monero-rust.h` C bindings for Rust, use `cbindgen` in the 
  `monero-rust` directory:
  ```
  cbindgen --config cbindgen.toml --crate libxmr --output monero-rust.h
  ```

# Roadmap

- Scan transactions for incoming funds.
- Match wallet2 API.
- `monero-wallet-cli`
- `monero-wallet-rpc`
- `monerod`
- Securely zero memory after secrets are used.

# Acknowledgements

- Thank you Luke "kayabaNerve" Parker and Serai for `monero-serai`.
- Thank you Diego "rehrar" Salazar and Cypher Stack for commissioning me to 
  prove this concept via the https://github.com/cypherstack/libxmr project.
