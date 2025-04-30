//! Multi-wallet scanning tests
//!
//! This module tests the parallel multi-wallet scanning functionality.

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use monero_rust::{WalletScanConfig, Lookahead, DEFAULT_LOOKAHEAD};

    const TEST_WALLET_1_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
    const TEST_WALLET_1_ADDRESS: &str = "45wsWad9EwZgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU8K2Dhi";

    const TEST_WALLET_2_SEED: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";
    const TEST_WALLET_2_ADDRESS: &str = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

    const TEST_WALLET_3_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
    const TEST_WALLET_3_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";

    #[test]
    fn test_wallet_scan_config_creation() {
        let config = WalletScanConfig {
            mnemonic: TEST_WALLET_1_SEED.to_string(),
            network: "mainnet".to_string(),
            lookahead: DEFAULT_LOOKAHEAD,
        };

        assert_eq!(config.mnemonic, TEST_WALLET_1_SEED);
        assert_eq!(config.network, "mainnet");
        assert_eq!(config.lookahead.account, 0);
        assert_eq!(config.lookahead.subaddress, 20);
    }

    #[test]
    fn test_multiple_wallet_configs() {
        let configs = vec![
            WalletScanConfig {
                mnemonic: TEST_WALLET_1_SEED.to_string(),
                network: "mainnet".to_string(),
                lookahead: DEFAULT_LOOKAHEAD,
            },
            WalletScanConfig {
                mnemonic: TEST_WALLET_2_SEED.to_string(),
                network: "stagenet".to_string(),
                lookahead: DEFAULT_LOOKAHEAD,
            },
            WalletScanConfig {
                mnemonic: TEST_WALLET_3_SEED.to_string(),
                network: "stagenet".to_string(),
                lookahead: Lookahead {
                    account: 0,
                    subaddress: 10,
                },
            },
        ];

        assert_eq!(configs.len(), 3);
        assert_eq!(configs[0].network, "mainnet");
        assert_eq!(configs[1].network, "stagenet");
        assert_eq!(configs[2].lookahead.subaddress, 10);
    }

    // Note: Integration tests that actually scan blocks would require:
    // 1. A running Monero daemon (or mock RPC server)
    // 2. Known blocks with transactions to these test wallets
    //
    // Example test structure (requires actual daemon):
    //
    // #[tokio::test]
    // async fn test_multi_wallet_scan_integration() {
    //     use monero_rust::scan_block_multi_wallet_with_url;
    //
    //     let node_url = "http://localhost:18081";
    //     let block_height = 1000000; // Block known to have txs
    //
    //     let configs = vec![
    //         WalletScanConfig {
    //             mnemonic: TEST_WALLET_1_SEED.to_string(),
    //             network: "mainnet".to_string(),
    //             lookahead: DEFAULT_LOOKAHEAD,
    //         },
    //         WalletScanConfig {
    //             mnemonic: TEST_WALLET_2_SEED.to_string(),
    //             network: "mainnet".to_string(),
    //             lookahead: DEFAULT_LOOKAHEAD,
    //         },
    //     ];
    //
    //     let result = scan_block_multi_wallet_with_url(
    //         node_url,
    //         block_height,
    //         configs,
    //     )
    //     .await;
    //
    //     assert!(result.is_ok());
    //     let scan_result = result.unwrap();
    //     assert_eq!(scan_result.block_height, block_height);
    //     assert_eq!(scan_result.wallet_results.len(), 2);
    // }

    // --- Seed parsing and address derivation tests ---

    use monero_rust::scanner::derive_address;

    #[test]
    fn test_derive_address_consistent_with_keys() {
        // Wallet 1 is mainnet
        let addr1 = derive_address(TEST_WALLET_1_SEED, "mainnet")
            .expect("derive_address failed for wallet 1");
        assert_eq!(addr1, TEST_WALLET_1_ADDRESS, "Wallet 1 address mismatch");

        // Wallet 2 is stagenet
        let addr2 = derive_address(TEST_WALLET_2_SEED, "stagenet")
            .expect("derive_address failed for wallet 2");
        assert_eq!(addr2, TEST_WALLET_2_ADDRESS, "Wallet 2 address mismatch");

        // Wallet 3 is stagenet
        let addr3 = derive_address(TEST_WALLET_3_SEED, "stagenet")
            .expect("derive_address failed for wallet 3");
        assert_eq!(addr3, TEST_WALLET_3_ADDRESS, "Wallet 3 address mismatch");
    }

    #[test]
    fn test_derive_address_called_twice_same_result() {
        let first = derive_address(TEST_WALLET_1_SEED, "mainnet")
            .expect("first derive_address call failed");
        let second = derive_address(TEST_WALLET_1_SEED, "mainnet")
            .expect("second derive_address call failed");
        assert_eq!(first, second, "Two calls to derive_address must return the same result");
    }

    #[test]
    fn test_wallet_scan_config_clone_preserves_data() {
        let original = WalletScanConfig {
            mnemonic: TEST_WALLET_1_SEED.to_string(),
            network: "mainnet".to_string(),
            lookahead: Lookahead {
                account: 2,
                subaddress: 50,
            },
        };

        let cloned = original.clone();

        assert_eq!(cloned.mnemonic, original.mnemonic);
        assert_eq!(cloned.network, original.network);
        assert_eq!(cloned.lookahead.account, original.lookahead.account);
        assert_eq!(cloned.lookahead.subaddress, original.lookahead.subaddress);
    }

    #[test]
    fn test_different_seeds_different_addresses() {
        let addr1 = derive_address(TEST_WALLET_1_SEED, "mainnet")
            .expect("derive_address failed for wallet 1");
        let addr2 = derive_address(TEST_WALLET_2_SEED, "stagenet")
            .expect("derive_address failed for wallet 2");
        let addr3 = derive_address(TEST_WALLET_3_SEED, "stagenet")
            .expect("derive_address failed for wallet 3");

        assert_ne!(addr1, addr2, "Wallet 1 and 2 should have different addresses");
        assert_ne!(addr1, addr3, "Wallet 1 and 3 should have different addresses");
        assert_ne!(addr2, addr3, "Wallet 2 and 3 should have different addresses");
    }

    #[test]
    fn test_multi_wallet_config_ordering() {
        let configs = vec![
            WalletScanConfig {
                mnemonic: TEST_WALLET_1_SEED.to_string(),
                network: "mainnet".to_string(),
                lookahead: DEFAULT_LOOKAHEAD,
            },
            WalletScanConfig {
                mnemonic: TEST_WALLET_2_SEED.to_string(),
                network: "stagenet".to_string(),
                lookahead: DEFAULT_LOOKAHEAD,
            },
            WalletScanConfig {
                mnemonic: TEST_WALLET_3_SEED.to_string(),
                network: "stagenet".to_string(),
                lookahead: Lookahead {
                    account: 0,
                    subaddress: 10,
                },
            },
        ];

        assert_eq!(configs.len(), 3);
        assert_eq!(configs[0].mnemonic, TEST_WALLET_1_SEED);
        assert_eq!(configs[1].mnemonic, TEST_WALLET_2_SEED);
        assert_eq!(configs[2].mnemonic, TEST_WALLET_3_SEED);
        assert_eq!(configs[0].network, "mainnet");
        assert_eq!(configs[1].network, "stagenet");
        assert_eq!(configs[2].network, "stagenet");
    }
}
