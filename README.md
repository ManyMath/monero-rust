# monero-rust
Rust Monero wallet tooling compiled to WebAssembly, with a Flutter web extension example.

## Requirements
- Flutter 3.24.4+
- Rust 1.82.0+
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

## Notes
This runs as a browser extension to bypass CORS restrictions when talking to Monero nodes. Most nodes don't send headers that allow arbitrary web origins. The extension sidesteps this for testing purposes.

# Acknowledgements
- Thank you Diego "rehrar" Salazar and Cypher Stack for commissioning me to
  prove this concept via the https://github.com/cypherstack/libxmr project.
- Thank you Luke "kayabaNerve" Parker and Serai and Boog900 and the monero-oxide contributors for `monero-serai` and `monero-oxide`, respectively.
- Thank you Cake Wallet for your "Exodus style" BIP39 test vectors and implementation.
