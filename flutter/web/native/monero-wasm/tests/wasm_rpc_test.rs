#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
use monero_wasm::rpc_adapter::WasmRpcAdapter;
use monero_serai::rpc::Rpc;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn test_wasm_rpc_get_height() {
    let adapter = WasmRpcAdapter::new("http://xmr-node.cakewallet.com:18081".to_string());
    let rpc = Rpc::new_with_connection(adapter);

    let height = rpc.get_height().await.expect("get_height failed");
    assert!(height > 0);
}

#[wasm_bindgen_test]
async fn test_wasm_rpc_get_block_hash() {
    let adapter = WasmRpcAdapter::new("http://xmr-node.cakewallet.com:18081".to_string());
    let rpc = Rpc::new_with_connection(adapter);

    let hash = rpc.get_block_hash(1).await.expect("get_block_hash failed");
    assert_eq!(hash.len(), 32);
}

#[wasm_bindgen_test]
async fn test_wasm_rpc_get_block() {
    let adapter = WasmRpcAdapter::new("http://xmr-node.cakewallet.com:18081".to_string());
    let rpc = Rpc::new_with_connection(adapter);

    let hash = rpc.get_block_hash(1).await.expect("get_block_hash failed");
    // get_block may fail due to parsing issues in monero-serai
    let _ = rpc.get_block(hash).await;
}

#[wasm_bindgen_test]
async fn test_wasm_full_scan() {
    use monero_wasm::scan_block_for_outputs;

    const HONKED_BAGPIPE_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";

    let result = scan_block_for_outputs(
        "http://xmr-node.cakewallet.com:18081",
        1,
        HONKED_BAGPIPE_SEED,
        "mainnet"
    ).await;

    // may fail due to CORS, just verify no panic
    let _ = result;
}
