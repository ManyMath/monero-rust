#![cfg(not(target_arch = "wasm32"))]

use monero_rust::monero_backend::rpc::HttpRpc;
use monero_rust::scanner::scan_block_for_outputs;

const HONKED_BAGPIPE_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const LOCAL_NODE: &str = "http://127.0.0.1:38081";

#[tokio::test]
async fn test_scan_invalid_inputs() {
    let rpc = HttpRpc::new(LOCAL_NODE.to_string()).expect("Failed to create RPC");

    let result = scan_block_for_outputs(&rpc, 1, "invalid seed words", "stagenet", "").await;
    assert!(result.is_err());

    let result = scan_block_for_outputs(&rpc, 1, HONKED_BAGPIPE_SEED, "invalidnet", "").await;
    assert!(result.is_err());
}
