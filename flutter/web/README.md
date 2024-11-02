# web
A Flutter web extension demonstrating monero-wasm integration.  Due to security restrictions this cannot be fully tested running on a local webserver (like with the usual `flutter run -d chrome` or `rinf serve`) and must be built as an extension and loaded into a browser.

## Prerequisites
- Flutter SDK (3.24.3)
- Rust toolchain (1.89.0)
- rinf CLI: `cargo install rinf`

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

Load in Chrome:
- Go to `chrome://extensions`
- Enable Developer mode
- Click "Load unpacked"
- Select `build/extension/` directory

The extension bypasses CORS restrictions for testing node connectivity.

## Testing
### Rust Tests
```sh
cd native/monero-wasm
cargo test --lib
```

### Flutter Tests
```sh
flutter test
```

### E2E Tests
```sh
npm install
npm test
```
