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
cargo test             # the generator's own tests
```

`cargo test` is a separate invocation because this crate is not in `rust/`'s workspace, so
`cargo test --workspace` there never reaches it. `scripts/test-all.sh` runs both.

### What the generator refuses

Dart's `fromJson` casts a required field straight out of the map — `json['outputs'] as List`
— so a key that isn't there throws, in a Web Worker, when the signal arrives. That
strictness is deliberate: a response missing a field is a bug and defaulting it silently
would bury it. It holds only while Rust never omits a field, so the generator enforces that
directly and rejects `#[serde(skip)]`, `#[serde(skip_serializing)]` and
`#[serde(skip_serializing_if = ...)]` on anything Dart deserialises.

That covers the `Rust2Dart` signals and every sub-struct reachable from one. A shared struct
nested only inside a `Request` travels the other way — Dart writes it, Rust reads it — so a
skip there is harmless and allowed. `Option` fields are allowed everywhere, since Dart reads
them as nullable; making the field `Option<T>` is the fix the error message suggests.

**Edit `signals/mod.rs` and regenerate — never edit the three outputs by hand.** They were
hand-edited for six months while nothing ran the check, and by the end running the
generator would have reverted shipping code. `test-all.sh` runs `--check` now so that
cannot happen quietly again; the tool needs `dart` and `rustfmt` on PATH, because it
formats its output before comparing.

Direction is inferred from the struct: `Serialize` + a `Response`/`Update` suffix is
Rust→Dart, `Deserialize` + a `Request` suffix is Dart→Rust, and anything else is a shared
sub-struct. `TxSet` is deliberately one word in snake_case (`extract_signed_txset_request`),
matching the JSON keys and the export names the actors call.
