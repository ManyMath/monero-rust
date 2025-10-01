use monero_rust::{WalletState, WalletOutput, BlockHashChain};

/// Helper: create a WalletOutput with deterministic fields.
fn make_test_output(amount: u64, height: u64, tx_hash: &str, account: u32) -> WalletOutput {
    WalletOutput {
        tx_hash: tx_hash.to_string(),
        output_index: 0,
        amount,
        amount_xmr: format!("{:.12}", amount as f64 / 1_000_000_000_000.0),
        key: format!("key_{}", tx_hash),
        key_offset: format!("ko_{}", tx_hash),
        commitment_mask: format!("cm_{}", tx_hash),
        subaddress_index: Some((account, 0)),
        payment_id: None,
        received_output_bytes: String::new(),
        block_height: height,
        spent: false,
        spent_height: None,
        key_image: format!("ki_{}", tx_hash),
        is_coinbase: false,
        frozen: false,
    }
}

/// Regression guard for the "No selected outputs" bug:
/// Wallet outputs must survive serialization/deserialization roundtrip
/// via replace_outputs with correct balance preserved.
#[test]
fn test_wallet_state_reload_roundtrip() {
    // 1. Create WalletState with known outputs
    let mut state = WalletState::new();
    state.add_outputs(vec![
        make_test_output(1_000_000_000_000, 1000, "tx_a", 0),
        make_test_output(2_500_000_000_000, 1001, "tx_b", 0),
        make_test_output(500_000_000_000, 1002, "tx_c", 1),
    ]);
    state.current_height = 1002;
    state.daemon_height = 1100;
    state.record_block_hash(1000, "hash_1000".to_string());
    state.record_block_hash(1001, "hash_1001".to_string());
    state.record_block_hash(1002, "hash_1002".to_string());

    // Record pre-reload balance
    let pre_balance = state.balance();
    let pre_output_count = state.outputs().len();

    // Serialize individual components; WalletState itself is not Serialize.
    let outputs_json = serde_json::to_string(state.outputs()).unwrap();
    let block_hashes_json = serde_json::to_string(&state.block_hashes).unwrap();

    // 3. Simulate reload: create fresh WalletState
    let mut restored = WalletState::new();

    // 4. Restore via replace_outputs (matching RestoreWalletDataRequest handler)
    let restored_outputs: Vec<WalletOutput> = serde_json::from_str(&outputs_json).unwrap();
    restored.replace_outputs(restored_outputs);
    restored.current_height = 1002;
    restored.daemon_height = 1100;
    let restored_chain: BlockHashChain = serde_json::from_str(&block_hashes_json).unwrap();
    restored.block_hashes = restored_chain;

    // 5. Verify: outputs match pre-reload (regression guard for "No selected outputs")
    assert_eq!(
        restored.outputs().len(),
        pre_output_count,
        "Output count must match after reload"
    );
    assert_eq!(
        restored.balance().confirmed,
        pre_balance.confirmed,
        "Confirmed balance must match after reload"
    );
    assert_eq!(restored.outputs()[0].tx_hash, "tx_a");
    assert_eq!(restored.outputs()[1].tx_hash, "tx_b");
    assert_eq!(restored.outputs()[2].tx_hash, "tx_c");
    assert_eq!(restored.outputs()[0].amount, 1_000_000_000_000);
    assert_eq!(restored.outputs()[1].amount, 2_500_000_000_000);
    assert_eq!(restored.outputs()[2].amount, 500_000_000_000);
    assert_eq!(restored.block_hashes.get_hash(1000), Some("hash_1000"));
    assert_eq!(restored.block_hashes.get_hash(1002), Some("hash_1002"));
}

/// Verify that spent status survives serialization roundtrip.
#[test]
fn test_wallet_state_reload_preserves_spent_status() {
    let mut state = WalletState::new();
    state.add_outputs(vec![
        make_test_output(1_000_000_000_000, 1000, "tx_spent", 0),
        make_test_output(2_000_000_000_000, 1001, "tx_unspent", 0),
    ]);
    state.mark_spent_by_key_images_at_height(&["ki_tx_spent".to_string()], 1005);

    let outputs_json = serde_json::to_string(state.outputs()).unwrap();

    let mut restored = WalletState::new();
    let restored_outputs: Vec<WalletOutput> = serde_json::from_str(&outputs_json).unwrap();
    restored.replace_outputs(restored_outputs);

    assert!(
        restored.outputs()[0].spent,
        "Spent output must remain spent after reload"
    );
    assert!(
        !restored.outputs()[1].spent,
        "Unspent output must remain unspent after reload"
    );
}

/// Edge case: empty wallet state roundtrip.
#[test]
fn test_wallet_state_reload_empty_outputs() {
    let state = WalletState::new();
    let outputs_json = serde_json::to_string(state.outputs()).unwrap();

    let mut restored = WalletState::new();
    let restored_outputs: Vec<WalletOutput> = serde_json::from_str(&outputs_json).unwrap();
    restored.replace_outputs(restored_outputs);

    assert_eq!(restored.outputs().len(), 0);
    assert_eq!(restored.balance().confirmed, 0);
}
