#![cfg(not(target_arch = "wasm32"))]

use monero_serai::rpc::HttpRpc;

const LOCAL_NODE: &str = "http://127.0.0.1:38081";

#[tokio::test]
async fn debug_get_block() {
    let rpc = HttpRpc::new(LOCAL_NODE.to_string()).unwrap();

    // Soft-skip when no local stagenet node is reachable, matching the
    // describeIfNode convention used by the E2E suite.
    let height = match rpc.get_height().await {
        Ok(height) => height,
        Err(e) => {
            eprintln!("Skipped: no local node at {} ({:?})", LOCAL_NODE, e);
            return;
        }
    };
    println!("height: {}", height);

    let hash = rpc.get_block_hash(1).await.expect("get_block_hash failed");
    println!("block 1 hash: {}", hex::encode(hash));

    if let Ok(block) = rpc.get_block(hash).await {
        println!("block timestamp: {}", block.header.timestamp);
    }

    if let Ok(block) = rpc.get_block_by_number(1).await {
        println!("block by number timestamp: {}", block.header.timestamp);
    }
}
