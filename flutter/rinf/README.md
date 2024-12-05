# monero-rust/flutter/rinf
Flutter web extension demonstrating monero-wasm with 
[rinf](https://github.com/cunarist/rinf) due to CORS restrictions, which add 
constraints incompatible with regular Flutter web webpages.

## Requirements
- Flutter 3.24.3
- Rust 1.89.0
- rinf CLI: `cargo install rinf_cli`

## Setup
```sh
flutter pub get
rinf gen
rinf wasm
```

## Build
```sh
dart run tool/build_extension.dart
```

Output: `build/extension/` (unpacked) and `build/monero-extension.zip`

Load in Chrome: open `chrome://extensions`, enable Developer mode, click "Load unpacked", select `build/extension/`.

For UI-only changes, rebuild just the Flutter Web aspect and pack a new .zip as in:

```sh
dart run tool/build_ui.dart
```

## Testing
Rust:
```sh
cd native/monero-wasm
cargo test --lib
```

Flutter:
```sh
flutter test
```

E2E:
```sh
npm install
npm test
```
