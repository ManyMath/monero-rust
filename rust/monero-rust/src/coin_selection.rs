use crate::wallet_output::WalletOutput;

/// Fee estimation constants (conservative estimates).
pub const FEE_PER_INPUT_ESTIMATE: u64 = 15_000_000; // ~0.015 XMR per input
pub const BASE_FEE_ESTIMATE: u64 = 20_000_000; // ~0.02 XMR base fee

/// Result of coin selection.
#[derive(Debug, Clone)]
pub struct CoinSelectionResult {
    pub selected: Vec<WalletOutput>,
    pub total: u64,
    pub estimated_fee: u64,
}

/// Select inputs for a transaction.
///
/// Strategy:
/// 1. Try to find the smallest single output covering amount + fee
/// 2. If no single output works, find the combination with minimum inputs
///    that minimizes excess (locked change)
///
/// If `manual_selection` is Some, only those outputs (by "txHash:outputIndex" key) are used.
pub fn select_inputs(
    spendable: &[WalletOutput],
    amount: u64,
    manual_selection: Option<&[String]>,
) -> Result<CoinSelectionResult, String> {
    let mut candidates: Vec<WalletOutput> = if let Some(selected_keys) = manual_selection {
        spendable
            .iter()
            .filter(|o| selected_keys.contains(&o.output_key()))
            .cloned()
            .collect()
    } else {
        spendable.to_vec()
    };

    if candidates.is_empty() {
        return Err(if manual_selection.is_some() {
            "No selected outputs available to spend".to_string()
        } else {
            "No confirmed outputs available to spend (outputs need 10 confirmations)".to_string()
        });
    }

    // If manual selection, use all selected outputs
    if manual_selection.is_some() {
        let total: u64 = candidates.iter().map(|o| o.amount).sum();
        let estimated_fee = BASE_FEE_ESTIMATE + (candidates.len() as u64 * FEE_PER_INPUT_ESTIMATE);
        return Ok(CoinSelectionResult {
            selected: candidates,
            total,
            estimated_fee,
        });
    }

    // Auto-selection: try single output first
    let single_input_fee = BASE_FEE_ESTIMATE + FEE_PER_INPUT_ESTIMATE;
    let needed_for_single = amount + single_input_fee;

    candidates.sort_by_key(|o| o.amount);

    if let Some(output) = candidates.iter().find(|o| o.amount >= needed_for_single) {
        let output = output.clone();
        return Ok(CoinSelectionResult {
            total: output.amount,
            estimated_fee: single_input_fee,
            selected: vec![output],
        });
    }

    // No single output works: find optimal multi-input combination
    candidates.sort_by_key(|o| std::cmp::Reverse(o.amount));

    let mut best: Option<CoinSelectionResult> = None;

    for target_count in 2..=candidates.len() {
        let estimated_fee = BASE_FEE_ESTIMATE + (target_count as u64 * FEE_PER_INPUT_ESTIMATE);
        let needed_total = amount + estimated_fee;

        if let Some((selection, total)) =
            find_best_combination(&candidates, needed_total, target_count)
        {
            if best.is_none() {
                best = Some(CoinSelectionResult {
                    selected: selection,
                    total,
                    estimated_fee,
                });
                break; // Minimum input count found
            }
        }
    }

    match best {
        Some(result) => Ok(result),
        None => {
            // Fall back to using all outputs
            let total: u64 = candidates.iter().map(|o| o.amount).sum();
            let estimated_fee =
                BASE_FEE_ESTIMATE + (candidates.len() as u64 * FEE_PER_INPUT_ESTIMATE);
            Ok(CoinSelectionResult {
                selected: candidates,
                total,
                estimated_fee,
            })
        }
    }
}

/// Maximum number of combinations to evaluate before giving up on exact search.
const MAX_COMBINATIONS: u64 = 100_000;

/// Find the best combination of exactly `target_count` outputs summing to >= `needed_total`.
/// Among valid combinations, picks the one with smallest excess (minimizes locked change).
/// Falls back to a greedy selection if the search space exceeds MAX_COMBINATIONS.
pub fn find_best_combination(
    outputs: &[WalletOutput],
    needed_total: u64,
    target_count: usize,
) -> Option<(Vec<WalletOutput>, u64)> {
    if target_count == 0 || target_count > outputs.len() {
        return None;
    }

    // Check if search space is tractable: C(n, k) <= MAX_COMBINATIONS
    if combinations_exceed(outputs.len(), target_count, MAX_COMBINATIONS) {
        // Greedy: pick the largest `target_count` outputs (already sorted descending)
        let selected: Vec<WalletOutput> = outputs.iter().take(target_count).cloned().collect();
        let total: u64 = selected.iter().map(|o| o.amount).sum();
        return if total >= needed_total {
            Some((selected, total))
        } else {
            None
        };
    }

    fn search(
        outputs: &[WalletOutput],
        needed: u64,
        target_count: usize,
        start_idx: usize,
        current: &mut Vec<WalletOutput>,
        current_sum: u64,
        best: &mut Option<(Vec<WalletOutput>, u64)>,
    ) {
        if current.len() == target_count {
            if current_sum >= needed {
                let current_excess = current_sum - needed;
                let is_better = match best {
                    None => true,
                    Some((_, best_sum)) => {
                        let best_excess = *best_sum - needed;
                        current_excess < best_excess
                    }
                };
                if is_better {
                    *best = Some((current.clone(), current_sum));
                }
            }
            return;
        }

        let remaining_needed = target_count - current.len();
        for i in start_idx..outputs.len() {
            if outputs.len() - i < remaining_needed {
                break;
            }

            current.push(outputs[i].clone());
            search(
                outputs,
                needed,
                target_count,
                i + 1,
                current,
                current_sum + outputs[i].amount,
                best,
            );
            current.pop();
        }
    }

    let mut best: Option<(Vec<WalletOutput>, u64)> = None;
    let mut current = Vec::new();
    search(
        outputs,
        needed_total,
        target_count,
        0,
        &mut current,
        0,
        &mut best,
    );
    best
}

/// Check if C(n, k) exceeds `limit` without overflowing.
fn combinations_exceed(n: usize, k: usize, limit: u64) -> bool {
    let k = k.min(n - k);
    let mut c: u64 = 1;
    for i in 0..k {
        c = match c.checked_mul((n - i) as u64) {
            Some(v) => v / (i as u64 + 1),
            None => return true,
        };
        if c > limit {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output(amount: u64, tx_hash: &str) -> WalletOutput {
        WalletOutput {
            tx_hash: tx_hash.to_string(),
            output_index: 0,
            amount,
            amount_xmr: format!("{:.12}", amount as f64 / 1_000_000_000_000.0),
            key: "k".into(),
            key_offset: "ko".into(),
            commitment_mask: "cm".into(),
            subaddress_index: None,
            payment_id: None,
            received_output_bytes: "".into(),
            block_height: 100,
            spent: false,
            spent_height: None,
            key_image: format!("ki_{}", tx_hash),
            is_coinbase: false,
        }
    }

    #[test]
    fn test_single_output_sufficient() {
        let outputs = vec![
            make_output(5_000_000_000_000, "tx1"),
            make_output(2_000_000_000_000, "tx2"),
            make_output(500_000_000_000, "tx3"),
        ];
        let result = select_inputs(&outputs, 1_000_000_000_000, None).unwrap();
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].amount, 2_000_000_000_000); // smallest sufficient
    }

    #[test]
    fn test_multiple_outputs_needed() {
        let outputs = vec![
            make_output(2_000_000_000_000, "tx1"),
            make_output(1_500_000_000_000, "tx2"),
            make_output(800_000_000_000, "tx3"),
        ];
        let result = select_inputs(&outputs, 3_000_000_000_000, None).unwrap();
        assert_eq!(result.selected.len(), 2);
        let total: u64 = result.selected.iter().map(|o| o.amount).sum();
        assert!(total >= 3_000_000_000_000 + result.estimated_fee);
    }

    #[test]
    fn test_manual_selection() {
        let outputs = vec![
            make_output(1_000_000_000_000, "tx1"),
            make_output(2_000_000_000_000, "tx2"),
            make_output(3_000_000_000_000, "tx3"),
        ];
        let selected_keys = vec!["tx1:0".to_string(), "tx3:0".to_string()];
        let result = select_inputs(&outputs, 500_000_000_000, Some(&selected_keys)).unwrap();
        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.total, 4_000_000_000_000);
    }

    #[test]
    fn test_empty_spendable_returns_error() {
        let result = select_inputs(&[], 1_000_000_000_000, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_prefers_single_over_multi() {
        let outputs = vec![
            make_output(3_000_000_000_000, "tx_large"),
            make_output(100_000_000_000, "tx1"),
            make_output(100_000_000_000, "tx2"),
            make_output(100_000_000_000, "tx3"),
            make_output(100_000_000_000, "tx4"),
            make_output(100_000_000_000, "tx5"),
        ];
        let result = select_inputs(&outputs, 400_000_000_000, None).unwrap();
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].tx_hash, "tx_large");
    }

    #[test]
    fn test_minimize_change() {
        // 3 XMR needed, outputs [0.75, 1, 1.5, 2.5]
        // Best: 2.5 + 0.75 = 3.25 (least change)
        let outputs = vec![
            make_output(750_000_000_000, "tx1"),
            make_output(1_000_000_000_000, "tx2"),
            make_output(1_500_000_000_000, "tx3"),
            make_output(2_500_000_000_000, "tx4"),
        ];
        let result = select_inputs(&outputs, 3_000_000_000_000, None).unwrap();
        assert_eq!(result.selected.len(), 2);
        let total: u64 = result.selected.iter().map(|o| o.amount).sum();
        assert_eq!(total, 3_250_000_000_000);
    }

    #[test]
    fn test_find_best_combination_basic() {
        let outputs = vec![
            make_output(500_000_000_000, "tx1"),
            make_output(600_000_000_000, "tx2"),
            make_output(700_000_000_000, "tx3"),
            make_output(800_000_000_000, "tx4"),
        ];
        let result = find_best_combination(&outputs, 1_250_000_000_000, 2);
        assert!(result.is_some());
        let (selected, total) = result.unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(total, 1_300_000_000_000); // 800 + 500 = smallest >= 1250
    }

    #[test]
    fn test_find_best_combination_impossible() {
        let outputs = vec![
            make_output(100_000_000_000, "tx1"),
            make_output(200_000_000_000, "tx2"),
        ];
        let result = find_best_combination(&outputs, 1_000_000_000_000, 2);
        assert!(result.is_none());
    }
}
