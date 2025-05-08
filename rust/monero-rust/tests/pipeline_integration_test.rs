use monero_rust::{
    is_spendable, prepare_send_inputs, prepare_sweep_inputs,
    WalletOutput, WalletState,
    process_single_wallet_batch, compute_lookahead, sync_progress,
    filter_outputs_by_accounts, BlockScanResult,
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
        key_image: format!("ki_{}", tx_hash),
        is_coinbase: false,
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
