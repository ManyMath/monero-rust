#![cfg(not(target_arch = "wasm32"))]

use monero_rust::monero_rpc::HttpRpc;
use monero_rust::scanner::scan_block_for_outputs;
use monero_rust::Network;

const LOCAL_NODE: &str = "http://127.0.0.1:38081";

#[tokio::test]
async fn test_scan_invalid_inputs() {
    let rpc = HttpRpc::new(LOCAL_NODE.to_string()).expect("Failed to create RPC");

    let result = scan_block_for_outputs(&rpc, 1, "invalid seed words", Network::Stagenet, "").await;
    assert!(result.is_err());
}
