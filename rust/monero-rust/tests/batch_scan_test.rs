#![cfg(not(target_arch = "wasm32"))]

mod common;

use monero_rust::monero_backend::block::Block;
use monero_rust::monero_backend::rpc::{BlockCompleteEntry, GetBlocksFastResponse};
use monero_rust::scanner::{
    process_batch_multi_wallet_response, process_batch_response, Lookahead, WalletScanConfig,
};

const STAGENET_SEED: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";

fn make_coinbase_block(height: u64) -> Block {
    use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    use monero_oxide::ed25519::CompressedPoint;
    use monero_rust::monero_backend::transaction::*;

    Block::new(
        monero_rust::monero_backend::block::BlockHeader {
            hardfork_version: 16,
            hardfork_signal: 16,
            timestamp: 1600000000 + height * 120,
            previous: [0u8; 32],
            nonce: 0,
        },
        Transaction::V2 {
            prefix: TransactionPrefix {
                additional_timelock: Timelock::Block((height + 60) as usize),
                inputs: vec![Input::Gen(height as usize)],
                outputs: vec![Output {
                    amount: Some(0),
                    key: CompressedPoint::from(ED25519_BASEPOINT_POINT.compress().to_bytes()),
                    view_tag: Some(0),
                }],
                extra: vec![
                    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0,
                ],
            },
            proofs: None,
        },
        vec![],
    )
    .expect("synthetic coinbase block should be valid")
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
    Lookahead {
        account: 0,
        subaddress: 5,
    }
}

// -- Error handling --

#[tokio::test]
async fn test_invalid_seed_rejected() {
    let resp = make_response(100, 1, 101);
    let err = process_batch_response(resp, "bad seed", "stagenet", lookahead(), None, "")
        .await
        .unwrap_err();
    assert!(err.contains("mnemonic") || err.contains("seed"), "{err}");
}

#[tokio::test]
async fn test_invalid_network_rejected() {
    let resp = make_response(100, 1, 101);
    let err = process_batch_response(resp, STAGENET_SEED, "badnet", lookahead(), None, "")
        .await
        .unwrap_err();
    assert!(err.contains("network") || err.contains("Network"), "{err}");
}

#[tokio::test]
async fn test_empty_configs_rejected() {
    let resp = make_response(100, 1, 101);
    let err = process_batch_multi_wallet_response(resp, vec![], None)
        .await
        .unwrap_err();
    assert!(err.contains("No wallet"));
}

#[tokio::test]
async fn test_one_bad_seed_fails_multi_wallet_batch() {
    let resp = make_response(100, 1, 101);
    let configs = vec![
        WalletScanConfig {
            mnemonic: STAGENET_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
            passphrase: String::new(),
        },
        WalletScanConfig {
            mnemonic: "bad".to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
            passphrase: String::new(),
        },
    ];
    assert!(process_batch_multi_wallet_response(resp, configs, None)
        .await
        .is_err());
}

// -- Edge cases --

#[tokio::test]
async fn test_empty_batch_returns_empty() {
    let resp = make_response(100, 0, 100);
    assert!(
        process_batch_response(resp, STAGENET_SEED, "stagenet", lookahead(), None, "")
            .await
            .unwrap()
            .0
            .is_empty()
    );
}

#[tokio::test]
async fn test_height_mismatch_detected() {
    let block = make_coinbase_block(999);
    let resp = GetBlocksFastResponse {
        status: "OK".to_string(),
        blocks: vec![BlockCompleteEntry {
            block: block.serialize(),
            txs: vec![],
            pruned: false,
            block_weight: 0,
        }],
        start_height: 100, // doesn't match Gen(999)
        current_height: 200,
        output_indices: vec![],
        untrusted: false,
        credits: 0,
        top_hash: String::new(),
        daemon_time: 0,
    };
    let err = process_batch_response(resp, STAGENET_SEED, "stagenet", lookahead(), None, "")
        .await
        .unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
}

// -- Multi-wallet structure --

#[tokio::test]
async fn test_multi_wallet_produces_entry_per_wallet() {
    let resp = make_response(100, 3, 103);
    let other_seed = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
    let configs = vec![
        WalletScanConfig {
            mnemonic: STAGENET_SEED.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
            passphrase: String::new(),
        },
        WalletScanConfig {
            mnemonic: other_seed.to_string(),
            network: "stagenet".to_string(),
            lookahead: lookahead(),
            passphrase: String::new(),
        },
    ];
    let (results, _) = process_batch_multi_wallet_response(resp, configs, None)
        .await
        .unwrap();
    assert_eq!(results.len(), 3);
    for r in &results {
        assert_eq!(r.wallet_results.len(), 2);
    }
}
