# Rust crates

- **`monero_wasm`** — the wallet crate the app runs. It builds to wasm32 for the browser
  (loaded by the Web Worker) and to a cdylib/staticlib for native, where `ffi_native.rs`
  exposes a C surface. Rinf is gone; the Dart side talks to it through hand-written
  bindings in `flutter/web/lib/src/ffi/`.
- **`codegen`** — generates the signal surface. It is not a dependency of anything; it is
  run by hand and by `scripts/test-all.sh`.

## The signal surface is generated

`monero_wasm/src/signals/mod.rs` is the source of truth. Every signal struct there is a
message crossing the Dart/Rust boundary, and `codegen` derives three files from it:

| generated file | what it is |
|---|---|
| `flutter/web/lib/src/ffi/signal_types.dart` | the Dart classes, with `sendSignalToRust()` and `stream` |
| `monero_wasm/src/ffi_web.rs` | the `dart_signal!` channels and the wasm-bindgen exports |
| `monero_wasm/src/signal_ids.rs` | numeric IDs and the native FFI routing |

```sh
cd flutter/web/native/codegen
cargo run              # regenerate
cargo run -- --check   # fail if the committed copies are stale
```

**Edit `signals/mod.rs` and regenerate — never edit the three outputs by hand.** They were
hand-edited for six months while nothing ran the check, and by the end running the
generator would have reverted shipping code. `test-all.sh` runs `--check` now so that
cannot happen quietly again; the tool needs `dart` and `rustfmt` on PATH, because it
formats its output before comparing.

Direction is inferred from the struct: `Serialize` + a `Response`/`Update` suffix is
Rust→Dart, `Deserialize` + a `Request` suffix is Dart→Rust, and anything else is a shared
sub-struct. `TxSet` is deliberately one word in snake_case (`extract_signed_txset_request`),
matching the JSON keys and the export names the actors call.
