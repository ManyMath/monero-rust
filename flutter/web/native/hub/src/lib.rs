//! This `hub` crate is the
//! entry point of the Rust logic.

mod actors;
pub mod ffi_web;
mod messages;
mod signals;

#[cfg(target_arch = "wasm32")]
pub mod test_api;
