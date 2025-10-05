# `monero-rust`
A proof-of-concept Monero SDK oriented towards use by Dart. Uses `monero-wallet`
from the `monero-oxide`  project, vendored as a git submodule.

## Setup

After cloning this repository, initialize the git submodules:
```bash
git submodule update --init --recursive
```

## Development
- Install `cbindgen`: `cargo install --force cbindgen`.
- To generate `monero-rust.h` C bindings for Rust, use `cbindgen` in the 
  `monero-rust` directory:
  ```
  cbindgen --config cbindgen.toml --crate monero-rust --output monero-rust.h
  ```

# Roadmap
- Scan transactions for incoming funds.
- Match wallet2 API.
- `monero-wallet-cli`
- `monero-wallet-rpc`
- `monerod`
- Securely zero memory after secrets are used.

# Acknowledgements
- Thank you Luke "kayabaNerve" Parker and Serai for `monero-serai` and the 
  Monero Oxide team for `monero-oxide`.
- Thank you Diego "rehrar" Salazar and Cypher Stack for commissioning me to 
  prove this concept via the https://github.com/cypherstack/libxmr project.
