#![cfg(not(target_arch = "wasm32"))]

use monero_serai::block::Block;
use monero_serai::rpc::{BlockCompleteEntry, GetBlocksFastResponse};
use monero_rust::scanner::{
    process_batch_multi_wallet_response, process_batch_response, Lookahead, WalletScanConfig,
};

const STAGENET_SEED: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";
const HONKED_BAGPIPE_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const HEMLOCK_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
const STAGENET_NODE: &str = "http://127.0.0.1:38081";

fn make_coinbase_block(height: u64) -> Block {
    use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    use monero_serai::ringct::*;
    use monero_serai::transaction::*;

    Block {
        header: monero_serai::block::BlockHeader {
            major_version: 16,
            minor_version: 16,
            timestamp: 1600000000 + height * 120,
            previous: [0u8; 32],
            nonce: 0,
        },
        miner_tx: Transaction {
            prefix: TransactionPrefix {
                version: 2,
                timelock: Timelock::Block((height + 60) as usize),
                inputs: vec![Input::Gen(height)],
                outputs: vec![Output {
                    amount: 0,
                    key: ED25519_BASEPOINT_POINT.compress(),
                    view_tag: Some(0),
                }],
                extra: vec![
                    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
            },
            signatures: vec![],
            rct_signatures: RctSignatures {
                base: RctBase { fee: 0, ecdh_info: vec![], commitments: vec![] },
                prunable: RctPrunable::Null,
            },
        },
        txs: vec![],
    }
}

fn make_response(start_height: u64, count: usize, daemon_height: u64) -> GetBlocksFastResponse {
    GetBlocksFastResponse {
        status: "OK".to_string(),
        blocks: (0..count)
            .map(|i| {
                let block = make_coinbase_block(start_height + i as u64);
                BlockCompleteEntry {
                    block: block.serialize(),
                    txs: vec![],
                    pruned: false,
                    block_weight: 0,
                }
            })
            .collect(),
        start_height,
        current_height: daemon_height,
        output_indices: vec![],
        untrusted: false,
        credits: 0,
        top_hash: String::new(),
        daemon_time: 0,
    }
}

fn lookahead() -> Lookahead {
    Lookahead { account: 0, subaddress: 5 }
}

// ── Error handling ──

#[tokio::test]
async fn test_invalid_seed_rejected() {
    let resp = make_response(100, 1, 101);
    let err = process_batch_response(resp, "bad seed", "stagenet", lookahead()).await.unwrap_err();
    assert!(err.contains("mnemonic") || err.contains("seed"), "{err}");
}

#[tokio::test]
async fn test_invalid_network_rejected() {
    let resp = make_response(100, 1, 101);
    let err = process_batch_response(resp, STAGENET_SEED, "badnet", lookahead()).await.unwrap_err();
    assert!(err.contains("network") || err.contains("Network"), "{err}");
}

#[tokio::test]
async fn test_empty_configs_rejected() {
    let resp = make_response(100, 1, 101);
    let err = process_batch_multi_wallet_response(resp, vec![]).await.unwrap_err();
    assert!(err.contains("No wallet"));
}

#[tokio::test]
async fn test_one_bad_seed_fails_multi_wallet_batch() {
    let resp = make_response(100, 1, 101);
    let configs = vec![
        WalletScanConfig { mnemonic: STAGENET_SEED.to_string(), network: "stagenet".to_string(), lookahead: lookahead() },
        WalletScanConfig { mnemonic: "bad".to_string(), network: "stagenet".to_string(), lookahead: lookahead() },
    ];
    assert!(process_batch_multi_wallet_response(resp, configs).await.is_err());
}

// ── Edge cases ──

#[tokio::test]
async fn test_empty_batch_returns_empty() {
    let resp = make_response(100, 0, 100);
    assert!(process_batch_response(resp, STAGENET_SEED, "stagenet", lookahead()).await.unwrap().is_empty());
}

#[tokio::test]
async fn test_height_mismatch_detected() {
    let block = make_coinbase_block(999);
    let resp = GetBlocksFastResponse {
        status: "OK".to_string(),
        blocks: vec![BlockCompleteEntry {
            block: block.serialize(), txs: vec![], pruned: false, block_weight: 0,
        }],
        start_height: 100, // doesn't match Gen(999)
        current_height: 200,
        output_indices: vec![],
        untrusted: false,
        credits: 0,
        top_hash: String::new(),
        daemon_time: 0,
    };
    let err = process_batch_response(resp, STAGENET_SEED, "stagenet", lookahead()).await.unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
}

// ── Multi-wallet structure ──

#[tokio::test]
async fn test_multi_wallet_produces_entry_per_wallet() {
    let resp = make_response(100, 3, 103);
    let other_seed = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
    let configs = vec![
        WalletScanConfig { mnemonic: STAGENET_SEED.to_string(), network: "stagenet".to_string(), lookahead: lookahead() },
        WalletScanConfig {
            mnemonic: other_seed.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
    ];
    let results = process_batch_multi_wallet_response(resp, configs).await.unwrap();
    assert_eq!(results.len(), 3);
    for r in &results {
        assert_eq!(r.wallet_results.len(), 2);
    }
}

// ── Live node tests ──

#[tokio::test]
#[ignore = "requires local stagenet node"]
async fn test_batch_scan_early_blocks() {
    use monero_rust::scanner::scan_blocks_batch;
    use monero_serai::rpc::HttpRpc;

    let rpc = HttpRpc::new(STAGENET_NODE.to_string()).unwrap();
    let results = scan_blocks_batch(
        &rpc, 100_000, STAGENET_SEED, "stagenet",
        Lookahead { account: 0, subaddress: 20 },
    ).await.expect("batch scan from 100k should succeed");

    assert!(!results.is_empty());
    let total_outputs: usize = results.iter().map(|r| r.outputs.len()).sum();
    let last_height = results.last().unwrap().block_height;
    println!("Scanned blocks 100000..{}: {} blocks, {} outputs",
        last_height, results.len(), total_outputs);
    assert!(total_outputs > 0, "wallet should have outputs in this range");
}

#[tokio::test]
#[ignore = "requires local stagenet node"]
async fn test_batch_scan_later_blocks() {
    use monero_rust::scanner::scan_blocks_batch;
    use monero_serai::rpc::HttpRpc;

    let rpc = HttpRpc::new(STAGENET_NODE.to_string()).unwrap();
    let results = scan_blocks_batch(
        &rpc, 800_000, STAGENET_SEED, "stagenet",
        Lookahead { account: 0, subaddress: 20 },
    ).await.expect("batch scan from 800k should succeed");

    assert!(!results.is_empty());
    let total_outputs: usize = results.iter().map(|r| r.outputs.len()).sum();
    let last_height = results.last().unwrap().block_height;
    println!("Scanned blocks 800000..{}: {} blocks, {} outputs",
        last_height, results.len(), total_outputs);
    assert!(total_outputs > 0, "wallet should have outputs in this range");
}

// ── Multi-wallet batch safety-net tests ──

#[tokio::test]
async fn test_multi_wallet_batch_metadata_matches_single_wallet() {
    let resp_single = make_response(100, 5, 105);
    let resp_multi = make_response(100, 5, 105);

    let single_results = process_batch_response(resp_single, STAGENET_SEED, "stagenet", lookahead())
        .await
        .unwrap();

    let configs = vec![WalletScanConfig {
        mnemonic: STAGENET_SEED.to_string(),
        network: "stagenet".to_string(),
        lookahead: lookahead(),
    }];
    let multi_results = process_batch_multi_wallet_response(resp_multi, configs)
        .await
        .unwrap();

    assert_eq!(single_results.len(), multi_results.len());
    for (s, m) in single_results.iter().zip(multi_results.iter()) {
        assert_eq!(s.block_height, m.block_height, "block_height mismatch at height {}", s.block_height);
        assert_eq!(s.block_timestamp, m.block_timestamp, "block_timestamp mismatch at height {}", s.block_height);
        assert_eq!(s.tx_count, m.tx_count, "tx_count mismatch at height {}", s.block_height);
    }
}

#[tokio::test]
async fn test_multi_wallet_batch_deterministic() {
    let configs = || {
        vec![
            WalletScanConfig {
                mnemonic: STAGENET_SEED.to_string(),
                network: "stagenet".to_string(),
                lookahead: lookahead(),
            },
            WalletScanConfig {
                mnemonic: HONKED_BAGPIPE_SEED.to_string(),
                network: "stagenet".to_string(),
                lookahead: lookahead(),
            },
        ]
    };

    let resp1 = make_response(200, 10, 210);
    let resp2 = make_response(200, 10, 210);

    let results1 = process_batch_multi_wallet_response(resp1, configs()).await.unwrap();
    let results2 = process_batch_multi_wallet_response(resp2, configs()).await.unwrap();

    assert_eq!(results1.len(), results2.len(), "result count differs between runs");

    for (r1, r2) in results1.iter().zip(results2.iter()) {
        assert_eq!(r1.block_height, r2.block_height);
        assert_eq!(r1.block_hash, r2.block_hash);
        assert_eq!(r1.block_timestamp, r2.block_timestamp);
        assert_eq!(r1.tx_count, r2.tx_count);

        // Same wallet addresses in both runs
        let addrs1: std::collections::BTreeSet<_> = r1.wallet_results.keys().collect();
        let addrs2: std::collections::BTreeSet<_> = r2.wallet_results.keys().collect();
        assert_eq!(addrs1, addrs2, "wallet addresses differ at height {}", r1.block_height);

        // Same output counts per wallet
        for addr in &addrs1 {
            let o1 = r1.wallet_results[*addr].outputs.len();
            let o2 = r2.wallet_results[*addr].outputs.len();
            assert_eq!(o1, o2, "output count differs for wallet {} at height {}", addr, r1.block_height);
        }
    }
}

#[tokio::test]
async fn test_multi_wallet_batch_wallets_independent() {
    let resp = make_response(100, 3, 103);
    let configs = vec![
        WalletScanConfig {
            mnemonic: STAGENET_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
        WalletScanConfig {
            mnemonic: HONKED_BAGPIPE_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
    ];

    let results = process_batch_multi_wallet_response(resp, configs).await.unwrap();
    assert_eq!(results.len(), 3);

    // Derive expected addresses
    let addr1 = monero_rust::scanner::derive_address(STAGENET_SEED, "stagenet").unwrap();
    let addr2 = monero_rust::scanner::derive_address(HONKED_BAGPIPE_SEED, "stagenet").unwrap();
    assert_ne!(addr1, addr2, "the two wallets must have different addresses");

    for r in &results {
        assert_eq!(r.wallet_results.len(), 2, "each block should have 2 wallet entries");
        assert!(
            r.wallet_results.contains_key(&addr1),
            "missing entry for first wallet at height {}",
            r.block_height
        );
        assert!(
            r.wallet_results.contains_key(&addr2),
            "missing entry for second wallet at height {}",
            r.block_height
        );
    }
}

#[tokio::test]
async fn test_multi_wallet_batch_spent_key_images_shared() {
    let resp = make_response(100, 5, 105);
    let configs = vec![
        WalletScanConfig {
            mnemonic: STAGENET_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
        WalletScanConfig {
            mnemonic: HONKED_BAGPIPE_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
    ];

    let results = process_batch_multi_wallet_response(resp, configs).await.unwrap();

    // Synthetic coinbase-only blocks have Input::Gen, not Input::ToKey,
    // so spent_key_images should be empty (shared, not per-wallet).
    for r in &results {
        assert!(
            r.spent_key_images.is_empty(),
            "coinbase-only blocks should have no spent key images, got {} at height {}",
            r.spent_key_images.len(),
            r.block_height
        );
    }
}

#[tokio::test]
async fn test_multi_wallet_single_wallet_matches_single_batch() {
    let resp_single = make_response(100, 5, 105);
    let resp_multi = make_response(100, 5, 105);

    let single_results = process_batch_response(resp_single, STAGENET_SEED, "stagenet", lookahead())
        .await
        .unwrap();

    let configs = vec![WalletScanConfig {
        mnemonic: STAGENET_SEED.to_string(),
        network: "stagenet".to_string(),
        lookahead: lookahead(),
    }];
    let multi_results = process_batch_multi_wallet_response(resp_multi, configs)
        .await
        .unwrap();

    // Same number of block results
    assert_eq!(single_results.len(), multi_results.len());

    // Same block heights
    let single_heights: Vec<u64> = single_results.iter().map(|r| r.block_height).collect();
    let multi_heights: Vec<u64> = multi_results.iter().map(|r| r.block_height).collect();
    assert_eq!(single_heights, multi_heights);

    // Same number of outputs (should be 0 for synthetic blocks, but counts must match)
    let addr = monero_rust::scanner::derive_address(STAGENET_SEED, "stagenet").unwrap();
    for (s, m) in single_results.iter().zip(multi_results.iter()) {
        let single_output_count = s.outputs.len();
        let multi_output_count = m.wallet_results[&addr].outputs.len();
        assert_eq!(
            single_output_count, multi_output_count,
            "output count mismatch at height {}",
            s.block_height
        );
    }
}

#[tokio::test]
async fn test_multi_wallet_batch_three_wallets() {
    let resp = make_response(500, 5, 505);
    let configs = vec![
        WalletScanConfig {
            mnemonic: STAGENET_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
        WalletScanConfig {
            mnemonic: HONKED_BAGPIPE_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
        WalletScanConfig {
            mnemonic: HEMLOCK_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
    ];

    let results = process_batch_multi_wallet_response(resp, configs).await.unwrap();
    assert_eq!(results.len(), 5);

    for r in &results {
        assert_eq!(
            r.wallet_results.len(),
            3,
            "each block result should have exactly 3 wallet entries, got {} at height {}",
            r.wallet_results.len(),
            r.block_height
        );
    }
}

#[tokio::test]
async fn test_multi_wallet_batch_large_batch() {
    let resp = make_response(1000, 100, 1100);
    let configs = vec![
        WalletScanConfig {
            mnemonic: STAGENET_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
        WalletScanConfig {
            mnemonic: HONKED_BAGPIPE_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
        },
    ];

    let results = process_batch_multi_wallet_response(resp, configs).await.unwrap();
    assert_eq!(results.len(), 100, "should get exactly 100 block results");

    for (i, r) in results.iter().enumerate() {
        assert_eq!(
            r.block_height,
            1000 + i as u64,
            "block height should be sequential"
        );
        assert_eq!(
            r.wallet_results.len(),
            2,
            "each block should have 2 wallet entries, got {} at height {}",
            r.wallet_results.len(),
            r.block_height
        );
    }
}
