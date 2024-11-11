# monero-rust
Rust Monero wallet tooling compiled to WebAssembly, with a Flutter web extension example.

## Requirements
- Flutter 3.24.3+
- Rust 1.89.0+
- rinf CLI: `cargo install rinf`

## Structure
### rust/monero-wasm
WebAssembly library providing Monero wallet primitives for browser environments. Abstracts networking, storage, and time for web platform constraints.

Test:
```sh
cd rust/monero-wasm
cargo test --lib
```

### flutter/web
Flutter web extension demonstrating monero-wasm. See `flutter/web/README.md` for details.

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
npm install && npm test
```

## Notes
This runs as a browser extension to bypass CORS restrictions when talking to Monero nodes. Most nodes don't send headers that allow arbitrary web origins. The extension sidesteps this for testing purposes.
