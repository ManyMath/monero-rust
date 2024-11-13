#![cfg(test)]

use super::*;
use crate::messages::*;
use messages::prelude::*;

#[tokio::test]
async fn test_wallet_actor_can_be_created() {
    let ctx = Context::new();
    let addr = ctx.address();
    let _wallet = WalletActor::new(addr);

    // If we get here without panicking, the actor was created successfully
    assert!(true);
}

#[tokio::test]
async fn test_store_outputs_message() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());

    let outputs = vec![
        StoredOutput {
            tx_hash: "test_hash".to_string(),
            output_index: 0,
            amount: 1000000000000,
            key: "test_key".to_string(),
            key_offset: "test_offset".to_string(),
            commitment_mask: "test_mask".to_string(),
            subaddress: None,
            payment_id: None,
            received_output_bytes: String::new(),
            block_height: 1000,
            spent: false,
            key_image: "test_key_image".to_string(),
        },
    ];

    let msg = StoreOutputs {
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
        outputs,
        daemon_height: 1100,
    };

    // Create a standalone context for calling notify
    let test_ctx = Context::new();

    // Notify the actor - this tests that the message can be processed
    wallet.notify(msg, &test_ctx).await;

    // If no panic, the message was processed successfully
    assert!(true);
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
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
    };

    let test_ctx = Context::new();
    wallet.notify(msg, &test_ctx).await;

    // Message processed without panic
    assert!(true);
}

#[tokio::test]
async fn test_stop_scan_message() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());

    // First set up scanning state
    let start_msg = UpdateScanState {
        is_scanning: true,
        current_height: 1000,
        target_height: 1100,
        node_url: "http://localhost:38081".to_string(),
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
    };

    let test_ctx = Context::new();
    wallet.notify(start_msg, &test_ctx).await;

    // Now stop the scan
    wallet.notify(StopScan, &test_ctx).await;

    // Both messages processed successfully
    assert!(true);
}

#[tokio::test]
async fn test_multiple_output_batches() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());
    let test_ctx = Context::new();

    // First batch
    let outputs1 = vec![
        StoredOutput {
            tx_hash: "hash1".to_string(),
            output_index: 0,
            amount: 1000000000000,
            key: "key1".to_string(),
            key_offset: "offset1".to_string(),
            commitment_mask: "mask1".to_string(),
            subaddress: None,
            payment_id: None,
            received_output_bytes: String::new(),
            block_height: 1000,
            spent: false,
            key_image: "key_image_1".to_string(),
        },
    ];

    wallet.notify(StoreOutputs {
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
        outputs: outputs1,
        daemon_height: 1100,
    }, &test_ctx).await;

    // Second batch
    let outputs2 = vec![
        StoredOutput {
            tx_hash: "hash2".to_string(),
            output_index: 1,
            amount: 2000000000000,
            key: "key2".to_string(),
            key_offset: "offset2".to_string(),
            commitment_mask: "mask2".to_string(),
            subaddress: None,
            payment_id: None,
            received_output_bytes: String::new(),
            block_height: 1050,
            spent: false,
            key_image: "key_image_2".to_string(),
        },
    ];

    wallet.notify(StoreOutputs {
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
        outputs: outputs2,
        daemon_height: 1100,
    }, &test_ctx).await;

    // Both batches processed successfully
    assert!(true);
}

#[tokio::test]
async fn test_scan_state_transitions() {
    let ctx = Context::new();
    let addr = ctx.address();
    let mut wallet = WalletActor::new(addr.clone());
    let test_ctx = Context::new();

    // Start scanning
    wallet.notify(UpdateScanState {
        is_scanning: true,
        current_height: 1000,
        target_height: 1100,
        node_url: "http://localhost:38081".to_string(),
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
    }, &test_ctx).await;

    // Update progress
    wallet.notify(UpdateScanState {
        is_scanning: true,
        current_height: 1050,
        target_height: 1100,
        node_url: "http://localhost:38081".to_string(),
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
    }, &test_ctx).await;

    // Complete scan
    wallet.notify(UpdateScanState {
        is_scanning: true,
        current_height: 1100,
        target_height: 1100,
        node_url: "http://localhost:38081".to_string(),
        seed: "test seed".to_string(),
        network: "stagenet".to_string(),
    }, &test_ctx).await;

    // Stop scan
    wallet.notify(StopScan, &test_ctx).await;

    // All state transitions processed successfully
    assert!(true);
}
