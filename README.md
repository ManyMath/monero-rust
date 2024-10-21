# monero-rust and monero-wasm
A monorepo for monero-rust (native use) and monero-wasm (web use).

## Quickstart
```sh
cd monero-rust/dart/monero_extension

# Generate Dart bindings from Rust
rinf gen

# Build WASM module
rinf wasm

# Build Flutter web app
flutter build web

# Run the app (get the command with proper headers)
rinf server
```

The `rinf server` command will copy a Flutter run command to your clipboard 
with the required CORS headers.  Paste and run it as in:
```sh
flutter run \
  --web-header=cross-origin-opener-policy=same-origin \
  --web-header=cross-origin-embedder-policy=require-corp
```

## Project Overview
- monero-rust: Core Monero cryptographic operations using monero-serai
- monero-wasm: Browser-compatible WebAssembly modules for web applications
- monero_extension: A Flutter Web demo using rinf

## Repository Structure
```text
monero-rust/
├── rust/                          # Core Rust workspace
│   ├── monero-rust/               # Native Monero library
│   ├── monero-wasm/               # WASM abstractions and utilities
│   └── monero-serai-mirror/       # Local monero-serai dependency
└── dart/                          # Flutter integration
    └── monero_extension/          # Flutter web demo app
        ├── lib/                   # Dart source code
        ├── web/                   # Web assets
        └── native/                # Rust code for Flutter integration
            ├── hub/               # Rinf signal handling
            └── monero-wasm/       # Core monero implementation
```

## Prerequisites
### For Native Development
- Rust toolchain (latest stable)
- Cargo

### For WASM Development
- wasm-pack: `curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh`
- wasm-bindgen-cli: `cargo install wasm-bindgen-cli`

### For Flutter Development
- Flutter SDK (3.24.3 or higher)
- rinf CLI: `cargo install rinf_cli`

## Testing
### Native Rust Tests
Run the library tests as in:
```sh
cd monero-rust/rust
cargo test --package monero-wasm --lib # or the monero-rust package
# or cargo test --all
```
and/or the integration tests as in:
```sh
cd monero-rust/dart/monero_extension/native/monero-wasm
cargo test --lib
```

### WASM Browser Tests
```sh
cd monero-rust/dart/monero_extension/native/monero-wasm
wasm-pack test --headless --chrome # or --firefox
```
**Note:** Browser tests require a WebDriver (chromedriver or geckodriver) to be installed and in your PATH.

#### Flutter Unit Tests
```sh
cd monero-rust/dart/monero_extension
flutter test
```

## Troubleshooting
### WASM tests fail with "WebDriver not found"
Install chromedriver or geckodriver:
```sh
# macOS
brew install chromedriver

# or for Firefox
brew install geckodriver
```

### Flutter build fails with "rinf not found"
Install rinf CLI:
```sh
cargo install rinf_cli
```

### Cargo test fails with dependency errors
Update the monero-serai submodule:
```sh
cd monero-rust/rust/monero-serai-mirror
git submodule update --init --recursive
```

### WASM module not loading in Flutter web
Ensure you're using the correct CORS headers when running:
```sh
rinf server  # Paste the command it puts in the clipboard and run it
```

## Development Workflow
1. Make changes to Rust code
```sh
cd monero-rust/rust
cargo test --all
```

2. Test in Flutter app
```sh
cd monero-rust/dart/monero_extension
rinf gen
rinf server # then paste the command and run it
```

3. Run WASM browser tests
```sh
cd monero-rust/dart/monero_extension/native/monero-wasm
wasm-pack test --headless --chrome
```

## License
See individual component LICENSE files.  monero-rust, monero-wasm, and
monero_extension are licensed under the MIT License.

## References
- Monero Project: https://getmonero.org
- monero-serai: https://github.com/serai-dex/serai
- wasm-pack: https://rustwasm.github.io/wasm-pack/
- rinf: https://rinf.cunarist.com/
