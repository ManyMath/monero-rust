#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
use monero_wasm::rpc_adapter::WasmRpcAdapter;
use monero_serai::rpc::Rpc;

wasm_bindgen_test_configure!(run_in_browser);

const PUBLIC_STAGENET_NODES: &[&str] = &[
    "http://stagenet.xmr-tw.org:38081",
    "http://node.sethforprivacy.com:38089",
    "http://node2.sethforprivacy.com:38089",
    "http://singapore.node.xmr.pm:38081",
];

#[wasm_bindgen_test]
async fn test_public_stagenet_nodes_connectivity() {
    let mut found_working = false;

    for node_url in PUBLIC_STAGENET_NODES {
        let adapter = WasmRpcAdapter::new(node_url.to_string());
        let rpc = Rpc::new_with_connection(adapter);

        if rpc.get_height().await.is_ok() {
            found_working = true;
            break;
        }
    }

    assert!(found_working, "No public stagenet nodes accessible");
}

#[wasm_bindgen_test]
async fn test_fetch_block_hash_from_public_nodes() {
    let mut found_working = false;

    for node_url in PUBLIC_STAGENET_NODES {
        let adapter = WasmRpcAdapter::new(node_url.to_string());
        let rpc = Rpc::new_with_connection(adapter);

        if rpc.get_height().await.is_err() {
            continue;
        }

        if rpc.get_block_hash(1000).await.is_ok() {
            found_working = true;
            break;
        }
    }

    assert!(found_working, "Could not fetch block hash from any public node");
}

#[wasm_bindgen_test]
async fn test_single_node_detailed() {
    let node_url = PUBLIC_STAGENET_NODES[0];
    let adapter = WasmRpcAdapter::new(node_url.to_string());
    let rpc = Rpc::new_with_connection(adapter);

    let _ = rpc.get_height().await;
    let _ = rpc.get_block_hash(1).await;
    let _ = rpc.get_block_hash(1000).await;
}

#[wasm_bindgen_test]
async fn test_public_node_for_scanning() {
    let mut working_node = None;

    for node_url in PUBLIC_STAGENET_NODES {
        let adapter = WasmRpcAdapter::new(node_url.to_string());
        let rpc = Rpc::new_with_connection(adapter);

        if let Ok(height) = rpc.get_height().await {
            working_node = Some((node_url, height));
            break;
        }
    }

    let (node_url, height) = working_node.expect("No working stagenet node");
    let adapter = WasmRpcAdapter::new(node_url.to_string());
    let rpc = Rpc::new_with_connection(adapter);

    let test_height = if height > 1000 { 1000 } else { 1 };
    rpc.get_block_hash(test_height).await.expect("Failed to get block hash");
}

#[wasm_bindgen_test]
async fn test_cors_configuration() {
    for node_url in PUBLIC_STAGENET_NODES {
        let adapter = WasmRpcAdapter::new(node_url.to_string());
        let rpc = Rpc::new_with_connection(adapter);

        if rpc.get_height().await.is_ok() {
            return;
        }
    }

    panic!("All nodes failed CORS test");
}
