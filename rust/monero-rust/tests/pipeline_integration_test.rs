use monero_rust::{
    is_spendable, prepare_send_inputs, prepare_sweep_inputs,
    WalletOutput, WalletState,
    process_single_wallet_batch, process_batch_with_reorg_detection,
    compute_lookahead, sync_progress,
    filter_outputs_by_accounts, BlockScanResult,
    ScanBatchOutcome, MAX_REORG_DEPTH,
};
use std::collections::HashSet;

fn make_output(amount: u64, height: u64, tx_hash: &str, account: u32) -> WalletOutput {
    WalletOutput {
        tx_hash: tx_hash.to_string(),
        output_index: 0,
        amount,
        amount_xmr: format!("{:.12}", amount as f64 / 1_000_000_000_000.0),
        key: "k".into(),
        key_offset: "ko".into(),
        commitment_mask: "cm".into(),
        subaddress_index: Some((account, 0)),
        payment_id: None,
        received_output_bytes: "".into(),
        block_height: height,
        spent: false,
        spent_height: None,
        key_image: format!("ki_{}", tx_hash),
        is_coinbase: false,
        frozen: false,
    }
}

fn make_coinbase(amount: u64, height: u64, tx_hash: &str) -> WalletOutput {
    let mut o = make_output(amount, height, tx_hash, 0);
    o.is_coinbase = true;
    o
}

fn make_block_result(
    height: u64,
    outputs: Vec<WalletOutput>,
    spent_key_images: Vec<String>,
) -> BlockScanResult {
    BlockScanResult {
        block_height: height,
        block_hash: format!("hash_{}", height),
        block_timestamp: height * 120,
        tx_count: outputs.len() + spent_key_images.len(),
        outputs,
        daemon_height: 5000,
        spent_key_images,
    }
}

#[test]
fn pipeline_wallet_to_send_preparation() {
    // Simulate a wallet that has been scanning and accumulating outputs
    let mut state = WalletState::new();
    state.daemon_height = 1000;

    // Add outputs from scanning batches
    state.add_outputs(vec![
        make_output(5_000_000_000_000, 800, "tx1", 0),  // 200 confs → confirmed
        make_output(2_000_000_000_000, 800, "tx2", 0),  // 200 confs → confirmed
        make_output(1_000_000_000_000, 995, "tx3", 0),  // 5 confs → unconfirmed
        make_coinbase(3_000_000_000_000, 950, "cb1"), // 50 confs → locked (needs 60)
    ]);

    // Verify balance
    state.current_height = 1000;
    let bal = state.balance();
    assert_eq!(bal.confirmed, 7_000_000_000_000); // tx1 + tx2
    assert_eq!(bal.unconfirmed, 4_000_000_000_000); // tx3 + cb1

    // Mark one output as spent (simulating a spent key image from scanning)
    let marked = state.mark_spent_by_key_images(&["ki_tx2".to_string()]);
    assert_eq!(marked, 1);

    let bal = state.balance();
    assert_eq!(bal.confirmed, 5_000_000_000_000); // only tx1 now

    // Prepare a send using the full pipeline
    let result = prepare_send_inputs(
        state.outputs(),
        state.daemon_height,
        1_000_000_000_000, // send 1 XMR
        None,
    );
    assert!(result.is_ok());
    let prepared = result.unwrap();

    // Should select the 5 XMR output (only confirmed+unspent output)
    assert_eq!(prepared.stored_outputs.len(), 1);
    assert_eq!(prepared.stored_outputs[0].amount, 5_000_000_000_000);
    assert_eq!(prepared.spent_output_keys, vec!["tx1:0"]);

    // The StoredOutputData should have correct subaddress mapping
    assert_eq!(prepared.stored_outputs[0].subaddress, Some((0, 0)));
}

#[test]
fn pipeline_wallet_to_sweep_preparation() {
    let mut state = WalletState::new();
    state.daemon_height = 1000;

    // Multi-account wallet
    state.add_outputs(vec![
        make_output(3_000_000_000_000, 800, "tx1", 0), // account 0
        make_output(2_000_000_000_000, 800, "tx2", 1), // account 1
        make_output(1_000_000_000_000, 800, "tx3", 0), // account 0
        make_output(500_000_000_000, 800, "tx4", 2),   // account 2
    ]);

    // Sweep account 0 only
    let keys: Vec<String> = state.outputs()
        .iter()
        .filter(|o| o.subaddress_index.map(|(a, _)| a) == Some(0))
        .map(|o| o.output_key())
        .collect();

    let result = prepare_sweep_inputs(
        state.outputs(),
        state.daemon_height,
        Some(&keys),
    );
    assert!(result.is_ok());
    let prepared = result.unwrap();

    // Should include only account 0 outputs
    assert_eq!(prepared.stored_outputs.len(), 2);
    assert_eq!(prepared.total_input, 4_000_000_000_000); // 3 + 1 XMR
    assert_eq!(prepared.spent_output_keys.len(), 2);
}

#[test]
fn pipeline_insufficient_funds() {
    let mut state = WalletState::new();
    state.daemon_height = 1000;

    state.add_outputs(vec![
        make_output(100_000_000_000, 800, "tx1", 0), // 0.1 XMR
    ]);

    // Try to send more than available
    let result = prepare_send_inputs(
        state.outputs(),
        state.daemon_height,
        50_000_000_000_000, // 50 XMR
        None,
    );
    // Should still return Ok (fallback to all outputs), but the amount won't cover
    // The core tx_builder will reject it when building the transaction
    assert!(result.is_ok());
}

#[test]
fn pipeline_no_spendable_outputs() {
    let mut state = WalletState::new();
    state.daemon_height = 100;

    // All outputs are too recent
    state.add_outputs(vec![
        make_output(5_000_000_000_000, 98, "tx1", 0), // 2 confs
        make_output(3_000_000_000_000, 99, "tx2", 0), // 1 conf
    ]);

    let result = prepare_send_inputs(
        state.outputs(),
        state.daemon_height,
        1_000_000_000_000,
        None,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No confirmed outputs"));
}

#[test]
fn pipeline_coinbase_maturity_in_full_flow() {
    let mut state = WalletState::new();
    state.daemon_height = 160;
    state.current_height = 160;

    // Coinbase at height 100 → 60 confs at height 160 → just mature
    state.add_outputs(vec![
        make_coinbase(5_000_000_000_000, 100, "cb1"),
    ]);

    // At daemon_height=160, coinbase should be spendable (60 confs)
    assert!(is_spendable(&state.outputs()[0], 160));

    let bal = state.balance();
    assert_eq!(bal.confirmed, 5_000_000_000_000);

    let result = prepare_send_inputs(
        state.outputs(),
        state.daemon_height,
        1_000_000_000_000,
        None,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap().stored_outputs.len(), 1);

    // But at daemon_height=159, coinbase is NOT spendable (59 confs)
    state.daemon_height = 159;
    assert!(!is_spendable(&state.outputs()[0], 159));

    let result = prepare_send_inputs(
        state.outputs(),
        state.daemon_height,
        1_000_000_000_000,
        None,
    );
    assert!(result.is_err());
}

#[test]
fn pipeline_coin_selection_optimizes_inputs() {
    let mut state = WalletState::new();
    state.daemon_height = 1000;

    // Several outputs: coin selection should pick the smallest sufficient one
    state.add_outputs(vec![
        make_output(10_000_000_000_000, 800, "tx_big", 0),
        make_output(3_000_000_000_000, 800, "tx_med", 0),
        make_output(1_100_000_000_000, 800, "tx_fit", 0), // barely covers 1 XMR + fee
        make_output(500_000_000_000, 800, "tx_small", 0),
    ]);

    let result = prepare_send_inputs(
        state.outputs(),
        state.daemon_height,
        1_000_000_000_000, // 1 XMR
        None,
    );
    assert!(result.is_ok());
    let prepared = result.unwrap();

    // Should pick the 1.1 XMR output (smallest sufficient for 1 XMR + fee)
    assert_eq!(prepared.stored_outputs.len(), 1);
    assert_eq!(prepared.stored_outputs[0].amount, 1_100_000_000_000);
}

#[test]
fn scan_pipeline_batch_to_wallet_state() {
    // Simulate: scan batch → process → add to wallet state → check balance
    let batch_results = vec![
        make_block_result(
            100,
            vec![make_output(1_000_000_000_000, 100, "tx1", 0)],
            vec![],
        ),
        make_block_result(101, vec![], vec!["some_spent_ki".into()]),
        make_block_result(
            102,
            vec![
                make_output(2_000_000_000_000, 102, "tx2", 0),
                make_output(500_000_000_000, 102, "tx3", 1),
            ],
            vec![],
        ),
    ];

    // Process the batch (account 0 only)
    let processed = process_single_wallet_batch(&batch_results, Some(&[0]), 5000, 100);

    assert!(processed.should_continue); // 103 < 5000
    assert_eq!(processed.batch_end_height, 103);
    assert_eq!(processed.outputs_to_store.len(), 2); // tx1 + tx2 (tx3 filtered)
    assert_eq!(processed.spent_key_images.len(), 1);
    assert_eq!(processed.blocks_with_outputs.len(), 2); // blocks 100 and 102

    // Feed outputs into WalletState
    let mut state = WalletState::new();
    state.daemon_height = 5000;
    state.add_outputs(processed.outputs_to_store);
    state.mark_spent_by_key_images(&processed.spent_key_images);

    // Check balance
    state.current_height = processed.batch_end_height;
    let bal = state.balance_at_height(5000);
    assert_eq!(bal.confirmed, 3_000_000_000_000); // tx1 + tx2
}

#[test]
fn scan_pipeline_multiple_batches() {
    // Simulate two consecutive batches
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    // Batch 1: blocks 100-102
    let batch1 = vec![
        make_block_result(
            100,
            vec![make_output(1_000_000_000_000, 100, "tx1", 0)],
            vec![],
        ),
        make_block_result(101, vec![], vec![]),
        make_block_result(102, vec![], vec![]),
    ];
    let p1 = process_single_wallet_batch(&batch1, None, 5000, 100);
    assert!(p1.should_continue);
    state.add_outputs(p1.outputs_to_store);

    // Batch 2: blocks 103-105 (includes spent key image from batch 1's output)
    let batch2 = vec![
        make_block_result(103, vec![], vec!["ki_tx1".into()]),
        make_block_result(
            104,
            vec![make_output(2_000_000_000_000, 104, "tx2", 0)],
            vec![],
        ),
        make_block_result(105, vec![], vec![]),
    ];
    let p2 = process_single_wallet_batch(&batch2, None, 5000, 103);
    assert!(p2.should_continue);
    state.add_outputs(p2.outputs_to_store);
    state.mark_spent_by_key_images(&p2.spent_key_images);

    // tx1 should now be spent, tx2 should be unspent
    assert!(state.outputs()[0].spent);
    assert!(!state.outputs()[1].spent);

    state.current_height = 5000;
    let bal = state.balance();
    assert_eq!(bal.confirmed, 2_000_000_000_000); // only tx2
}

#[test]
fn scan_pipeline_multi_account_filtering() {
    // Test that account filtering works correctly across a batch
    let batch = vec![
        make_block_result(
            200,
            vec![
                make_output(1_000_000_000_000, 200, "tx_a0", 0),
                make_output(2_000_000_000_000, 200, "tx_a1", 1),
                make_output(3_000_000_000_000, 200, "tx_a2", 2),
            ],
            vec![],
        ),
    ];

    // Filter accounts [0, 2]
    let processed = process_single_wallet_batch(&batch, Some(&[0, 2]), 5000, 200);
    assert_eq!(processed.outputs_to_store.len(), 2);
    assert_eq!(processed.outputs_to_store[0].tx_hash, "tx_a0");
    assert_eq!(processed.outputs_to_store[1].tx_hash, "tx_a2");

    // The same filtering using filter_outputs_by_accounts directly
    let all_outputs = &batch[0].outputs;
    let accounts: HashSet<u32> = [0, 2].into();
    let filtered = filter_outputs_by_accounts(all_outputs.iter(), Some(&accounts));
    assert_eq!(filtered.len(), 2);
}

#[test]
fn scan_pipeline_lookahead_computation() {
    // When scanning specific accounts [0, 3, 1], lookahead should be max(3)
    let la = compute_lookahead(10, Some(&[0, 3, 1]));
    assert_eq!(la.account, 3);

    // When scanning all accounts with lookahead 5
    let la = compute_lookahead(5, None);
    assert_eq!(la.account, 5);
}

#[test]
fn scan_pipeline_progress_tracking() {
    let p = sync_progress(500, 5000);
    assert!(!p.is_synced);
    assert!(p.is_scanning);
    assert_eq!(p.current_height, 500);
    assert_eq!(p.target_height, 5000);

    let p = sync_progress(5000, 5000);
    assert!(p.is_synced);
    assert!(!p.is_scanning);
}

#[test]
fn lifecycle_scan_spend_rescan() {
    let mut state = WalletState::new();
    state.daemon_height = 1000;

    // Phase 1: Initial scan finds outputs
    state.add_outputs(vec![
        make_output(5_000_000_000_000, 800, "tx1", 0),
        make_output(3_000_000_000_000, 850, "tx2", 0),
    ]);

    let bal = state.balance_at_height(1000);
    assert_eq!(bal.confirmed, 8_000_000_000_000);

    // Phase 2: Build a transaction (send 2 XMR)
    let send_result = prepare_send_inputs(
        state.outputs(),
        state.daemon_height,
        2_000_000_000_000,
        None,
    ).unwrap();

    // Should pick tx2 (3 XMR, smallest sufficient)
    assert_eq!(send_result.stored_outputs.len(), 1);
    assert_eq!(send_result.stored_outputs[0].amount, 3_000_000_000_000);

    // Phase 3: After broadcast, mark the output as spent
    // (In real flow, this happens via MarkOutputsSpent after broadcast)
    state.mark_spent_by_output_keys(&send_result.spent_output_keys);

    let bal = state.balance_at_height(1000);
    assert_eq!(bal.confirmed, 5_000_000_000_000); // only tx1 remains

    // Phase 4: Continued scanning finds change output
    state.add_outputs(vec![
        make_output(950_000_000_000, 1001, "tx_change", 0), // ~0.95 XMR change
    ]);
    state.daemon_height = 1100;
    state.current_height = 1100;

    let bal = state.balance();
    assert_eq!(bal.confirmed, 5_950_000_000_000); // tx1 + change
}

#[test]
fn lifecycle_manual_coin_control() {
    let mut state = WalletState::new();
    state.daemon_height = 1000;

    state.add_outputs(vec![
        make_output(1_000_000_000_000, 800, "tx1", 0),
        make_output(2_000_000_000_000, 800, "tx2", 0),
        make_output(3_000_000_000_000, 800, "tx3", 0),
    ]);

    // Manual selection: user picks specific outputs
    let manual_keys = vec!["tx1:0".to_string(), "tx3:0".to_string()];
    let result = prepare_send_inputs(
        state.outputs(),
        state.daemon_height,
        500_000_000_000, // 0.5 XMR
        Some(&manual_keys),
    ).unwrap();

    // Should use exactly the manually selected outputs
    assert_eq!(result.stored_outputs.len(), 2);
    assert_eq!(result.total_input, 4_000_000_000_000); // 1 + 3 XMR
    assert!(result.spent_output_keys.contains(&"tx1:0".to_string()));
    assert!(result.spent_output_keys.contains(&"tx3:0".to_string()));
}

#[test]
fn lifecycle_spendable_output_queries() {
    let mut state = WalletState::new();
    state.daemon_height = 1000;

    state.add_outputs(vec![
        make_output(1_000_000_000_000, 800, "tx1", 0),  // account 0, spendable
        make_output(2_000_000_000_000, 800, "tx2", 1),  // account 1, spendable
        make_output(3_000_000_000_000, 995, "tx3", 0),  // account 0, too recent
        make_coinbase(5_000_000_000_000, 950, "cb1"), // account 0, coinbase immature
    ]);

    // All spendable
    let spendable = state.spendable_outputs();
    assert_eq!(spendable.len(), 2); // tx1 + tx2

    // Spendable for account 0 only
    let spendable_a0 = state.spendable_outputs_for_accounts(&[0]);
    assert_eq!(spendable_a0.len(), 1); // only tx1
    assert_eq!(spendable_a0[0].key_image, "ki_tx1");

    // Spendable for accounts 0 + 1
    let spendable_both = state.spendable_outputs_for_accounts(&[0, 1]);
    assert_eq!(spendable_both.len(), 2); // tx1 + tx2
}

// ==== Reorg handling integration tests ====

/// Helper: run a normal batch through the reorg-aware pipeline, recording hashes.
fn apply_normal_batch(
    state: &mut WalletState,
    batch_results: &[BlockScanResult],
    accounts: Option<&[u32]>,
    target_height: u64,
    start_height: u64,
) -> monero_rust::ProcessedBatch {
    let outcome = process_batch_with_reorg_detection(
        batch_results, state, accounts, target_height, start_height,
    ).unwrap();
    match outcome {
        ScanBatchOutcome::Normal(batch) => {
            state.add_outputs(batch.outputs_to_store.clone());
            state.mark_spent_by_key_images_at_height(
                &batch.spent_key_images, batch.batch_end_height,
            );
            for (h, hash) in &batch.block_hashes {
                state.record_block_hash(*h, hash.clone());
            }
            batch
        }
        ScanBatchOutcome::Reorg(_) => panic!("Expected Normal, got Reorg"),
    }
}

#[test]
fn reorg_basic_detection_and_rollback() {
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    // Scan blocks 100-102 normally
    let batch1 = vec![
        make_block_result(100, vec![make_output(1_000_000_000_000, 100, "tx1", 0)], vec![]),
        make_block_result(101, vec![], vec![]),
        make_block_result(102, vec![make_output(2_000_000_000_000, 102, "tx2", 0)], vec![]),
    ];
    apply_normal_batch(&mut state, &batch1, None, 5000, 100);

    assert_eq!(state.outputs().len(), 2);
    assert_eq!(state.current_height, 102);

    // Now a reorg: block 101 has a different hash
    let reorg_batch = vec![
        make_block_result(100, vec![], vec![]), // hash matches
        // block 101: make_block_result produces "hash_101" but we recorded "hash_101" — same.
        // To trigger reorg, we need a block with a different hash at a known height.
    ];
    // Simulate reorg by creating a block 101 with a custom hash
    let mut reorg_block_101 = make_block_result(101, vec![], vec![]);
    reorg_block_101.block_hash = "reorged_hash_101".to_string();
    let reorg_batch = vec![
        make_block_result(100, vec![], vec![]),
        reorg_block_101,
    ];

    let outcome = process_batch_with_reorg_detection(
        &reorg_batch, &mut state, None, 5000, 100,
    ).unwrap();

    match outcome {
        ScanBatchOutcome::Reorg(info) => {
            assert_eq!(info.split_height, 101);
            // tx2 at height 102 was removed
            assert_eq!(info.outputs_removed, 1);
        }
        _ => panic!("Expected Reorg"),
    }

    // State rolled back: only tx1 (height 100) remains
    assert_eq!(state.outputs().len(), 1);
    assert_eq!(state.outputs()[0].tx_hash, "tx1");
    assert_eq!(state.current_height, 100);
}

#[test]
fn reorg_removes_outputs_in_fork_zone() {
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    // Scan blocks 100-104 with outputs at various heights
    let batch = vec![
        make_block_result(100, vec![make_output(1_000_000_000_000, 100, "tx_100", 0)], vec![]),
        make_block_result(101, vec![make_output(2_000_000_000_000, 101, "tx_101", 0)], vec![]),
        make_block_result(102, vec![make_output(3_000_000_000_000, 102, "tx_102", 0)], vec![]),
        make_block_result(103, vec![], vec![]),
        make_block_result(104, vec![make_output(4_000_000_000_000, 104, "tx_104", 0)], vec![]),
    ];
    apply_normal_batch(&mut state, &batch, None, 5000, 100);
    assert_eq!(state.outputs().len(), 4);

    // Reorg at height 102
    let mut reorg_block = make_block_result(102, vec![], vec![]);
    reorg_block.block_hash = "new_chain_102".to_string();
    let reorg_batch = vec![
        make_block_result(101, vec![], vec![]), // matches
        reorg_block,
    ];

    let outcome = process_batch_with_reorg_detection(
        &reorg_batch, &mut state, None, 5000, 101,
    ).unwrap();

    match outcome {
        ScanBatchOutcome::Reorg(info) => {
            assert_eq!(info.split_height, 102);
            // tx_102 and tx_104 removed (heights 102, 104)
            assert_eq!(info.outputs_removed, 2);
        }
        _ => panic!("Expected Reorg"),
    }

    assert_eq!(state.outputs().len(), 2);
    let remaining_txs: Vec<&str> = state.outputs().iter().map(|o| o.tx_hash.as_str()).collect();
    assert!(remaining_txs.contains(&"tx_100"));
    assert!(remaining_txs.contains(&"tx_101"));
}

#[test]
fn reorg_unspends_outputs_spent_in_fork_zone() {
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    // Receive output at height 50 (well before the fork)
    let batch1 = vec![
        make_block_result(50, vec![make_output(5_000_000_000_000, 50, "tx_early", 0)], vec![]),
    ];
    apply_normal_batch(&mut state, &batch1, None, 5000, 50);

    // In a later batch, the output is spent at height 100
    let batch2 = vec![
        make_block_result(100, vec![], vec!["ki_tx_early".into()]),
        make_block_result(101, vec![], vec![]),
    ];
    apply_normal_batch(&mut state, &batch2, None, 5000, 100);

    assert!(state.outputs()[0].spent);
    assert_eq!(state.outputs()[0].spent_height, Some(102)); // batch_end_height

    // Balance: 0 confirmed (output is spent)
    let bal = state.balance_at_height(5000);
    assert_eq!(bal.confirmed, 0);

    // Reorg at height 100: the spend is undone
    let mut reorg_block = make_block_result(100, vec![], vec![]);
    reorg_block.block_hash = "forked_100".to_string();
    let reorg_batch = vec![reorg_block];

    let outcome = process_batch_with_reorg_detection(
        &reorg_batch, &mut state, None, 5000, 100,
    ).unwrap();

    match outcome {
        ScanBatchOutcome::Reorg(info) => {
            assert_eq!(info.split_height, 100);
            assert_eq!(info.outputs_unspent, 1);
            assert_eq!(info.unspent_key_images, vec!["ki_tx_early".to_string()]);
        }
        _ => panic!("Expected Reorg"),
    }

    // Output is now unspent again
    assert!(!state.outputs()[0].spent);
    assert_eq!(state.outputs()[0].spent_height, None);

    // Balance restored
    let bal = state.balance_at_height(5000);
    assert_eq!(bal.confirmed, 5_000_000_000_000);
}

#[test]
fn reorg_then_rescan_new_chain() {
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    // Original chain: blocks 100-102
    let original = vec![
        make_block_result(100, vec![make_output(1_000_000_000_000, 100, "tx_orig", 0)], vec![]),
        make_block_result(101, vec![], vec![]),
        make_block_result(102, vec![make_output(2_000_000_000_000, 102, "tx_gone", 0)], vec![]),
    ];
    apply_normal_batch(&mut state, &original, None, 5000, 100);
    assert_eq!(state.outputs().len(), 2);

    // Reorg at 101 detected
    let mut forked_101 = make_block_result(101, vec![], vec![]);
    forked_101.block_hash = "fork_101".to_string();
    let reorg_batch = vec![
        make_block_result(100, vec![], vec![]),
        forked_101,
        make_block_result(102, vec![make_output(9_000_000_000_000, 102, "tx_new", 0)], vec![]),
    ];

    let outcome = process_batch_with_reorg_detection(
        &reorg_batch, &mut state, None, 5000, 100,
    ).unwrap();

    match &outcome {
        ScanBatchOutcome::Reorg(info) => {
            assert_eq!(info.split_height, 101);
        }
        _ => panic!("Expected Reorg"),
    }

    // Now process the new chain blocks (101+)
    let new_chain_blocks: Vec<BlockScanResult> = reorg_batch.into_iter()
        .filter(|r| r.block_height >= 101)
        .collect();
    let batch = process_single_wallet_batch(&new_chain_blocks, None, 5000, 101);
    state.add_outputs(batch.outputs_to_store);
    for (h, hash) in &batch.block_hashes {
        state.record_block_hash(*h, hash.clone());
    }

    // tx_orig kept, tx_gone removed, tx_new added
    assert_eq!(state.outputs().len(), 2);
    let txs: Vec<&str> = state.outputs().iter().map(|o| o.tx_hash.as_str()).collect();
    assert!(txs.contains(&"tx_orig"));
    assert!(txs.contains(&"tx_new"));

    let bal = state.balance_at_height(5000);
    assert_eq!(bal.confirmed, 10_000_000_000_000); // 1 + 9 XMR
}

#[test]
fn reorg_multi_account_balance_recalculation() {
    let mut state = WalletState::new();
    state.daemon_height = 200;

    // Multi-account outputs
    let batch = vec![
        make_block_result(100, vec![
            make_output(1_000_000_000_000, 100, "tx_a0", 0),
            make_output(2_000_000_000_000, 100, "tx_a1", 1),
        ], vec![]),
        make_block_result(101, vec![
            make_output(3_000_000_000_000, 101, "tx_a0_later", 0),
        ], vec![]),
    ];
    apply_normal_batch(&mut state, &batch, None, 200, 100);

    let bal = state.balance_at_height(200);
    assert_eq!(bal.confirmed, 6_000_000_000_000);

    // Reorg at 101: only tx_a0_later removed
    let mut forked = make_block_result(101, vec![], vec![]);
    forked.block_hash = "fork_101".to_string();

    let outcome = process_batch_with_reorg_detection(
        &[make_block_result(100, vec![], vec![]), forked],
        &mut state, None, 200, 100,
    ).unwrap();

    assert!(matches!(outcome, ScanBatchOutcome::Reorg(_)));
    assert_eq!(state.outputs().len(), 2);

    let bal = state.balance_at_height(200);
    assert_eq!(bal.confirmed, 3_000_000_000_000); // tx_a0 + tx_a1

    // Per-account balance via spendable queries
    state.daemon_height = 200;
    let a0 = state.spendable_outputs_for_accounts(&[0]);
    assert_eq!(a0.len(), 1);
    assert_eq!(a0[0].amount, 1_000_000_000_000);

    let a1 = state.spendable_outputs_for_accounts(&[1]);
    assert_eq!(a1.len(), 1);
    assert_eq!(a1[0].amount, 2_000_000_000_000);
}

#[test]
fn sequential_batches_with_block_hash_continuity() {
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    // Batch 1: blocks 100-102
    let batch1 = vec![
        make_block_result(100, vec![], vec![]),
        make_block_result(101, vec![], vec![]),
        make_block_result(102, vec![], vec![]),
    ];
    apply_normal_batch(&mut state, &batch1, None, 5000, 100);

    // Verify hashes recorded
    assert_eq!(state.block_hashes.get_hash(100), Some("hash_100"));
    assert_eq!(state.block_hashes.get_hash(102), Some("hash_102"));

    // Batch 2: blocks 103-105, should continue normally
    let batch2 = vec![
        make_block_result(103, vec![], vec![]),
        make_block_result(104, vec![], vec![]),
        make_block_result(105, vec![], vec![]),
    ];
    apply_normal_batch(&mut state, &batch2, None, 5000, 103);

    // All hashes recorded
    for h in 100..=105 {
        assert!(state.block_hashes.get_hash(h).is_some());
    }

    // Batch 3: overlaps with previous, should detect no reorg (matching hashes)
    let batch3 = vec![
        make_block_result(104, vec![], vec![]),
        make_block_result(105, vec![], vec![]),
        make_block_result(106, vec![make_output(1_000_000_000_000, 106, "tx_new", 0)], vec![]),
    ];
    apply_normal_batch(&mut state, &batch3, None, 5000, 104);
    assert_eq!(state.outputs().len(), 1);
}

#[test]
fn reorg_max_depth_boundary_integration() {
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    // Record a hash far back
    state.record_block_hash(100, "old_hash_100".to_string());

    // Set current height to MAX_REORG_DEPTH + 1 away (exceeds limit)
    state.current_height = 100 + MAX_REORG_DEPTH + 1;

    let results = vec![make_block_result(100, vec![], vec![])];
    let result = process_batch_with_reorg_detection(
        &results, &mut state, None, 5000, 100,
    );
    assert!(result.is_err());

    // Exactly MAX_REORG_DEPTH: should succeed (> not >=)
    state.current_height = 100 + MAX_REORG_DEPTH;
    state.record_block_hash(100, "old_hash_100".to_string());
    let result = process_batch_with_reorg_detection(
        &results, &mut state, None, 5000, 100,
    );
    assert!(result.is_ok());
}

#[test]
fn block_hash_compact_during_scan_pipeline() {
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    // Simulate scanning 600 blocks in batches of 100
    for batch_start in (0..600).step_by(100) {
        let batch: Vec<BlockScanResult> = (batch_start..batch_start + 100)
            .map(|h| make_block_result(h, vec![], vec![]))
            .collect();
        apply_normal_batch(&mut state, &batch, None, 5000, batch_start);
        state.block_hashes.compact();
    }

    // After compaction, we should have far fewer than 600 hashes
    assert!(state.block_hashes.len() < 150);
    // But still have the tip and genesis-area
    assert!(state.block_hashes.get_hash(599).is_some());
    assert!(state.block_hashes.get_hash(0).is_some());

    // Short chain history should work for fork detection
    let history = state.get_short_chain_history();
    assert!(!history.is_empty());
    assert_eq!(history[0].0, 599); // tip
}

#[test]
fn lifecycle_scan_reorg_spend_balance() {
    let mut state = WalletState::new();
    state.daemon_height = 200;

    // Phase 1: Scan and accumulate outputs
    let batch1 = vec![
        make_block_result(100, vec![
            make_output(5_000_000_000_000, 100, "tx_big", 0),
        ], vec![]),
        make_block_result(101, vec![
            make_output(2_000_000_000_000, 101, "tx_med", 0),
        ], vec![]),
        make_block_result(102, vec![
            make_output(1_000_000_000_000, 102, "tx_small", 0),
        ], vec![]),
    ];
    apply_normal_batch(&mut state, &batch1, None, 200, 100);

    let bal = state.balance_at_height(200);
    assert_eq!(bal.confirmed, 8_000_000_000_000);

    // Phase 2: Spend tx_med (prepare and mark spent)
    let send = prepare_send_inputs(state.outputs(), 200, 1_000_000_000_000, None).unwrap();
    state.mark_spent_by_output_keys(&send.spent_output_keys);

    let bal = state.balance_at_height(200);
    assert_eq!(bal.confirmed, 8_000_000_000_000 - send.stored_outputs[0].amount);

    // Phase 3: Reorg at height 102 — tx_small removed
    let mut forked = make_block_result(102, vec![], vec![]);
    forked.block_hash = "fork_102".to_string();
    let reorg_batch = vec![
        make_block_result(101, vec![], vec![]),
        forked,
    ];

    let outcome = process_batch_with_reorg_detection(
        &reorg_batch, &mut state, None, 200, 101,
    ).unwrap();

    match outcome {
        ScanBatchOutcome::Reorg(info) => {
            assert_eq!(info.split_height, 102);
            assert_eq!(info.outputs_removed, 1); // tx_small
        }
        _ => panic!("Expected Reorg"),
    }

    // tx_big and tx_med remain (one spent via mark_spent_by_output_keys — spent_height=None,
    // so conservative rollback does NOT unspend it)
    assert_eq!(state.outputs().len(), 2);
}

#[test]
fn reorg_conservative_no_unspend_without_height() {
    let mut state = WalletState::new();
    state.daemon_height = 5000;

    let batch = vec![
        make_block_result(100, vec![make_output(3_000_000_000_000, 100, "tx1", 0)], vec![]),
    ];
    apply_normal_batch(&mut state, &batch, None, 5000, 100);

    // Mark spent via output keys (no height tracked — old API)
    state.mark_spent_by_output_keys(&["tx1:0".to_string()]);
    assert!(state.outputs()[0].spent);
    assert_eq!(state.outputs()[0].spent_height, None);

    // Reorg at 50 — output is below split, but spent_height is None
    state.record_block_hash(50, "old_50".to_string());
    state.current_height = 100;

    let mut forked = make_block_result(50, vec![], vec![]);
    forked.block_hash = "new_50".to_string();
    let reorg_batch = vec![forked];

    let outcome = process_batch_with_reorg_detection(
        &reorg_batch, &mut state, None, 5000, 50,
    ).unwrap();

    match outcome {
        ScanBatchOutcome::Reorg(info) => {
            assert_eq!(info.outputs_unspent, 0); // Conservative: no unspend
            // Output at height 100 was removed though
            assert_eq!(info.outputs_removed, 1);
        }
        _ => panic!("Expected Reorg"),
    }
}
