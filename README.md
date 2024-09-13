# `monero-rust`

A Monero Rust SDK oriented towards use by Dart.

## Development

- Install `cbindgen`: `cargo install --force cbindgen`.
- To generate `monero-rust.h` C bindings for Rust, use `cbindgen` in the `monero-rust` directory:
  ```
  cbindgen --config cbindgen.toml --crate libxmr --output monero-rust.h
  ```
