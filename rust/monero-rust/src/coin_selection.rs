use crate::wallet_output::WalletOutput;
use monero_wallet::{interface::FeeRate, ringct::RctType};
use rand_core::{CryptoRng, RngCore};

/// `estimate_fee`'s per-input and base fees: CLSAG, 16-member rings, the
/// minimum fee rate, and about 4% margin.
pub const FEE_PER_INPUT_ESTIMATE: u64 = 16_000_000;
pub const BASE_FEE_ESTIMATE: u64 = 19_000_000;

/// Extra fee per Bulletproofs+ output-count doubling beyond the base 2 outputs.
/// When outputs exceed 2, BP+ pads to next power of 2. Each doubling adds ~640
/// bytes of proof weight = ~12,800,000 piconero. We use 13M for margin.
const BP_DOUBLING_FEE_ESTIMATE: u64 = 13_000_000;

/// Per-output field overhead (commitment + encrypted amount ~40 bytes = 800,000 piconero).
const PER_OUTPUT_FIELD_FEE_ESTIMATE: u64 = 1_000_000;

/// Minimum useful output amount (piconero). Outputs below this cost more
/// in fees to spend than they're worth. Approximately equal to the marginal
/// fee for one input at minimum fee rate.
pub const DUST_THRESHOLD: u64 = 20_000_000; // 0.00002 XMR

const MIN_OUTPUT_VALUE: u64 = 2_000_000_000_000;
const MIN_OUTPUT_COUNT: usize = 5;

/// Estimate the transaction fee given input and output counts.
///
/// Accounts for Bulletproofs+ output padding (outputs are padded to the next
/// power of 2, and each doubling beyond 2 adds ~640 bytes of proof weight).
pub fn estimate_fee(num_inputs: usize, num_outputs: usize) -> u64 {
    checked_estimate_fee(num_inputs, num_outputs).unwrap_or(u64::MAX)
}

/// Conservative fee for this transaction shape at the selected daemon rate.
pub fn estimate_fee_with_rate(
    num_inputs: usize,
    num_outputs: usize,
    rct_type: RctType,
    fee_rate: FeeRate,
) -> Result<u64, String> {
    let weight = crate::fee_estimate::transaction_weight_upper_bound(
        rct_type,
        num_inputs,
        num_outputs,
        true,
    )?;
    crate::fee_estimate::fee_for_weight(fee_rate, weight)
}

fn checked_estimate_fee(num_inputs: usize, num_outputs: usize) -> Option<u64> {
    let inputs = u64::try_from(num_inputs).ok()?;
    let base = BASE_FEE_ESTIMATE.checked_add(inputs.checked_mul(FEE_PER_INPUT_ESTIMATE)?)?;

    if num_outputs <= 2 {
        return Some(base);
    }

    // Extra output fields beyond the 2 included in BASE_FEE_ESTIMATE
    let extra_outputs = u64::try_from(num_outputs - 2).ok()?;
    let extra_output_fields = extra_outputs.checked_mul(PER_OUTPUT_FIELD_FEE_ESTIMATE)?;

    // BP+ pads to next power of 2; each doubling beyond 2 adds ~640 bytes
    let padded = u32::try_from(num_outputs)
        .ok()?
        .checked_next_power_of_two()?;
    let bp_doublings = padded.trailing_zeros().saturating_sub(1); // base is 2 = 2^1
    let bp_extra = u64::from(bp_doublings).checked_mul(BP_DOUBLING_FEE_ESTIMATE)?;

    base.checked_add(extra_output_fields)?.checked_add(bp_extra)
}

/// The amount below which wallet2 leaves an output unspent: the fee of one
/// more ring at `fee_rate`. Unlike wallet2, selection applies it to
/// preferred inputs too.
pub fn fractional_threshold(rct_type: RctType, fee_rate: FeeRate) -> Result<u64, String> {
    let one_ring = crate::fee_estimate::wallet2_weight_estimate(rct_type, 1, 2)?;
    let two_rings = crate::fee_estimate::wallet2_weight_estimate(rct_type, 2, 2)?;
    u64::try_from(two_rings - one_ring)
        .ok()
        .and_then(|weight| weight.checked_mul(fee_rate.per_weight()))
        .ok_or_else(|| "fractional output threshold overflows".to_string())
}

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
/// `num_recipients` is the number of destination addresses (used to estimate
/// fee weight from output count: recipients + 1 change output).
///
/// If `manual_selection` is Some, only those outputs (by "txHash:outputIndex" key) are used.
pub fn select_inputs(
    spendable: &[WalletOutput],
    amount: u64,
    num_recipients: usize,
    manual_selection: Option<&[String]>,
) -> Result<CoinSelectionResult, String> {
    select_inputs_with_estimator(
        spendable,
        amount,
        num_recipients,
        manual_selection,
        |inputs, outputs| {
            checked_estimate_fee(inputs, outputs)
                .ok_or_else(|| "fee estimate overflows".to_string())
        },
    )
}

/// wallet2's input selection for one transaction at `fee_rate`. Outputs below
/// [`fractional_threshold`] are left out unless `manual_selection` names them.
pub fn select_inputs_with_fee(
    spendable: &[WalletOutput],
    amount: u64,
    num_recipients: usize,
    manual_selection: Option<&[String]>,
    source_account: u32,
    rct_type: RctType,
    fee_rate: FeeRate,
) -> Result<CoinSelectionResult, String> {
    select_inputs_with_fee_and_rng(
        &mut rand::rngs::OsRng,
        spendable,
        amount,
        num_recipients,
        manual_selection,
        source_account,
        rct_type,
        fee_rate,
    )
}

/// RNG-injected wallet2-style selection, exposed for deterministic testing.
pub fn select_inputs_with_fee_and_rng<R: RngCore + CryptoRng>(
    rng: &mut R,
    spendable: &[WalletOutput],
    amount: u64,
    num_recipients: usize,
    manual_selection: Option<&[String]>,
    source_account: u32,
    rct_type: RctType,
    fee_rate: FeeRate,
) -> Result<CoinSelectionResult, String> {
    if amount == 0 {
        return Err("Nothing to send: the amount is zero".to_string());
    }
    let candidates = spendable
        .iter()
        .filter(|output| output.subaddress_index.map_or(0, |index| index.0) == source_account)
        .cloned()
        .collect::<Vec<_>>();
    wallet2_select_inputs(
        rng,
        candidates,
        amount,
        num_recipients,
        manual_selection,
        fractional_threshold(rct_type, fee_rate)?,
        crate::fee_estimate::fee_for_weight(
            fee_rate,
            crate::fee_estimate::wallet2_weight_estimate(rct_type, 2, 2)?,
        )?,
        |inputs, outputs| estimate_fee_with_rate(inputs, outputs, rct_type, fee_rate),
    )
}

fn output_relatedness(a: &WalletOutput, b: &WalletOutput) -> u8 {
    if a.tx_hash == b.tx_hash {
        return 100;
    }
    let distance = a.block_height.abs_diff(b.block_height);
    match distance {
        0 => 90,
        1 => 80,
        2..=9 => 20,
        _ => 0,
    }
}

fn in_subaddress(candidates: &[WalletOutput], subaddress: Option<(u32, u32)>) -> Vec<usize> {
    (0..candidates.len())
        .filter(|index| candidates[*index].subaddress_index == subaddress)
        .collect()
}

fn subaddress_order(candidates: &[WalletOutput]) -> Result<Vec<Option<(u32, u32)>>, String> {
    let mut balances: Vec<(Option<(u32, u32)>, u64)> = Vec::new();
    for output in candidates {
        match balances
            .iter_mut()
            .find(|(subaddress, _)| *subaddress == output.subaddress_index)
        {
            Some((_, balance)) => {
                *balance = balance
                    .checked_add(output.amount)
                    .ok_or_else(|| "subaddress balance overflows".to_string())?;
            }
            None => balances.push((output.subaddress_index, output.amount)),
        }
    }
    balances.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(balances
        .into_iter()
        .map(|(subaddress, _)| subaddress)
        .collect())
}

fn least_related(
    candidates: &[WalletOutput],
    pool: &[usize],
    selected: &[WalletOutput],
) -> Vec<usize> {
    let score = |index: usize| {
        selected
            .iter()
            .map(|chosen| output_relatedness(&candidates[index], chosen))
            .max()
            .unwrap_or(0)
    };
    let best = pool
        .iter()
        .map(|index| score(*index))
        .min()
        .expect("pool is non-empty");
    pool.iter()
        .copied()
        .filter(|index| score(*index) == best)
        .collect()
}

fn random_of<R: RngCore + CryptoRng>(rng: &mut R, indexes: &[usize]) -> usize {
    indexes[(rng.next_u64() as usize) % indexes.len()]
}

/// The second input wallet2 adds when one input already pays: an output of
/// the same subaddress, unrelated to the first, or none.
fn second_input(candidates: &[WalletOutput], first: &WalletOutput) -> Option<usize> {
    let pool = in_subaddress(candidates, first.subaddress_index);
    if pool.is_empty() {
        return None;
    }
    let ties = least_related(candidates, &pool, std::slice::from_ref(first));
    let index = ties
        .into_iter()
        .min_by_key(|index| candidates[*index].amount)?;
    if candidates[index].amount >= MIN_OUTPUT_VALUE {
        let large = pool
            .iter()
            .filter(|index| candidates[**index].amount >= MIN_OUTPUT_VALUE)
            .count();
        if large < MIN_OUTPUT_COUNT {
            return None;
        }
    }
    (output_relatedness(&candidates[index], first) == 0).then_some(index)
}

fn take(candidates: &mut Vec<WalletOutput>, index: usize) -> WalletOutput {
    let subaddress = candidates[index].subaddress_index;
    let last = (index..candidates.len())
        .rev()
        .find(|at| candidates[*at].subaddress_index == subaddress)
        .unwrap_or(index);
    candidates.swap(index, last);
    candidates.remove(last)
}

fn wallet2_select_inputs<R: RngCore + CryptoRng>(
    rng: &mut R,
    mut candidates: Vec<WalletOutput>,
    amount: u64,
    num_recipients: usize,
    manual_selection: Option<&[String]>,
    fractional_threshold: u64,
    preferred_fee: u64,
    fee_for: impl Fn(usize, usize) -> Result<u64, String>,
) -> Result<CoinSelectionResult, String> {
    let outputs = num_recipients
        .checked_add(1)
        .ok_or_else(|| "too many recipients".to_string())?;
    if let Some(keys) = manual_selection {
        candidates.retain(|output| keys.contains(&output.output_key()));
        if candidates.is_empty() {
            return Err("No selected outputs available to spend".to_string());
        }
        let total = crate::tx_utils::checked_amount_sum(candidates.iter().map(|o| o.amount))
            .ok_or_else(|| "selected output amounts overflow".to_string())?;
        let estimated_fee = fee_for(candidates.len(), outputs)?;
        return Ok(CoinSelectionResult {
            selected: candidates,
            total,
            estimated_fee,
        });
    }
    if candidates.is_empty() {
        return Err("No confirmed outputs available in the selected account".to_string());
    }

    let mut order = subaddress_order(&candidates)?;
    candidates.retain(|output| output.amount >= fractional_threshold);
    if candidates.is_empty() {
        return Err("No confirmed outputs worth their fee in the selected account".to_string());
    }
    // wallet2 judges preferred inputs by its fee estimate for two inputs and
    // two outputs, whatever the number of destinations.
    let preferred_needed = amount
        .checked_add(preferred_fee)
        .ok_or_else(|| "amount plus fee overflows".to_string())?;
    let mut selected = Vec::new();

    if let Some(index) = candidates
        .iter()
        .position(|output| output.amount >= preferred_needed)
    {
        selected.push(take(&mut candidates, index));
    } else {
        let mut best: Option<(usize, usize, u8)> = None;
        for first in 0..candidates.len() {
            for second in (first + 1)..candidates.len() {
                if candidates[first].subaddress_index != candidates[second].subaddress_index {
                    continue;
                }
                if candidates[first]
                    .amount
                    .checked_add(candidates[second].amount)
                    .map_or(true, |total| total < preferred_needed)
                {
                    continue;
                }
                let relatedness = output_relatedness(&candidates[first], &candidates[second]);
                // Strictly below 100, as in wallet2: two outputs of one
                // transaction are never a pair.
                if relatedness < best.map_or(100, |(_, _, score)| score) {
                    best = Some((first, second, relatedness));
                    if relatedness == 0 {
                        break;
                    }
                }
            }
            if best.is_some_and(|(_, _, score)| score == 0) {
                break;
            }
        }
        if let Some((first, second, _)) = best {
            selected.push(take(&mut candidates, second));
            selected.push(take(&mut candidates, first));
        }
    }

    if let Some(first) = selected.first() {
        if let Some(position) = order
            .iter()
            .position(|subaddress| *subaddress == first.subaddress_index)
        {
            order.swap(0, position);
        }
    }
    let mut current = 0;

    loop {
        let total = crate::tx_utils::checked_amount_sum(selected.iter().map(|o| o.amount))
            .ok_or_else(|| "selected output amounts overflow".to_string())?;
        let fee = fee_for(selected.len().max(1), outputs)?;
        let enough = total
            >= amount
                .checked_add(fee)
                .ok_or_else(|| "amount plus fee overflows".to_string())?;
        if enough {
            if let [first] = selected.as_slice() {
                if let Some(index) = second_input(&candidates, first) {
                    let with_second = total.checked_add(candidates[index].amount);
                    let needed = amount.checked_add(fee_for(2, outputs)?);
                    if with_second
                        .zip(needed)
                        .is_some_and(|(have, need)| have >= need)
                    {
                        selected.push(take(&mut candidates, index));
                        continue;
                    }
                }
            }
            return Ok(CoinSelectionResult {
                selected,
                total,
                estimated_fee: fee,
            });
        }
        let pool = loop {
            let Some(subaddress) = order.get(current) else {
                return Err("Insufficient funds in the selected account".to_string());
            };
            let pool = in_subaddress(&candidates, *subaddress);
            if !pool.is_empty() {
                break pool;
            }
            current += 1;
        };
        let ties = least_related(&candidates, &pool, &selected);
        let index = random_of(rng, &ties);
        selected.push(take(&mut candidates, index));
    }
}

fn select_inputs_with_estimator(
    spendable: &[WalletOutput],
    amount: u64,
    num_recipients: usize,
    manual_selection: Option<&[String]>,
    fee_for: impl Fn(usize, usize) -> Result<u64, String>,
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

    // recipients + change
    let num_outputs = num_recipients
        .checked_add(1)
        .ok_or_else(|| "too many recipients".to_string())?;

    // If manual selection, use all selected outputs
    if manual_selection.is_some() {
        let total = crate::tx_utils::checked_amount_sum(candidates.iter().map(|o| o.amount))
            .ok_or_else(|| "selected output amounts overflow".to_string())?;
        let estimated_fee = fee_for(candidates.len(), num_outputs)?;
        return Ok(CoinSelectionResult {
            selected: candidates,
            total,
            estimated_fee,
        });
    }

    // Auto-selection: try single output first
    let single_input_fee = fee_for(1, num_outputs)?;
    let needed_for_single = amount
        .checked_add(single_input_fee)
        .ok_or_else(|| "amount plus fee overflows".to_string())?;

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
        let estimated_fee = fee_for(target_count, num_outputs)?;
        let needed_total = amount
            .checked_add(estimated_fee)
            .ok_or_else(|| "amount plus fee overflows".to_string())?;

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
            let total = crate::tx_utils::checked_amount_sum(candidates.iter().map(|o| o.amount))
                .ok_or_else(|| "selected output amounts overflow".to_string())?;
            let estimated_fee = fee_for(candidates.len(), num_outputs)?;
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
        let total = crate::tx_utils::checked_amount_sum(selected.iter().map(|o| o.amount))?;
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
            if let Some(sum) = current_sum.checked_add(outputs[i].amount) {
                search(outputs, needed, target_count, i + 1, current, sum, best);
            }
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
    use rand::SeedableRng;

    fn make_output(amount: u64, tx_hash: &str) -> WalletOutput {
        WalletOutput {
            tx_hash: tx_hash.to_string(),
            output_index: 0,
            amount,
            key: "k".into(),
            key_offset: "ko".into(),
            commitment_mask: "cm".into(),
            subaddress_index: None,
            payment_id: None,
            received_output_bytes: "".into(),
            global_index: None,
            ring: None,
            block_height: 100,
            spent: false,
            spent_height: None,
            key_image: format!("ki_{}", tx_hash),
            is_coinbase: false,
            frozen: false,
        }
    }

    #[test]
    fn test_single_output_sufficient() {
        let outputs = vec![
            make_output(5_000_000_000_000, "tx1"),
            make_output(2_000_000_000_000, "tx2"),
            make_output(500_000_000_000, "tx3"),
        ];
        let result = select_inputs(&outputs, 1_000_000_000_000, 1, None).unwrap();
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].amount, 2_000_000_000_000); // smallest sufficient
    }

    #[test]
    fn selected_fee_tier_changes_which_inputs_are_affordable() {
        let mut first = make_output(1_050_000_000, "tx1");
        first.block_height = 100;
        let mut unrelated_cleanup = make_output(100_000_000, "tx2");
        unrelated_cleanup.block_height = 200;
        let mut related_larger = make_output(200_000_000, "tx3");
        related_larger.block_height = 101;
        let outputs = vec![first, unrelated_cleanup, related_larger];
        let low = FeeRate::new(20_000, 10_000).unwrap();
        let normal = FeeRate::new(80_000, 10_000).unwrap();
        let rct = RctType::ClsagBulletproofPlus;
        let mut low_rng = rand::rngs::StdRng::seed_from_u64(1);
        let mut normal_rng = rand::rngs::StdRng::seed_from_u64(1);

        let low_selection = select_inputs_with_fee_and_rng(
            &mut low_rng,
            &outputs,
            1_000_000_000,
            1,
            None,
            0,
            rct,
            low,
        )
        .unwrap();
        assert_eq!(low_selection.selected.len(), 2);
        assert!(low_selection
            .selected
            .iter()
            .any(|output| output.tx_hash == "tx2"));

        let normal_selection = select_inputs_with_fee_and_rng(
            &mut normal_rng,
            &outputs,
            1_000_000_000,
            1,
            None,
            0,
            rct,
            normal,
        )
        .unwrap();
        assert_eq!(normal_selection.selected.len(), 2);
        assert!(normal_selection
            .selected
            .iter()
            .any(|output| output.tx_hash == "tx3"));
        assert!(normal_selection.total >= 1_000_000_000 + normal_selection.estimated_fee);
    }

    #[test]
    fn wallet2_style_selection_uses_storage_order_and_a_second_input() {
        let mut oldest = make_output(3_000_000_000, "oldest");
        oldest.block_height = 100;
        let mut smallest = make_output(2_000_000_000, "smallest");
        smallest.block_height = 101;
        let mut unrelated = make_output(100_000_000, "cleanup");
        unrelated.block_height = 200;
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);
        let selected = select_inputs_with_fee_and_rng(
            &mut rng,
            &[oldest, smallest, unrelated],
            1_000_000_000,
            1,
            None,
            0,
            RctType::ClsagBulletproofPlus,
            FeeRate::new(20_000, 10_000).unwrap(),
        )
        .unwrap();

        assert_eq!(selected.selected.len(), 2);
        assert_eq!(selected.selected[0].tx_hash, "oldest");
        assert_eq!(selected.selected[1].tx_hash, "cleanup");
    }

    #[test]
    fn a_large_second_input_is_added_once_five_large_outputs_remain() {
        const XMR: u64 = 1_000_000_000_000;
        let large = |amount: u64, tx: &str, height: u64| {
            let mut output = make_output(amount, tx);
            output.block_height = height;
            output
        };
        let mut outputs = vec![large(3 * XMR, "first", 100)];
        for i in 0..4u64 {
            outputs.push(large(5 * XMR / 2, &format!("large{i}"), 200 + 100 * i));
        }
        let select = |outputs: &[WalletOutput]| {
            let mut rng = rand::rngs::StdRng::seed_from_u64(3);
            select_inputs_with_fee_and_rng(
                &mut rng,
                outputs,
                XMR,
                1,
                None,
                0,
                RctType::ClsagBulletproofPlus,
                FeeRate::new(20_000, 10_000).unwrap(),
            )
            .unwrap()
            .selected
        };
        assert_eq!(select(&outputs).len(), 1);
        outputs.push(large(5 * XMR / 2, "large4", 600));
        let selected = select(&outputs);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[1].tx_hash, "large4");
    }

    #[test]
    fn of_equal_second_inputs_the_one_wallet2s_list_puts_first_is_added() {
        const XMR: u64 = 1_000_000_000_000;
        let mut outputs = Vec::new();
        for (tx, height, count) in [
            ("a561", 600, 4),
            ("dc88", 601, 4),
            ("b6d8", 900, 1),
            ("9741", 2200, 4),
            ("9c8e", 3300, 4),
            ("dc15", 3600, 1),
        ] {
            for index in 0..count {
                let mut output = make_output(4 * XMR / 10, tx);
                output.output_index = index;
                output.key_image = format!("ki_{tx}_{index}");
                output.block_height = height;
                output.subaddress_index = Some((0, 0));
                outputs.push(output);
            }
        }
        for seed in 0..8 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let selected = select_inputs_with_fee_and_rng(
                &mut rng,
                &outputs,
                XMR / 10,
                1,
                None,
                0,
                RctType::ClsagBulletproofPlus,
                FeeRate::new(1_200_000, 10_000).unwrap(),
            )
            .unwrap()
            .selected;
            let heights: Vec<u64> = selected.iter().map(|output| output.block_height).collect();
            assert_eq!(heights, [600, 3600]);
            assert_eq!(selected[0].output_index, 0);
        }
    }

    #[test]
    fn wallet2_style_selection_never_crosses_accounts() {
        let mut account_zero = make_output(500_000_000, "a0");
        account_zero.subaddress_index = Some((0, 0));
        let mut account_three = make_output(5_000_000_000, "a3");
        account_three.subaddress_index = Some((3, 0));
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);

        let error = select_inputs_with_fee_and_rng(
            &mut rng,
            &[account_zero, account_three],
            1_000_000_000,
            1,
            None,
            0,
            RctType::ClsagBulletproofPlus,
            FeeRate::new(20_000, 10_000).unwrap(),
        )
        .unwrap_err();
        assert!(error.contains("selected account"), "{error}");
    }

    #[test]
    fn subaddresses_are_ordered_by_balance_then_first_receipt() {
        let at = |amount: u64, tx: &str, subaddress: (u32, u32)| {
            let mut output = make_output(amount, tx);
            output.subaddress_index = Some(subaddress);
            output
        };
        let outputs = [
            at(2, "a", (0, 4)),
            at(5, "b", (0, 1)),
            at(3, "c", (0, 4)),
            at(5, "d", (0, 9)),
            at(9, "e", (0, 2)),
        ];
        assert_eq!(
            subaddress_order(&outputs).unwrap(),
            [Some((0, 2)), Some((0, 4)), Some((0, 1)), Some((0, 9))]
        );
    }

    #[test]
    fn test_multiple_outputs_needed() {
        let outputs = vec![
            make_output(2_000_000_000_000, "tx1"),
            make_output(1_500_000_000_000, "tx2"),
            make_output(800_000_000_000, "tx3"),
        ];
        let result = select_inputs(&outputs, 3_000_000_000_000, 1, None).unwrap();
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
        let result = select_inputs(&outputs, 500_000_000_000, 1, Some(&selected_keys)).unwrap();
        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.total, 4_000_000_000_000);
    }

    #[test]
    fn manual_selection_refuses_an_amount_sum_that_overflows() {
        let outputs = vec![make_output(u64::MAX, "tx1"), make_output(1, "tx2")];
        let selected_keys = vec!["tx1:0".to_string(), "tx2:0".to_string()];
        let error = select_inputs(&outputs, 1, 1, Some(&selected_keys)).unwrap_err();
        assert!(error.contains("overflow"), "{error}");
    }

    #[test]
    fn test_empty_spendable_returns_error() {
        let result = select_inputs(&[], 1_000_000_000_000, 1, None);
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
        let result = select_inputs(&outputs, 400_000_000_000, 1, None).unwrap();
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
        let result = select_inputs(&outputs, 3_000_000_000_000, 1, None).unwrap();
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

    #[test]
    fn test_estimate_fee_2_outputs() {
        // 2 outputs = base case, no extra BP+ cost
        assert_eq!(
            estimate_fee(1, 2),
            BASE_FEE_ESTIMATE + FEE_PER_INPUT_ESTIMATE
        );
        assert_eq!(
            estimate_fee(2, 2),
            BASE_FEE_ESTIMATE + 2 * FEE_PER_INPUT_ESTIMATE
        );
    }

    #[test]
    fn test_estimate_fee_3_outputs() {
        // 3 outputs pads to 4 = 1 BP+ doubling beyond base
        let fee_1in_3out = estimate_fee(1, 3);
        let fee_1in_2out = estimate_fee(1, 2);
        // Should be noticeably more than 2-output case
        assert!(fee_1in_3out > fee_1in_2out + 10_000_000);
    }

    #[test]
    fn test_estimate_fee_increases_with_outputs() {
        // More outputs = higher fee
        let fee_2out = estimate_fee(1, 2);
        let fee_3out = estimate_fee(1, 3);
        let fee_5out = estimate_fee(1, 5);
        assert!(fee_3out > fee_2out);
        assert!(fee_5out > fee_3out);
    }

    #[test]
    fn test_select_inputs_multi_recipient() {
        // With 3 recipients (4 outputs), fee estimate should be higher
        let outputs = vec![
            make_output(5_000_000_000_000, "tx1"),
            make_output(2_000_000_000_000, "tx2"),
        ];
        let result_1r = select_inputs(&outputs, 1_000_000_000_000, 1, None).unwrap();
        let result_3r = select_inputs(&outputs, 1_000_000_000_000, 3, None).unwrap();
        assert!(result_3r.estimated_fee > result_1r.estimated_fee);
    }
}
