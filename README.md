# monero-rust
Rust Monero wallet tooling compiled to WebAssembly.

## Requirements
- Rust 1.89.0+, with the `wasm32-unknown-unknown` target

## Structure
### rust/monero-rust
WebAssembly library providing Monero wallet primitives for browser environments. Abstracts networking, storage, and time for web platform constraints.

Test everything, in one command:
```sh
scripts/test-all.sh              # --no-regtest skips the tier that needs monerod
```

Five tiers: the default suite, the wasm32 build, the wasm32 unit tests, the mock-rpc
send path and the regtest end-to-end tests. A missing or broken toolchain is reported
as a **skip**; a tier that runs and fails is a **failure**. The two are never conflated.

Or just the library's own unit tests:
```sh
cd rust/monero-rust
cargo test --lib
```

### The Flutter extension
The example browser extension lives in its own repository now, and consumes this one as a
submodule at `vendor/monero-rust`. It carries the five tiers that need it - the
`monero_wasm` wasm32 build, the signal generator's tests, the generated-signal-surface
check, and the dart and browser suites - in its own `scripts/test-all.sh`. Neither
runner covers the other.

### Constraints
The example app runs as a browser extension because Monero RPC nodes don't typically send CORS headers allowing arbitrary web origins.  Running as an extension bypasses these restrictions, enabling direct communication with nodes for testing.  This is one of monero-rust's several web- or extension-based constraints:
- CORS restrictions apply when making RPC calls to Monero nodes.
- All network calls must go through the browser's `fetch` API.
- Storage uses browser localStorage/IndexedDB via traits.
- No filesystem or native OS dependencies.

## Notes
This runs as a browser extension to bypass CORS restrictions when talking to Monero nodes. Most nodes don't send headers that allow arbitrary web origins. The extension sidesteps this for testing purposes.

# Acknowledgements
- Thank you Diego "rehrar" Salazar and Cypher Stack for commissioning me to
  prove this concept via the https://github.com/cypherstack/libxmr project.
- Thank you Luke "kayabaNerve" Parker and Serai and Boog900 and the monero-oxide contributors for `monero-serai` and `monero-oxide`, respectively.
- Thank you Cake Wallet for your "Exodus style" BIP39 test vectors and implementation.
