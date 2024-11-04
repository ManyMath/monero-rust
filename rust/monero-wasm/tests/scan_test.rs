#![cfg(not(target_arch = "wasm32"))]

use monero_serai::rpc::HttpRpc;
use monero_wasm::scanner::scan_block_for_outputs;

const HONKED_BAGPIPE_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const LOCAL_NODE: &str = "http://127.0.0.1:38081";
const TEST_BLOCK: u64 = 1384526;

#[tokio::test]
async fn test_scan_mainnet_block() {
    let rpc = HttpRpc::new(LOCAL_NODE.to_string())
        .expect("Failed to create RPC");

    let result = scan_block_for_outputs(&rpc, TEST_BLOCK, HONKED_BAGPIPE_SEED, "stagenet")
        .await
        .expect("scan failed");

    assert_eq!(result.outputs.len(), 1);
    assert!(result.tx_count > 0);
}

#[tokio::test]
async fn test_scan_block_no_outputs() {
    let rpc = HttpRpc::new(LOCAL_NODE.to_string())
        .expect("Failed to create RPC");

    let result = scan_block_for_outputs(&rpc, 2000000, HONKED_BAGPIPE_SEED, "stagenet")
        .await
        .expect("scan failed");

    assert!(result.tx_count > 0);
}

#[tokio::test]
async fn test_rpc_connectivity() {
    let rpc = HttpRpc::new(LOCAL_NODE.to_string())
        .expect("Failed to create RPC");

    let height = rpc.get_height().await.expect("get_height failed");
    assert!(height > 0);

    let hash = rpc.get_block_hash(TEST_BLOCK as usize).await.expect("get_block_hash failed");
    assert_eq!(hash.len(), 32);

    let block = rpc.get_block_by_number(TEST_BLOCK as usize).await.expect("get_block failed");
    assert!(block.header.timestamp > 0);
}

#[tokio::test]
async fn test_scan_invalid_inputs() {
    let rpc = HttpRpc::new(LOCAL_NODE.to_string())
        .expect("Failed to create RPC");

    let result = scan_block_for_outputs(&rpc, 1, "invalid seed words", "stagenet").await;
    assert!(result.is_err());

    let result = scan_block_for_outputs(&rpc, 1, HONKED_BAGPIPE_SEED, "invalidnet").await;
    assert!(result.is_err());
}
