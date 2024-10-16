# monero_extension
A Flutter web app demonstrating monero-wasm functionality.

## Setup
Requires:
- Flutter SDK
- Rust toolchain
- `cargo install rinf_cli`

## Build
```sh
rinf gen          # Generate Dart bindings
rinf wasm         # Build WASM from Rust
flutter build web # Build web app
```

## Run
```sh
rinf server       # Get full `flutter run` command for server
```
Which will place a command into the clipboard like:
```sh
flutter run \
  --web-header=cross-origin-opener-policy=same-origin \
  --web-header=cross-origin-embedder-policy=require-corp
```
Paste the command from the clipboard to start the server.
