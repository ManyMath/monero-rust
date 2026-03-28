//! WASM entry point for the Monero wallet extension.

mod actors;
pub mod ffi_web;
pub(crate) mod logging;
mod messages;
#[allow(dead_code)]
pub(crate) mod signal_ids;
mod signals;

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi_native;

#[cfg(target_arch = "wasm32")]
pub mod test_api;
