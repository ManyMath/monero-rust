//! Minimal logger that routes `log` crate macros to the platform console.
//!
//! - WASM: `web_sys::console::log_1` / `warn_1` / `error_1`
//! - Native: `eprintln!`

use log::{Level, LevelFilter, Log, Metadata, Record};

struct ConsoleLogger;

impl Log for ConsoleLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let msg = format!(
            "[{}] {} — {}",
            record.level(),
            record.target(),
            record.args()
        );
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsValue;
            match record.level() {
                Level::Error => web_sys::console::error_1(&JsValue::from_str(&msg)),
                Level::Warn => web_sys::console::warn_1(&JsValue::from_str(&msg)),
                _ => web_sys::console::log_1(&JsValue::from_str(&msg)),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!("{}", msg);
        }
    }

    fn flush(&self) {}
}

static LOGGER: ConsoleLogger = ConsoleLogger;

/// Initialize the logger. Safe to call more than once (subsequent calls are no-ops).
pub fn init() {
    // `set_logger` returns Err if already set — that's fine.
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(LevelFilter::Debug);
}
