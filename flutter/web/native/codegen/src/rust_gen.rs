use crate::dart_gen::pascal_to_snake;
use crate::model::{Direction, SignalClass};

// ---------------------------------------------------------------------------
// ffi_web.rs generation
// ---------------------------------------------------------------------------

pub fn generate_ffi_web(signals: &[SignalClass]) -> String {
    let dart2rust: Vec<&SignalClass> = signals
        .iter()
        .filter(|s| s.direction == Direction::Dart2Rust)
        .collect();
    let rust2dart: Vec<&SignalClass> = signals
        .iter()
        .filter(|s| s.direction == Direction::Rust2Dart)
        .collect();

    let mut out = String::new();

    out.push_str(FFI_WEB_PREAMBLE);
    out.push('\n');

    // Comment header for the dart_signal! block
    out.push_str(&format!(
        "// All {} DartSignal types\n",
        dart2rust.len()
    ));
    for sig in &dart2rust {
        let snake = pascal_to_snake(&sig.name);
        out.push_str(&format!(
            "dart_signal!({}, {});\n",
            sig.name, snake
        ));
    }

    out.push('\n');
    out.push_str(FFI_WEB_CALLBACK_SECTION);
    out.push('\n');

    out.push_str("impl_send_to_dart! {\n");
    for (i, sig) in rust2dart.iter().enumerate() {
        if i < rust2dart.len() - 1 {
            out.push_str(&format!(
                "    {} => \"{}\",\n",
                sig.name, sig.name
            ));
        } else {
            out.push_str(&format!(
                "    {} => \"{}\",\n",
                sig.name, sig.name
            ));
        }
    }
    out.push_str("}\n");

    out.push('\n');
    out.push_str(FFI_WEB_ENTRY_POINT);

    out
}

// ---------------------------------------------------------------------------
// signal_ids.rs generation
// ---------------------------------------------------------------------------

pub fn generate_signal_ids(signals: &[SignalClass]) -> String {
    let dart2rust: Vec<&SignalClass> = signals
        .iter()
        .filter(|s| s.direction == Direction::Dart2Rust)
        .collect();
    let rust2dart: Vec<&SignalClass> = signals
        .iter()
        .filter(|s| s.direction == Direction::Rust2Dart)
        .collect();

    let mut out = String::new();
    out.push_str("/// Numeric signal IDs shared between Rust and Dart.\n");
    out.push_str(
        "/// DartSignal = Dart->Rust (requests), RustSignal = Rust->Dart (responses).\n",
    );
    out.push_str("///\n");
    out.push_str(
        "/// IMPORTANT: These IDs must stay in sync with `hub_signal_ids.dart` on the Dart side.\n",
    );
    out.push('\n');

    out.push_str("// -- DartSignal IDs (Dart -> Rust) --\n");
    for (i, sig) in dart2rust.iter().enumerate() {
        let const_name = pascal_to_screaming_snake(&sig.name);
        out.push_str(&format!(
            "pub const {}: u32 = {};\n",
            const_name,
            i + 1
        ));
    }

    out.push('\n');
    out.push_str("// -- RustSignal IDs (Rust -> Dart) --\n");
    for (i, sig) in rust2dart.iter().enumerate() {
        let const_name = pascal_to_screaming_snake(&sig.name);
        out.push_str(&format!(
            "pub const {}: u32 = {};\n",
            const_name,
            101 + i
        ));
    }

    out.push('\n');
    out.push_str(SIGNAL_IDS_SUFFIX);

    out
}

// ---------------------------------------------------------------------------
// hub_signal_ids.dart generation
// ---------------------------------------------------------------------------

pub fn generate_hub_signal_ids(signals: &[SignalClass]) -> String {
    let dart2rust: Vec<&SignalClass> = signals
        .iter()
        .filter(|s| s.direction == Direction::Dart2Rust)
        .collect();
    let rust2dart: Vec<&SignalClass> = signals
        .iter()
        .filter(|s| s.direction == Direction::Rust2Dart)
        .collect();

    let mut out = String::new();
    out.push_str("/// Numeric signal IDs shared between Rust and Dart.\n");
    out.push_str("///\n");
    out.push_str(
        "/// IMPORTANT: Must stay in sync with `signal_ids.rs` on the Rust side.\n",
    );
    out.push_str("class HubSignalIds {\n");
    out.push_str("  HubSignalIds._();\n");
    out.push('\n');

    out.push_str("  // -- DartSignal IDs (Dart -> Rust) --\n");
    for (i, sig) in dart2rust.iter().enumerate() {
        let camel = pascal_to_lower_camel(&sig.name);
        out.push_str(&format!(
            "  static const int {} = {};\n",
            camel,
            i + 1
        ));
    }

    out.push('\n');
    out.push_str("  // -- RustSignal IDs (Rust -> Dart) --\n");
    for (i, sig) in rust2dart.iter().enumerate() {
        let camel = pascal_to_lower_camel(&sig.name);
        out.push_str(&format!(
            "  static const int {} = {};\n",
            camel,
            101 + i
        ));
    }

    out.push_str("}\n");
    out
}

// ---------------------------------------------------------------------------
// Name conversion helpers
// ---------------------------------------------------------------------------

/// PascalCase → SCREAMING_SNAKE_CASE
fn pascal_to_screaming_snake(s: &str) -> String {
    pascal_to_snake(s).to_uppercase()
}

/// PascalCase → lowerCamelCase (first letter lowercase)
fn pascal_to_lower_camel(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Verbatim template sections
// ---------------------------------------------------------------------------

const FFI_WEB_PREAMBLE: &str = r#"use std::cell::RefCell;
use wasm_bindgen::prelude::*;

use crate::signals::*;

// ---------------------------------------------------------------------------
// Macro: generate DartSignal channel infrastructure
// ---------------------------------------------------------------------------
//
// For each DartSignal type, generates:
//   - A thread_local UnboundedSender
//   - A #[wasm_bindgen] export `send_<snake_name>(json)` that deserializes and sends
//   - A `get_<snake_name>_receiver()` function for actors to call during init

macro_rules! dart_signal {
    ($type:ty, $snake:ident) => {
        paste::paste! {
            thread_local! {
                static [<$snake:upper _SENDER>]: RefCell<Option<tokio::sync::mpsc::UnboundedSender<$type>>>
                    = RefCell::new(None);
            }

            #[wasm_bindgen]
            pub fn [<send_ $snake>](json: &str) -> Result<(), JsValue> {
                let msg: $type = serde_json::from_str(json)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                [<$snake:upper _SENDER>].with(|s| {
                    if let Some(tx) = s.borrow().as_ref() {
                        tx.send(msg).map_err(|_| JsValue::from_str("channel closed"))
                    } else {
                        Err(JsValue::from_str("not initialized"))
                    }
                })
            }

            pub fn [<get_ $snake _receiver>]() -> tokio::sync::mpsc::UnboundedReceiver<$type> {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                [<$snake:upper _SENDER>].with(|s| { *s.borrow_mut() = Some(tx); });
                rx
            }
        }
    };
}
"#;

const FFI_WEB_CALLBACK_SECTION: &str = r#"// ---------------------------------------------------------------------------
// Rust -> Dart callback
// ---------------------------------------------------------------------------

thread_local! {
    static DART_CALLBACK: RefCell<Option<js_sys::Function>> = RefCell::new(None);
}

#[wasm_bindgen]
pub fn register_rust_signal_callback(callback: js_sys::Function) {
    DART_CALLBACK.with(|c| { *c.borrow_mut() = Some(callback); });
}

fn send_to_dart_raw<T: serde::Serialize + ?Sized>(type_name: &str, msg: &T) {
    DART_CALLBACK.with(|c| {
        if let Some(cb) = c.borrow().as_ref() {
            match serde_json::to_string(msg) {
                Ok(json) => {
                    let _ = cb.call2(
                        &JsValue::NULL,
                        &JsValue::from_str(type_name),
                        &JsValue::from_str(&json),
                    );
                }
                Err(e) => {
                    web_sys::console::error_1(
                        &JsValue::from_str(&format!("serialize error for {}: {}", type_name, e))
                    );
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// SendToDart trait — replaces rinf's .send_signal_to_dart()
// ---------------------------------------------------------------------------

pub trait SendToDart: serde::Serialize {
    const TYPE_NAME: &'static str;

    fn send_signal_to_dart(&self) {
        send_to_dart_raw(Self::TYPE_NAME, self);
    }
}

macro_rules! impl_send_to_dart {
    ($($type:ty => $name:expr),* $(,)?) => {
        $(
            impl SendToDart for $type {
                const TYPE_NAME: &'static str = $name;
            }
        )*
    };
}
"#;

const FFI_WEB_ENTRY_POINT: &str = r#"// ---------------------------------------------------------------------------
// WASM entry point
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub async fn start_rust_runtime() {
    tokio_with_wasm::alias::spawn(crate::actors::create_actors());
}
"#;

const SIGNAL_IDS_SUFFIX: &str = r#"/// Route an incoming DartSignal to the appropriate handler.
///
/// This is the native-FFI equivalent of the wasm-bindgen signal routing.
/// For now this is a stub — full routing would deserialize each signal
/// via JSON and dispatch to the actor mailboxes.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn route_dart_signal(_signal_id: u32, _data: Vec<u8>) {
    // TODO: match on signal_id, JSON-deserialize, and send to actors.
    // Example:
    // match signal_id {
    //     MONERO_TEST_REQUEST => { ... }
    //     CREATE_WALLET_REQUEST => { ... }
    //     _ => { eprintln!("unknown signal_id: {signal_id}"); }
    // }
}
"#;
