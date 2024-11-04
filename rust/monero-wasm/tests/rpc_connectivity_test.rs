#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod rpc_tests {
    use monero_serai::rpc::{HttpRpc, Rpc};

    const STAGENET_NODES: &[&str] = &[
        "http://127.0.0.1:38081",
        "http://monero-stagenet.exan.tech:38081",
        "http://stagenet.community.rino.io:38081",
        "http://18.133.55.120:38081",
    ];

    const MAINNET_NODES: &[&str] = &[
        "http://127.0.0.1:18081",
        "http://node.moneroworld.com:18089",
        "http://node.community.rino.io:18081",
        "http://xmr-node.cakewallet.com:18081",
    ];

    #[tokio::test]
    async fn test_mainnet_connectivity() {
        let mut working_node: Option<String> = None;

        for node_url in MAINNET_NODES {
            if let Ok(rpc) = HttpRpc::new(node_url.to_string()) {
                if let Ok(height) = rpc.get_height().await {
                    println!("{} height: {}", node_url, height);
                    working_node = Some(node_url.to_string());
                    break;
                }
            }
        }

        assert!(working_node.is_some(), "No working mainnet nodes found");
    }

    #[tokio::test]
    #[ignore]
    async fn test_stagenet_connectivity() {
        for node_url in STAGENET_NODES {
            if let Ok(rpc) = HttpRpc::new(node_url.to_string()) {
                if let Ok(height) = rpc.get_height().await {
                    println!("{} height: {}", node_url, height);
                    return;
                }
            }
        }
        println!("No stagenet nodes reachable - run local node with: monerod --stagenet");
    }

    async fn get_working_stagenet_node() -> Option<(String, Rpc<HttpRpc>)> {
        for node_url in STAGENET_NODES {
            if let Ok(rpc) = HttpRpc::new(node_url.to_string()) {
                if rpc.get_height().await.is_ok() {
                    return Some((node_url.to_string(), rpc));
                }
            }
        }
        None
    }

    async fn get_working_mainnet_node() -> Option<(String, Rpc<HttpRpc>)> {
        for node_url in MAINNET_NODES {
            if let Ok(rpc) = HttpRpc::new(node_url.to_string()) {
                if rpc.get_height().await.is_ok() {
                    return Some((node_url.to_string(), rpc));
                }
            }
        }
        None
    }

    #[tokio::test]
    #[ignore]
    async fn test_fetch_stagenet_block_1384526() {
        let target_height = 1384526;

        let (node_url, rpc) = get_working_stagenet_node()
            .await
            .expect("No working stagenet node");

        println!("Using {}", node_url);

        let block_hash_bytes = rpc.get_block_hash(target_height).await.unwrap();
        let block_hash = hex::encode(block_hash_bytes);
        println!("Block hash: {}", block_hash);

        let block = rpc.get_block_by_number(target_height).await.unwrap();
        println!(
            "Timestamp: {}, txs: {}",
            block.header.timestamp,
            block.txs.len() + 1
        );

        if !block.txs.is_empty() {
            let transactions = rpc.get_transactions(&block.txs).await.unwrap();
            assert_eq!(transactions.len(), block.txs.len());
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_fetch_stagenet_specific_transaction() {
        let tx_hex = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";

        let (_node_url, rpc) = get_working_stagenet_node()
            .await
            .expect("No working stagenet node");

        let tx_hash_bytes = hex::decode(tx_hex).unwrap();
        let mut tx_hash = [0u8; 32];
        tx_hash.copy_from_slice(&tx_hash_bytes);

        let transactions = rpc.get_transactions(&[tx_hash]).await.unwrap();
        assert_eq!(transactions.len(), 1);

        let tx = &transactions[0];
        assert_eq!(hex::encode(tx.hash()), tx_hex);
        println!(
            "inputs: {}, outputs: {}",
            tx.prefix.inputs.len(),
            tx.prefix.outputs.len()
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_comprehensive_stagenet_rpc_check() {
        let (node_url, rpc) = get_working_stagenet_node()
            .await
            .expect("No working stagenet node");

        println!("Node: {}", node_url);

        let current_height = rpc.get_height().await.unwrap();
        println!("Height: {}", current_height);

        let target_height = 1384526;
        let _block_hash_bytes = rpc.get_block_hash(target_height).await.unwrap();
        let block = rpc.get_block_by_number(target_height).await.unwrap();
        println!("Block {}: {} txs", target_height, block.txs.len() + 1);

        let tx_hex = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";
        let tx_hash_bytes = hex::decode(tx_hex).unwrap();
        let mut tx_hash = [0u8; 32];
        tx_hash.copy_from_slice(&tx_hash_bytes);

        let transactions = rpc.get_transactions(&[tx_hash]).await.unwrap();
        assert_eq!(transactions.len(), 1);
    }

    #[tokio::test]
    async fn test_mainnet_rpc_comprehensive() {
        let (node_url, rpc) = get_working_mainnet_node()
            .await
            .expect("No working mainnet node");

        println!("Node: {}", node_url);

        let current_height = rpc.get_height().await.unwrap();
        println!("Height: {}", current_height);

        let target_height = current_height.saturating_sub(100).max(1);
        let block_hash_bytes = rpc.get_block_hash(target_height).await.unwrap();
        let block_hash = hex::encode(&block_hash_bytes);

        let block = rpc.get_block_by_number(target_height).await.unwrap();
        println!(
            "Block {}: hash={}, txs={}",
            target_height,
            &block_hash[..16],
            block.txs.len() + 1
        );

        if !block.txs.is_empty() {
            let limit = block.txs.len().min(3);
            let transactions = rpc.get_transactions(&block.txs[..limit]).await.unwrap();
            println!("Fetched {} txs", transactions.len());
        }
    }
}
