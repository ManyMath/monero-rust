# `monero-rust`
A proof-of-concept Monero SDK oriented towards use by Dart.  Seeks to provide 
bindings for Rust crates such as `monero-serai` (soon to be `monero-wallet` and 
`monero-oxide` less soon) and `cuprate` (soon™).

## Development

- Install `cbindgen`: `cargo install --force cbindgen`.
- To generate `monero-rust.h` C bindings for Rust, use `cbindgen` in the `monero-rust` directory:
  ```
  cbindgen --config cbindgen.toml --crate libxmr --output monero-rust.h
  ```

# Roadmap

- Provide a hook for securely zeroing memory after secrets are used.
- Encrypt data while at-rest in memory.
- Encrypt FFI passage of sensitive data.

# Acknowledgements

Thank you Luke "kayabaNerve" Parker for your work on crates.io/crates/monero-wallet.
