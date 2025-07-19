//! WASM entry point for the Monero wallet extension.

mod actors;
pub mod ffi_web;
mod messages;
mod signals;

#[cfg(target_arch = "wasm32")]
pub mod test_api;
