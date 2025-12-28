use monero_rust::{
    process_batch_with_reorg_detection, BlockScanResult, ScanBatchOutcome, WalletOutput,
    WalletState,
};

/// Helper: create a WalletOutput with deterministic fields.
fn make_output(amount: u64, height: u64, tx_hash: &str, account: u32) -> WalletOutput {
    WalletOutput {
        tx_hash: tx_hash.to_string(),
        output_index: 0,
        amount,
        amount_xmr: String::new(),
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

/// Helper: create a BlockScanResult matching the scan_coordinator test pattern.
fn make_block_result(
    height: u64,
    outputs: Vec<WalletOutput>,
    spent_key_images: Vec<String>,
) -> BlockScanResult {
    let tx_hashes_len = spent_key_images.len();
    BlockScanResult {
        block_height: height,
        block_hash: format!("hash_{}", height),
        block_timestamp: height * 120,
        tx_count: outputs.len() + spent_key_images.len(),
        outputs,
        daemon_height: 3000,
        spent_key_images,
        spent_key_image_tx_hashes: vec![String::new(); tx_hashes_len],
    }
}

/// Scan, detect a reorg, roll back, and verify balance recovery.
#[test]
fn test_reorg_recovery_full_cycle() {
    let mut state = WalletState::new();

    // Phase 1: Normal scan of blocks 100-104
    let initial_results = vec![
        make_block_result(
            100,
            vec![make_output(1_000_000_000_000, 100, "tx_100", 0)],
            vec![],
        ),
        make_block_result(
            101,
            vec![make_output(2_000_000_000_000, 101, "tx_101", 0)],
            vec![],
        ),
        make_block_result(
            102,
            vec![make_output(500_000_000_000, 102, "tx_102", 0)],
            vec![],
        ),
        make_block_result(103, vec![], vec!["ki_tx_100".to_string()]), // tx_100 spent at height 103
        make_block_result(
            104,
            vec![make_output(3_000_000_000_000, 104, "tx_104", 0)],
            vec![],
        ),
    ];

    let outcome =
        process_batch_with_reorg_detection(&initial_results, &mut state, None, 3000, 100).unwrap();

    let batch = match outcome {
        ScanBatchOutcome::Normal(batch) => batch,
        _ => panic!("Expected Normal initial scan"),
    };

    // Apply batch results to state (mimicking what WalletActor does)
    state.add_outputs(batch.outputs_to_store);
    // Mark spent key images with height so rollback can correctly unspend them.
    // ki_tx_100 was spent in block 103 (known from our test scenario).
    state.mark_spent_by_key_images_at_height(&["ki_tx_100".to_string()], 103);
    for (h, hash) in &batch.block_hashes {
        state.record_block_hash(*h, hash.clone());
    }
    state.current_height = 104;

    // Verify initial state: 4 outputs, 1 spent
    assert_eq!(
        state.outputs().len(),
        4,
        "Should have 4 outputs after initial scan"
    );
    // Use balance_at_height with enough confirmations (10+ required for confirmed)
    let high_height = 200;
    let initial_balance = state.balance_at_height(high_height).confirmed;
    assert!(initial_balance > 0, "Should have non-zero balance");

    // Phase 2: Reorg detected at height 102
    // Blocks 100-101 match, but 102 has a different hash (simulating fork)
    let reorg_scan = vec![
        make_block_result(100, vec![], vec![]), // hash_100 matches
        make_block_result(101, vec![], vec![]), // hash_101 matches
        {
            // Inject a mismatched hash at height 102 to trigger reorg
            let mut block = make_block_result(102, vec![], vec![]);
            block.block_hash = "new_chain_hash_102".to_string();
            block
        },
    ];

    let outcome =
        process_batch_with_reorg_detection(&reorg_scan, &mut state, None, 3000, 100).unwrap();

    let info = match outcome {
        ScanBatchOutcome::Reorg(info) => info,
        _ => panic!("Expected Reorg at height 102"),
    };

    // Phase 3: Verify rollback
    assert_eq!(info.split_height, 102, "Fork point must be height 102");
    // Outputs at height >= 102 should be removed: tx_102 (h=102) and tx_104 (h=104)
    assert_eq!(
        info.outputs_removed, 2,
        "Should remove 2 outputs at heights >= 102"
    );
    // tx_100 was spent at height 103 (>= 102), so it should be unspent
    assert_eq!(
        info.outputs_unspent, 1,
        "Should unspend 1 output (tx_100 spent at h=103)"
    );
    // State rolled back to height 101
    assert_eq!(
        state.current_height, 101,
        "State height must roll back to fork_point - 1"
    );

    // Remaining outputs: tx_100 (unspent again) and tx_101
    assert_eq!(
        state.outputs().len(),
        2,
        "Should have 2 outputs after rollback"
    );

    // Phase 4: Re-scan from fork point with new chain
    let new_chain_results = vec![
        BlockScanResult {
            block_height: 102,
            block_hash: "new_chain_hash_102".to_string(),
            block_timestamp: 102 * 120,
            tx_count: 1,
            outputs: vec![make_output(4_000_000_000_000, 102, "tx_new_102", 0)],
            daemon_height: 3000,
            spent_key_images: vec![],
            spent_key_image_tx_hashes: vec![],
        },
        BlockScanResult {
            block_height: 103,
            block_hash: "new_chain_hash_103".to_string(),
            block_timestamp: 103 * 120,
            tx_count: 0,
            outputs: vec![],
            daemon_height: 3000,
            spent_key_images: vec![],
            spent_key_image_tx_hashes: vec![],
        },
    ];

    let outcome =
        process_batch_with_reorg_detection(&new_chain_results, &mut state, None, 3000, 102)
            .unwrap();

    let batch = match outcome {
        ScanBatchOutcome::Normal(batch) => batch,
        _ => panic!("Expected Normal re-scan"),
    };

    // Apply new chain results
    state.add_outputs(batch.outputs_to_store);
    for (h, hash) in &batch.block_hashes {
        state.record_block_hash(*h, hash.clone());
    }
    state.current_height = 103;

    // Phase 5: Verify balance recovery
    // Should now have: tx_100 (1 XMR, unspent), tx_101 (2 XMR), tx_new_102 (4 XMR)
    assert_eq!(
        state.outputs().len(),
        3,
        "Should have 3 outputs after re-scan"
    );
    // Use balance_at_height with enough confirmations (10+ required for confirmed)
    let recovered_balance = state.balance_at_height(high_height).confirmed;
    assert!(
        recovered_balance > 0,
        "Balance must be non-zero after recovery"
    );
    // Expected: 1 + 2 + 4 = 7 XMR in piconero
    let expected = 1_000_000_000_000u64 + 2_000_000_000_000 + 4_000_000_000_000;
    assert_eq!(
        recovered_balance, expected,
        "Balance must reflect new chain outputs"
    );
}

/// Edge case: reorg at the very first known block.
#[test]
fn test_reorg_at_first_block_recovers() {
    let mut state = WalletState::new();
    state.add_outputs(vec![make_output(1_000_000_000_000, 100, "tx_100", 0)]);
    state.current_height = 100;
    state.record_block_hash(100, "old_hash_100".to_string());

    let results = vec![BlockScanResult {
        block_height: 100,
        block_hash: "new_hash_100".to_string(), // different!
        block_timestamp: 12000,
        tx_count: 0,
        outputs: vec![],
        daemon_height: 3000,
        spent_key_images: vec![],
        spent_key_image_tx_hashes: vec![],
    }];

    let outcome =
        process_batch_with_reorg_detection(&results, &mut state, None, 3000, 100).unwrap();

    let info = match outcome {
        ScanBatchOutcome::Reorg(info) => info,
        _ => panic!("Expected Reorg"),
    };

    assert_eq!(info.split_height, 100);
    assert_eq!(info.outputs_removed, 1);
    assert_eq!(state.outputs().len(), 0);
}
