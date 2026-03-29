#![cfg(test)]

use super::*;
use crate::messages::*;
use messages::prelude::*;

const BIP39_SEED: &str =
    "color ranch color remove subway public water embrace before begin liberty fault";

fn make_test_output(
    tx_hash: &str,
    output_index: u8,
    amount: u64,
    block_height: u64,
) -> monero_rust::WalletOutput {
    monero_rust::WalletOutput {
        tx_hash: tx_hash.to_string(),
        output_index,
        amount,
        amount_xmr: format!("{:.12}", amount as f64 / 1_000_000_000_000.0),
        key: format!("key_{}", tx_hash),
        key_offset: format!("offset_{}", tx_hash),
        commitment_mask: format!("mask_{}", tx_hash),
        subaddress_index: None,
        payment_id: None,
        received_output_bytes: String::new(),
        block_height,
        spent: false,
        spent_height: None,
        key_image: format!("ki_{}", tx_hash),
        is_coinbase: false,
        frozen: false,
    }
}

#[tokio::test]
async fn test_wallet_actor_can_be_created() {
    let ctx = Context::new();
    let addr = ctx.address();
    let _wallet = WalletActor::new(addr);
}

#[tokio::test]
async fn test_store_outputs_message() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());

    let msg = StoreOutputs {
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
        outputs: vec![make_test_output("test_hash", 0, 1_000_000_000_000, 1000)],
        daemon_height: 1100,
        block_hashes: vec![(1000, "hash_1000".to_string())],
    };

    let test_ctx = Context::new();
    wallet.notify(msg, &test_ctx).await;
}

#[tokio::test]
async fn test_update_scan_state_message() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());

    let msg = UpdateScanState {
        is_scanning: true,
        current_height: 1050,
        target_height: 1100,
        node_url: "http://localhost:38081".to_string(),
        seed: "test seed".to_string().into(),
        passphrase: String::new().into(),
        network: "stagenet".to_string(),
        account_lookahead: 50,
        subaddress_lookahead: 0,
        accounts_to_scan: None,
    };

    let test_ctx = Context::new();
    wallet.notify(msg, &test_ctx).await;
}

#[tokio::test]
async fn test_stop_scan_message() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());

    let start_msg = UpdateScanState {
        is_scanning: true,
        current_height: 1000,
        target_height: 1100,
        node_url: "http://localhost:38081".to_string(),
        seed: "test seed".to_string().into(),
        passphrase: String::new().into(),
        network: "stagenet".to_string(),
        account_lookahead: 50,
        subaddress_lookahead: 0,
        accounts_to_scan: None,
    };

    let test_ctx = Context::new();
    wallet.notify(start_msg, &test_ctx).await;
    wallet.notify(StopScan, &test_ctx).await;
}

#[tokio::test]
async fn test_multiple_output_batches() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());
    let test_ctx = Context::new();

    wallet
        .notify(
            StoreOutputs {
                seed: "test seed".to_string(),
                network: "stagenet".to_string(),
                outputs: vec![make_test_output("hash1", 0, 1_000_000_000_000, 1000)],
                daemon_height: 1100,
                block_hashes: vec![(1000, "bh_1000".to_string())],
            },
            &test_ctx,
        )
        .await;

    wallet
        .notify(
            StoreOutputs {
                seed: "test seed".to_string(),
                network: "stagenet".to_string(),
                outputs: vec![make_test_output("hash2", 1, 2_000_000_000_000, 1050)],
                daemon_height: 1100,
                block_hashes: vec![(1050, "bh_1050".to_string())],
            },
            &test_ctx,
        )
        .await;
}

#[tokio::test]
async fn test_scan_state_transitions() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());
    let test_ctx = Context::new();

    for height in [1000u64, 1050, 1100] {
        wallet
            .notify(
                UpdateScanState {
                    is_scanning: true,
                    current_height: height,
                    target_height: 1100,
                    node_url: "http://localhost:38081".to_string(),
                    seed: "test seed".to_string().into(),
                    passphrase: String::new().into(),
                    network: "stagenet".to_string(),
                    account_lookahead: 50,
                    subaddress_lookahead: 0,
                    accounts_to_scan: None,
                },
                &test_ctx,
            )
            .await;
    }

    wallet.notify(StopScan, &test_ctx).await;
}

// ---------------------------------------------------------------------------
// BIP39 actor-level tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_store_outputs_with_bip39_seed() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());
    let test_ctx = Context::new();

    wallet
        .notify(
            StoreOutputs {
                seed: BIP39_SEED.to_string(),
                network: "mainnet".to_string(),
                outputs: vec![make_test_output("bip39_tx", 0, 500_000_000_000, 3_100_000)],
                daemon_height: 3_200_000,
                block_hashes: vec![(3_100_000, "bh_3100000".to_string())],
            },
            &test_ctx,
        )
        .await;
}

#[tokio::test]
async fn test_update_scan_state_with_bip39_seed() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());
    let test_ctx = Context::new();

    wallet
        .notify(
            UpdateScanState {
                is_scanning: true,
                current_height: 3_000_000,
                target_height: 3_200_000,
                node_url: "http://localhost:18081".to_string(),
                seed: BIP39_SEED.to_string().into(),
                passphrase: String::new().into(),
                network: "mainnet".to_string(),
                account_lookahead: 50,
                subaddress_lookahead: 0,
                accounts_to_scan: None,
            },
            &test_ctx,
        )
        .await;

    wallet.notify(StopScan, &test_ctx).await;
}

#[tokio::test]
async fn test_full_scan_lifecycle_with_bip39_seed() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());
    let test_ctx = Context::new();

    // Start scan with BIP39 seed
    wallet
        .notify(
            UpdateScanState {
                is_scanning: true,
                current_height: 3_000_000,
                target_height: 3_200_000,
                node_url: "http://localhost:18081".to_string(),
                seed: BIP39_SEED.to_string().into(),
                passphrase: String::new().into(),
                network: "mainnet".to_string(),
                account_lookahead: 50,
                subaddress_lookahead: 0,
                accounts_to_scan: None,
            },
            &test_ctx,
        )
        .await;

    // Receive outputs
    wallet
        .notify(
            StoreOutputs {
                seed: BIP39_SEED.to_string(),
                network: "mainnet".to_string(),
                outputs: vec![
                    make_test_output("bip39_tx_1", 0, 1_000_000_000_000, 3_050_000),
                    make_test_output("bip39_tx_2", 0, 2_500_000_000_000, 3_100_000),
                ],
                daemon_height: 3_200_000,
                block_hashes: vec![
                    (3_050_000, "bh_3050000".to_string()),
                    (3_100_000, "bh_3100000".to_string()),
                ],
            },
            &test_ctx,
        )
        .await;

    // Update progress
    wallet
        .notify(
            UpdateScanState {
                is_scanning: true,
                current_height: 3_200_000,
                target_height: 3_200_000,
                node_url: "http://localhost:18081".to_string(),
                seed: BIP39_SEED.to_string().into(),
                passphrase: String::new().into(),
                network: "mainnet".to_string(),
                account_lookahead: 50,
                subaddress_lookahead: 0,
                accounts_to_scan: None,
            },
            &test_ctx,
        )
        .await;

    // Stop
    wallet.notify(StopScan, &test_ctx).await;
}
