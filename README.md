# monero-rust
Rust Monero wallet tooling compiled to WebAssembly, with a Flutter web extension example.

## Requirements
- Flutter 3.24.3+
- Rust 1.89.0+
- rinf CLI: `cargo install rinf`

## Structure
### rust/monero-rust
WebAssembly library providing Monero wallet primitives for browser environments. Abstracts networking, storage, and time for web platform constraints.

Test:
```sh
cd rust/monero-rust
cargo test --lib
```

### flutter/web
Flutter web extension demonstrating monero-rust. See `flutter/web/README.md` for details.

Build:
```sh
cd flutter/web
flutter pub get
rinf gen
rinf wasm
dart run tool/build_extension.dart
```

Output goes to `build/extension/` (unpacked) and `build/monero-extension.zip`.

Load in Chrome: open `chrome://extensions`, enable Developer mode, click "Load unpacked", select `build/extension/`.

Test:
```sh
cd flutter/web
flutter test
npm install
npm run test:signal-e2e -- test/e2e/multi_wallet_scan.test.js
npm test
```

`npm run test:signal-e2e -- <jest paths...>` is the dedicated Puppeteer/Jest signal entrypoint.
`npm test` remains the broader repo-style path and still runs the Rust test suite before signal E2E, so unrelated Rust regressions can still fail that path.

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
