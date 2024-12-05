//! This `hub` crate is the
//! entry point of the Rust logic.

mod actors;
mod signals;
mod messages;
mod encryption;

use actors::create_actors;
use rinf::{dart_shutdown, write_interface};
use tokio::spawn;

use tokio_with_wasm::alias as tokio;

write_interface!();

#[tokio::main(flavor = "current_thread")]
async fn main() {
    spawn(create_actors());
    dart_shutdown().await;
}
