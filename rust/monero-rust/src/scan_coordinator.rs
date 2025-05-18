use std::collections::HashSet;

use crate::scanner::{BlockScanResult, Lookahead, DEFAULT_LOOKAHEAD};
use crate::wallet_output::WalletOutput;

/// Sync progress information.
#[derive(Debug, Clone)]
pub struct SyncProgress {
    pub current_height: u64,
    pub target_height: u64,
    pub is_synced: bool,
    pub is_scanning: bool,
}

/// Compute sync progress from heights.
pub fn sync_progress(current_height: u64, target_height: u64) -> SyncProgress {
    let is_synced = current_height >= target_height;
    SyncProgress {
        current_height,
        target_height,
        is_synced,
        is_scanning: !is_synced,
    }
}

/// Per-block output summary from batch processing.
#[derive(Debug, Clone)]
pub struct BlockOutputSummary {
    pub block_height: u64,
    pub block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: usize,
    pub outputs: Vec<WalletOutput>,
    pub daemon_height: u64,
    pub spent_key_images: Vec<String>,
}

/// Processed result from a single-wallet batch scan.
#[derive(Debug, Clone)]
pub struct ProcessedBatch {
    pub batch_end_height: u64,
    pub outputs_to_store: Vec<WalletOutput>,
    pub spent_key_images: Vec<String>,
    pub should_continue: bool,
    pub blocks_with_outputs: Vec<BlockOutputSummary>,
    pub daemon_height: u64,
}

/// Compute the lookahead needed for a scan.
///
/// If specific accounts are requested, the lookahead account is the max.
/// Otherwise, use the provided account_lookahead.
pub fn compute_lookahead(
    account_lookahead: u32,
    accounts_to_scan: Option<&[u32]>,
) -> Lookahead {
    let account = if let Some(accounts) = accounts_to_scan {
        accounts.iter().max().copied().unwrap_or(0)
    } else {
        account_lookahead
    };
    Lookahead {
        account,
        subaddress: DEFAULT_LOOKAHEAD.subaddress,
    }
}

/// Filter outputs by account set.
///
/// If `accounts` is None, all outputs pass. If Some, only outputs whose
/// account (first element of subaddress_index, defaulting to 0) is in
/// the set pass.
pub fn filter_outputs_by_accounts<'a>(
    outputs: impl Iterator<Item = &'a WalletOutput>,
    accounts: Option<&HashSet<u32>>,
) -> Vec<WalletOutput> {
    outputs
        .filter(|o| match accounts {
            None => true,
            Some(set) => {
                let account = o.subaddress_index.map(|(a, _)| a).unwrap_or(0);
                set.contains(&account)
            }
        })
        .cloned()
        .collect()
}

/// Process a batch of single-wallet scan results.
///
/// Aggregates outputs and spent key images across the batch, filters
/// outputs by account, and determines whether scanning should continue.
pub fn process_single_wallet_batch(
    batch_results: &[BlockScanResult],
    accounts_to_scan: Option<&[u32]>,
    target_height: u64,
    batch_start_height: u64,
) -> ProcessedBatch {
    if batch_results.is_empty() {
        return ProcessedBatch {
            batch_end_height: batch_start_height,
            outputs_to_store: Vec::new(),
            spent_key_images: Vec::new(),
            should_continue: false,
            blocks_with_outputs: Vec::new(),
            daemon_height: 0,
        };
    }

    let batch_end_height = batch_results
        .last()
        .map(|r| r.block_height + 1)
        .unwrap_or(batch_start_height);

    let accounts_set: Option<HashSet<u32>> =
        accounts_to_scan.map(|a| a.iter().copied().collect());

    let mut all_outputs = Vec::new();
    let mut all_spent_key_images = Vec::new();
    let mut blocks_with_outputs = Vec::new();
    let mut last_daemon_height = 0u64;

    for result in batch_results {
        last_daemon_height = result.daemon_height;

        all_spent_key_images.extend(result.spent_key_images.iter().cloned());

        let filtered = filter_outputs_by_accounts(
            result.outputs.iter(),
            accounts_set.as_ref(),
        );

        if !filtered.is_empty() {
            all_outputs.extend(filtered.iter().cloned());

            blocks_with_outputs.push(BlockOutputSummary {
                block_height: result.block_height,
                block_hash: result.block_hash.clone(),
                block_timestamp: result.block_timestamp,
                tx_count: result.tx_count,
                outputs: filtered,
                daemon_height: result.daemon_height,
                spent_key_images: result.spent_key_images.clone(),
            });
        }
    }

    ProcessedBatch {
        batch_end_height,
        outputs_to_store: all_outputs,
        spent_key_images: all_spent_key_images,
        should_continue: batch_end_height < target_height,
        blocks_with_outputs,
        daemon_height: last_daemon_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output(amount: u64, height: u64, tx_hash: &str, account: u32) -> WalletOutput {
        WalletOutput {
            tx_hash: tx_hash.to_string(),
            output_index: 0,
            amount,
            amount_xmr: String::new(),
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
        }
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
            daemon_height: 1000,
            spent_key_images,
        }
    }

    // ---- sync_progress ----

    #[test]
    fn progress_synced_when_current_ge_target() {
        let p = sync_progress(100, 100);
        assert!(p.is_synced);
        assert!(!p.is_scanning);

        let p = sync_progress(101, 100);
        assert!(p.is_synced);
        assert!(!p.is_scanning);
    }

    #[test]
    fn progress_scanning_when_behind() {
        let p = sync_progress(50, 100);
        assert!(!p.is_synced);
        assert!(p.is_scanning);
    }

    // ---- compute_lookahead ----

    #[test]
    fn lookahead_uses_account_lookahead_when_no_accounts() {
        let l = compute_lookahead(5, None);
        assert_eq!(l.account, 5);
        assert_eq!(l.subaddress, DEFAULT_LOOKAHEAD.subaddress);
    }

    #[test]
    fn lookahead_uses_max_account_from_list() {
        let l = compute_lookahead(5, Some(&[0, 3, 1]));
        assert_eq!(l.account, 3);
    }

    #[test]
    fn lookahead_handles_empty_accounts_list() {
        let l = compute_lookahead(5, Some(&[]));
        assert_eq!(l.account, 0);
    }

    // ---- filter_outputs_by_accounts ----

    #[test]
    fn filter_none_passes_all() {
        let outputs = vec![
            make_output(1000, 100, "tx1", 0),
            make_output(2000, 100, "tx2", 1),
            make_output(3000, 100, "tx3", 2),
        ];
        let filtered = filter_outputs_by_accounts(outputs.iter(), None);
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn filter_specific_accounts() {
        let outputs = vec![
            make_output(1000, 100, "tx1", 0),
            make_output(2000, 100, "tx2", 1),
            make_output(3000, 100, "tx3", 2),
        ];
        let accounts: HashSet<u32> = [0, 2].into();
        let filtered = filter_outputs_by_accounts(outputs.iter(), Some(&accounts));
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].tx_hash, "tx1");
        assert_eq!(filtered[1].tx_hash, "tx3");
    }

    #[test]
    fn filter_defaults_none_subaddress_to_account_0() {
        let mut o = make_output(1000, 100, "tx1", 0);
        o.subaddress_index = None;
        let outputs = vec![o];
        let accounts: HashSet<u32> = [0].into();
        let filtered = filter_outputs_by_accounts(outputs.iter(), Some(&accounts));
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn filter_excludes_none_subaddress_if_account_0_not_in_set() {
        let mut o = make_output(1000, 100, "tx1", 0);
        o.subaddress_index = None;
        let outputs = vec![o];
        let accounts: HashSet<u32> = [1].into();
        let filtered = filter_outputs_by_accounts(outputs.iter(), Some(&accounts));
        assert_eq!(filtered.len(), 0);
    }

    // ---- process_single_wallet_batch ----

    #[test]
    fn empty_batch_returns_no_continue() {
        let batch = process_single_wallet_batch(&[], None, 1000, 500);
        assert!(!batch.should_continue);
        assert_eq!(batch.batch_end_height, 500);
        assert!(batch.outputs_to_store.is_empty());
        assert!(batch.spent_key_images.is_empty());
    }

    #[test]
    fn batch_aggregates_outputs_across_blocks() {
        let results = vec![
            make_block_result(100, vec![make_output(1000, 100, "tx1", 0)], vec![]),
            make_block_result(101, vec![], vec![]),
            make_block_result(102, vec![make_output(2000, 102, "tx2", 0)], vec![]),
        ];
        let batch = process_single_wallet_batch(&results, None, 1000, 100);
        assert_eq!(batch.outputs_to_store.len(), 2);
        assert_eq!(batch.batch_end_height, 103); // last height + 1
    }

    #[test]
    fn batch_aggregates_key_images_across_blocks() {
        let results = vec![
            make_block_result(100, vec![], vec!["ki_a".into()]),
            make_block_result(101, vec![], vec!["ki_b".into(), "ki_c".into()]),
        ];
        let batch = process_single_wallet_batch(&results, None, 1000, 100);
        assert_eq!(batch.spent_key_images.len(), 3);
        assert!(batch.spent_key_images.contains(&"ki_a".to_string()));
        assert!(batch.spent_key_images.contains(&"ki_b".to_string()));
        assert!(batch.spent_key_images.contains(&"ki_c".to_string()));
    }

    #[test]
    fn batch_filters_outputs_by_account() {
        let results = vec![
            make_block_result(
                100,
                vec![
                    make_output(1000, 100, "tx1", 0),
                    make_output(2000, 100, "tx2", 1),
                    make_output(3000, 100, "tx3", 2),
                ],
                vec![],
            ),
        ];
        let batch = process_single_wallet_batch(&results, Some(&[0, 2]), 1000, 100);
        assert_eq!(batch.outputs_to_store.len(), 2);
        assert_eq!(batch.outputs_to_store[0].tx_hash, "tx1");
        assert_eq!(batch.outputs_to_store[1].tx_hash, "tx3");
    }

    #[test]
    fn batch_only_includes_blocks_with_filtered_outputs() {
        let results = vec![
            make_block_result(100, vec![make_output(1000, 100, "tx1", 0)], vec![]),
            make_block_result(101, vec![make_output(2000, 101, "tx2", 1)], vec![]),
            make_block_result(102, vec![make_output(3000, 102, "tx3", 0)], vec![]),
        ];
        // Only account 0
        let batch = process_single_wallet_batch(&results, Some(&[0]), 1000, 100);
        assert_eq!(batch.blocks_with_outputs.len(), 2); // blocks 100 and 102
        assert_eq!(batch.blocks_with_outputs[0].block_height, 100);
        assert_eq!(batch.blocks_with_outputs[1].block_height, 102);
    }

    #[test]
    fn batch_should_continue_when_below_target() {
        let results = vec![
            make_block_result(100, vec![], vec![]),
        ];
        let batch = process_single_wallet_batch(&results, None, 1000, 100);
        assert!(batch.should_continue);
        assert_eq!(batch.batch_end_height, 101);
    }

    #[test]
    fn batch_should_not_continue_at_target() {
        let results = vec![
            make_block_result(999, vec![], vec![]),
        ];
        let batch = process_single_wallet_batch(&results, None, 1000, 999);
        assert!(!batch.should_continue);
        assert_eq!(batch.batch_end_height, 1000);
    }

    #[test]
    fn batch_preserves_daemon_height() {
        let results = vec![
            make_block_result(100, vec![], vec![]),
        ];
        let batch = process_single_wallet_batch(&results, None, 1000, 100);
        assert_eq!(batch.daemon_height, 1000);
    }

    #[test]
    fn batch_key_images_not_filtered_by_account() {
        // Key images from all transactions should be collected regardless of account filter
        let results = vec![
            make_block_result(
                100,
                vec![make_output(1000, 100, "tx1", 1)], // account 1 output
                vec!["ki_from_any_tx".into()],
            ),
        ];
        // Filter to account 0 only
        let batch = process_single_wallet_batch(&results, Some(&[0]), 1000, 100);
        // Outputs filtered out, but key images preserved
        assert_eq!(batch.outputs_to_store.len(), 0);
        assert_eq!(batch.spent_key_images.len(), 1);
        assert_eq!(batch.spent_key_images[0], "ki_from_any_tx");
    }
}
