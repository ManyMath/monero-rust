# monero_extension
A Flutter web app demonstrating monero-wasm functionality.

## Prerequisites
### Required
- Flutter SDK (3.24.3 or higher)
- Rust toolchain (latest stable)
- rinf CLI: `cargo install rinf_cli`

### Optional (for testing)
- wasm-pack: For WASM browser tests
- chromedriver: For headless browser testing

## Setup
1. Install dependencies:
   ```sh
   cd monero-rust/dart/monero_extension
   flutter pub get
   ```

2. Generate Dart bindings from Rust:
   ```sh
   rinf gen
   ```

3. Build WASM module:
   ```sh
   rinf wasm
   ```

## Build
### Development Build
```sh
rinf gen          # Generate Dart bindings from Rust signals
rinf wasm         # Compile Rust to WASM
flutter build web # Build Flutter web app
```

### Production Build
```sh
rinf gen
rinf wasm --release
flutter build web --release
```
The output will be in `build/web/`.

## Run
### Development Server
```sh
rinf server       # Copies the full flutter run command to clipboard
```

This will copy a command like the following to your clipboard:
```sh
flutter run \
  --web-header=cross-origin-opener-policy=same-origin \
  --web-header=cross-origin-embedder-policy=require-corp
```

Paste and run the command to start the development server.

## Testing
### Flutter Unit Tests
```sh
cd monero-rust/dart/monero_extension
flutter test
```

### Native Rust Tests
```sh
cd monero-rust/dart/monero_extension/native/monero-wasm
cargo test --lib
```

### WASM Browser Tests
```sh
cd monero-rust/dart/monero_extension/native/monero-wasm

# Chrome (requires chromedriver)
wasm-pack test --headless --chrome

# Firefox (requires geckodriver)
wasm-pack test --headless --firefox
```

## Project Structure
```text
dart/monero_extension/
├── lib/
│   ├── main.dart                     # App entry point
│   └── src/
│       └── bindings/                 # Generated Dart bindings
│           └── signals/              # Rust signal handlers
├── native/
│   ├── hub/                          # Rinf message hub
│   │   └── src/
│   │       ├── signals/              # Signal definitions
│   │       └── actors/               # Signal processors
│   └── monero-wasm/                  # Core Monero implementation
│       ├── src/
│       │   └── lib.rs                # Seed & address functions + tests
│       └── Cargo.toml
├── web/
│   ├── index.html                    # Web entry point
│   └── pkg/                          # WASM output (generated)
├── pubspec.yaml                      # Flutter dependencies
└── README.md                         # This file
```
