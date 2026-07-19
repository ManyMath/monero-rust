#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod local_stagenet_tests {
    use monero_rust::monero_backend::rpc::HttpRpc;
    use monero_rust::scanner::{derive_address, scan_block_for_outputs};

    const LOCAL_NODE: &str = "http://127.0.0.1:38081";
    const TEST_BLOCK: u64 = 1384526;
    const HONKED_BAGPIPE: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
    const EXPECTED_TX: &str = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";
    const EXPECTED_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";

    /// Soft-skip when no local stagenet node is reachable, matching the
    /// describeIfNode convention used by the E2E suite.
    async fn node_height(rpc: &monero_rust::monero_backend::rpc::Rpc<HttpRpc>) -> Option<usize> {
        match rpc.get_height().await {
            Ok(height) => Some(height),
            Err(e) => {
                eprintln!("Skipped: no local node at {} ({:?})", LOCAL_NODE, e);
                None
            }
        }
    }

    #[tokio::test]
    async fn test_local_node_connectivity() {
        let rpc = HttpRpc::new(LOCAL_NODE.to_string()).unwrap();
        let Some(height) = node_height(&rpc).await else {
            return;
        };
        assert!(height > TEST_BLOCK as usize);
    }

    #[tokio::test]
    async fn test_fetch_test_block() {
        let rpc = HttpRpc::new(LOCAL_NODE.to_string()).unwrap();
        if node_height(&rpc).await.is_none() {
            return;
        }
        let block = rpc.get_block_by_number(TEST_BLOCK as usize).await.unwrap();

        let expected_tx_hash = hex::decode(EXPECTED_TX).unwrap();
        let mut expected = [0u8; 32];
        expected.copy_from_slice(&expected_tx_hash);
        assert!(block.transactions.contains(&expected));
    }

    #[tokio::test]
    async fn test_scan_block_native() {
        let rpc = HttpRpc::new(LOCAL_NODE.to_string()).unwrap();
        if node_height(&rpc).await.is_none() {
            return;
        }

        let result = scan_block_for_outputs(&rpc, TEST_BLOCK, HONKED_BAGPIPE, "stagenet", "")
            .await
            .expect("scan failed");

        assert_eq!(result.outputs.len(), 1);
        let output = &result.outputs[0];
        assert_eq!(output.tx_hash, EXPECTED_TX);
        assert_eq!(output.amount, 10_000_000_000_000);
    }

    #[tokio::test]
    async fn test_wallet_address() {
        let address = derive_address(HONKED_BAGPIPE, "stagenet", "").unwrap();
        assert_eq!(address, EXPECTED_ADDRESS);
    }
}
