use std::collections::HashSet;

use rand_core::{CryptoRng, RngCore};

use crate::coin_selection::{
    estimate_fee, estimate_fee_with_rate, fractional_threshold, least_related, random_of,
    select_inputs, select_inputs_with_fee, CoinSelectionResult,
};
use crate::fee_estimate::wallet2_transaction_is_full;
use crate::monero_rpc::{Fee, Rpc, RpcConnection};
use crate::tx_builder::native::FeePolicy;
use crate::tx_builder::native::StoredOutputData;
use crate::wallet_output::WalletOutput;
use crate::wallet_state::{current_time_secs, is_spendable_at};

/// Prepared inputs ready for transaction building.
#[derive(Debug, Clone)]
pub struct PreparedInputs {
    pub stored_outputs: Vec<StoredOutputData>,
    pub spent_output_keys: Vec<String>,
    pub total_input: u64,
    /// Fee estimate for the selected transaction shape.
    pub estimated_fee: u64,
    /// The node's fee estimate the `_with_rpc` preparers used; `None` from
    /// the others.
    pub fee_rate: Option<Fee>,
    /// The fee policy the `_with_rpc` preparers used, an Automatic priority
    /// settled; `None` from the others.
    pub fee_policy: Option<FeePolicy>,
    /// Inputs to add, in order, if the transaction cannot pay its real fee.
    pub further_inputs: Vec<StoredOutputData>,
}

/// Prepare send inputs with a fixed tier-one fee estimate.
#[deprecated(note = "use prepare_send_inputs_with_rpc so selection uses the construction fee tier")]
pub fn prepare_send_inputs(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    total_amount: u64,
    num_recipients: usize,
    manual_selection: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
) -> Result<PreparedInputs, String> {
    prepare_send_inputs_at(
        wallet_outputs,
        wallet_height,
        current_time_secs(),
        total_amount,
        num_recipients,
        manual_selection,
        excluded_key_images,
    )
}

/// Prepare send inputs at the fee rate transaction construction will use.
/// `integrated` says the destination is an integrated address.
pub fn prepare_send_inputs_with_fee(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    total_amount: u64,
    num_recipients: usize,
    integrated: bool,
    manual_selection: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    rct_type: monero_wallet::ringct::RctType,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<PreparedInputs, String> {
    prepare_send_inputs_at_with_fee(
        wallet_outputs,
        wallet_height,
        current_time_secs(),
        total_amount,
        num_recipients,
        integrated,
        manual_selection,
        excluded_key_images,
        source_account,
        rct_type,
        fee_rate,
    )
}

/// Prepare send inputs at the node's RingCT type and the fee tier `policy`
/// selects.
/// An Automatic policy is settled here; build with
/// `PreparedInputs::fee_policy`.
/// `wallet_height` must be the wallet's own chain height
/// (`WalletState::chain_height()`), never the node's.
pub async fn prepare_send_inputs_with_rpc<R: RpcConnection>(
    rpc: &Rpc<R>,
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    total_amount: u64,
    num_recipients: usize,
    integrated: bool,
    manual_selection: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    policy: FeePolicy,
) -> Result<PreparedInputs, String> {
    let (rct_type, fee_rate, policy, fee) =
        construction_fee_rate(rpc, wallet_height, policy).await?;
    prepare_send_inputs_with_fee(
        wallet_outputs,
        wallet_height,
        total_amount,
        num_recipients,
        integrated,
        manual_selection,
        excluded_key_images,
        source_account,
        rct_type,
        fee_rate,
    )
    .map(|prepared| with_policy(prepared, policy, fee))
}

/// Clock-injected variant of `prepare_send_inputs_with_fee`.
pub fn prepare_send_inputs_at_with_fee(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    now_secs: Option<u64>,
    total_amount: u64,
    num_recipients: usize,
    integrated: bool,
    manual_selection: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    rct_type: monero_wallet::ringct::RctType,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<PreparedInputs, String> {
    let spendable: Vec<WalletOutput> = wallet_outputs
        .iter()
        .filter(|o| {
            !o.spent
                && !o.frozen
                && is_spendable_at(o, wallet_height, now_secs)
                && !excluded_key_images.map_or(false, |exc| exc.contains(&o.key_image))
        })
        .cloned()
        .collect();
    let CoinSelectionResult {
        selected,
        total,
        estimated_fee,
        further,
    } = select_inputs_with_fee(
        &spendable,
        total_amount,
        num_recipients,
        integrated,
        manual_selection,
        source_account,
        rct_type,
        fee_rate,
    )?;
    let spent_output_keys = selected.iter().map(WalletOutput::output_key).collect();
    let stored_outputs = selected.into_iter().map(Into::into).collect();
    Ok(PreparedInputs {
        stored_outputs,
        spent_output_keys,
        total_input: total,
        estimated_fee,
        fee_rate: None,
        fee_policy: None,
        further_inputs: further.into_iter().map(Into::into).collect(),
    })
}

/// Prepare a send using an explicit Unix clock, such as adjusted network time.
/// With no clock, timestamp-locked outputs remain locked.
#[deprecated(note = "use prepare_send_inputs_at_with_fee with the construction fee tier")]
pub fn prepare_send_inputs_at(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    now_secs: Option<u64>,
    total_amount: u64,
    num_recipients: usize,
    manual_selection: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
) -> Result<PreparedInputs, String> {
    let spendable: Vec<WalletOutput> = wallet_outputs
        .iter()
        .filter(|o| {
            !o.spent
                && !o.frozen
                && is_spendable_at(o, wallet_height, now_secs)
                && !excluded_key_images.map_or(false, |exc| exc.contains(&o.key_image))
        })
        .cloned()
        .collect();

    let CoinSelectionResult {
        selected,
        total,
        estimated_fee,
        ..
    } = select_inputs(&spendable, total_amount, num_recipients, manual_selection)?;

    let spent_output_keys: Vec<String> = selected.iter().map(|o| o.output_key()).collect();
    let stored_outputs: Vec<StoredOutputData> = selected.into_iter().map(Into::into).collect();

    Ok(PreparedInputs {
        stored_outputs,
        spent_output_keys,
        total_input: total,
        estimated_fee,
        fee_rate: None,
        fee_policy: None,
        further_inputs: Vec::new(),
    })
}

/// Prepare sweep inputs at the node's RingCT type and the fee tier `policy`
/// selects.
/// Without `selected_output_keys` it sweeps one subaddress.
pub async fn prepare_sweep_inputs_with_rpc<R: RpcConnection>(
    rpc: &Rpc<R>,
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    selected_output_keys: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    policy: FeePolicy,
) -> Result<PreparedInputs, String> {
    let (rct_type, fee_rate, policy, fee) =
        construction_fee_rate(rpc, wallet_height, policy).await?;
    prepare_sweep_inputs_with_fee(
        wallet_outputs,
        wallet_height,
        selected_output_keys,
        excluded_key_images,
        source_account,
        rct_type,
        fee_rate,
    )
    .map(|prepared| with_policy(prepared, policy, fee))
}

/// Prepare sweep inputs at an explicit daemon fee tier.
pub fn prepare_sweep_inputs_with_fee(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    selected_output_keys: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    rct_type: monero_wallet::ringct::RctType,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<PreparedInputs, String> {
    prepare_sweep_inputs_at_with_fee(
        wallet_outputs,
        wallet_height,
        current_time_secs(),
        selected_output_keys,
        excluded_key_images,
        source_account,
        rct_type,
        fee_rate,
    )
}

/// Clock-injected variant of `prepare_sweep_inputs_with_fee`.
/// Without `selected_output_keys` it sweeps one subaddress
/// ([`SweepSubaddresses::OneAtRandom`]).
/// A sweep too large for one transaction is an error; see
/// [`prepare_sweep_transactions`].
pub fn prepare_sweep_inputs_at_with_fee(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    now_secs: Option<u64>,
    selected_output_keys: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    rct_type: monero_wallet::ringct::RctType,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<PreparedInputs, String> {
    let spendable = spendable_in_account(
        wallet_outputs,
        wallet_height,
        now_secs,
        excluded_key_images,
        source_account,
    );
    let spendable = match selected_output_keys {
        Some(keys) => spendable
            .into_iter()
            .filter(|output| keys.contains(&output.output_key()))
            .collect(),
        None => from_subaddresses(
            &mut rand::rngs::OsRng,
            spendable,
            SweepSubaddresses::OneAtRandom,
            rct_type,
            fee_rate,
        )?,
    };
    prepared_sweep(spendable, rct_type, fee_rate)
}

/// Which subaddresses of the source account a sweep spends from. A sweep of
/// several links them on chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepSubaddresses<'a> {
    /// One funded subaddress at random, index 0 only when no other holds funds:
    /// wallet2's `sweep_all` without `index=`.
    OneAtRandom,
    /// The subaddresses with these indexes, wallet2's `index=N1,N2`.
    Only(&'a [u32]),
    /// Every subaddress of the account, wallet2's `index=all`.
    All,
}

/// Prepare a sweep of the chosen subaddresses of `source_account`.
pub fn prepare_sweep_inputs_from_subaddresses<R: RngCore + CryptoRng>(
    rng: &mut R,
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    now_secs: Option<u64>,
    subaddresses: SweepSubaddresses<'_>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    rct_type: monero_wallet::ringct::RctType,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<PreparedInputs, String> {
    let spendable = spendable_in_account(
        wallet_outputs,
        wallet_height,
        now_secs,
        excluded_key_images,
        source_account,
    );
    let spendable = from_subaddresses(rng, spendable, subaddresses, rct_type, fee_rate)?;
    prepared_sweep(spendable, rct_type, fee_rate)
}

/// [`prepare_sweep_inputs_from_subaddresses`] at the node's current RingCT
/// type and the fee tier `policy` selects, with the system clock and RNG.
pub async fn prepare_sweep_inputs_from_subaddresses_with_rpc<R: RpcConnection>(
    rpc: &Rpc<R>,
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    subaddresses: SweepSubaddresses<'_>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    policy: FeePolicy,
) -> Result<PreparedInputs, String> {
    let (rct_type, fee_rate, policy, fee) =
        construction_fee_rate(rpc, wallet_height, policy).await?;
    prepare_sweep_inputs_from_subaddresses(
        &mut rand::rngs::OsRng,
        wallet_outputs,
        wallet_height,
        current_time_secs(),
        subaddresses,
        excluded_key_images,
        source_account,
        rct_type,
        fee_rate,
    )
    .map(|prepared| with_policy(prepared, policy, fee))
}

/// Prepare a sweep as the series of transactions wallet2 would make of it.
pub fn prepare_sweep_transactions<R: RngCore + CryptoRng>(
    rng: &mut R,
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    now_secs: Option<u64>,
    subaddresses: SweepSubaddresses<'_>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    rct_type: monero_wallet::ringct::RctType,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<Vec<PreparedInputs>, String> {
    let spendable = spendable_in_account(
        wallet_outputs,
        wallet_height,
        now_secs,
        excluded_key_images,
        source_account,
    );
    let mut remaining = from_subaddresses(rng, spendable, subaddresses, rct_type, fee_rate)?;
    if remaining.is_empty() {
        return Err("No spendable outputs available for sweep in the selected account".to_string());
    }

    let mut transactions = Vec::new();
    let mut current: Vec<WalletOutput> = Vec::new();
    while !remaining.is_empty() {
        let pool: Vec<usize> = (0..remaining.len()).collect();
        let ties = least_related(&remaining, &pool, &current);
        current.push(remaining.remove(random_of(rng, &ties)));
        if remaining.is_empty() || wallet2_transaction_is_full(rct_type, current.len(), 2)? {
            transactions.push(prepared_sweep(
                std::mem::take(&mut current),
                rct_type,
                fee_rate,
            )?);
        }
    }
    Ok(transactions)
}

/// [`prepare_sweep_transactions`] at the node's current RingCT type and the
/// fee tier `policy` selects, with the system clock and RNG.
pub async fn prepare_sweep_transactions_with_rpc<R: RpcConnection>(
    rpc: &Rpc<R>,
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    subaddresses: SweepSubaddresses<'_>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
    policy: FeePolicy,
) -> Result<Vec<PreparedInputs>, String> {
    let (rct_type, fee_rate, policy, fee) =
        construction_fee_rate(rpc, wallet_height, policy).await?;
    prepare_sweep_transactions(
        &mut rand::rngs::OsRng,
        wallet_outputs,
        wallet_height,
        current_time_secs(),
        subaddresses,
        excluded_key_images,
        source_account,
        rct_type,
        fee_rate,
    )
    .map(|transactions| {
        transactions
            .into_iter()
            .map(|prepared| with_policy(prepared, policy, fee))
            .collect()
    })
}

async fn construction_fee_rate<R: RpcConnection>(
    rpc: &Rpc<R>,
    wallet_height: u64,
    policy: FeePolicy,
) -> Result<
    (
        monero_wallet::ringct::RctType,
        monero_wallet::interface::FeeRate,
        FeePolicy,
        Fee,
    ),
    String,
> {
    let rct_type = rpc
        .get_rct_type_at_height(Some(wallet_height))
        .await
        .map_err(|e| format!("Failed to get protocol: {e:?}"))?;
    let policy = policy.resolve(rpc).await?;
    let fee = rpc
        .get_fee_checked_for_priority_at_height(
            policy.priority,
            policy.wallet_height,
            policy.max_fee_per_weight,
        )
        .await
        .map_err(|e| format!("Failed to get fee rate: {e:?}"))?;
    let fee_rate = fee
        .rate()
        .ok_or_else(|| "Fee rate and quantization mask must be nonzero".to_string())?;
    Ok((rct_type, fee_rate, policy, fee))
}

fn spendable_in_account(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    now_secs: Option<u64>,
    excluded_key_images: Option<&HashSet<String>>,
    source_account: u32,
) -> Vec<WalletOutput> {
    wallet_outputs
        .iter()
        .filter(|output| {
            !output.spent
                && !output.frozen
                && is_spendable_at(output, wallet_height, now_secs)
                && output.subaddress_index.map_or(0, |index| index.0) == source_account
                && !excluded_key_images
                    .map_or(false, |excluded| excluded.contains(&output.key_image))
        })
        .cloned()
        .collect()
}

fn from_subaddresses<R: RngCore + CryptoRng>(
    rng: &mut R,
    mut spendable: Vec<WalletOutput>,
    subaddresses: SweepSubaddresses<'_>,
    rct_type: monero_wallet::ringct::RctType,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<Vec<WalletOutput>, String> {
    let threshold = fractional_threshold(rct_type, fee_rate)?;
    spendable.retain(|output| output.amount >= threshold);

    let minor = |output: &WalletOutput| output.subaddress_index.map_or(0, |index| index.1);
    match subaddresses {
        SweepSubaddresses::All => {}
        SweepSubaddresses::Only(indexes) => {
            spendable.retain(|output| indexes.contains(&minor(output)));
        }
        SweepSubaddresses::OneAtRandom => {
            let mut funded: Vec<u32> = spendable.iter().map(minor).collect();
            funded.sort_unstable();
            funded.dedup();
            if funded.len() > 1 {
                funded.retain(|index| *index != 0);
            }
            if !funded.is_empty() {
                let chosen = funded[(rng.next_u64() as usize) % funded.len()];
                spendable.retain(|output| minor(output) == chosen);
            }
        }
    }
    Ok(spendable)
}

fn prepared_sweep(
    spendable: Vec<WalletOutput>,
    rct_type: monero_wallet::ringct::RctType,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<PreparedInputs, String> {
    if spendable.is_empty() {
        return Err("No spendable outputs available for sweep in the selected account".to_string());
    }
    // wallet2 had closed the transaction before its last input was added.
    if spendable.len() > 1 && wallet2_transaction_is_full(rct_type, spendable.len() - 1, 2)? {
        return Err(format!(
            "This sweep has {} inputs, more than wallet2 puts in one transaction; \
             prepare_sweep_transactions splits it the way wallet2 does",
            spendable.len()
        ));
    }

    let total_input = crate::tx_utils::checked_amount_sum(spendable.iter().map(|o| o.amount))
        .ok_or_else(|| "sweep input amounts overflow".to_string())?;
    // wallet2 adds a zero-amount dummy output to a sweep's destination.
    let estimated_fee = estimate_fee_with_rate(spendable.len(), 2, rct_type, fee_rate)?;
    if total_input <= estimated_fee {
        return Err("Sweep inputs cannot pay the selected fee tier".to_string());
    }
    let spent_output_keys = spendable.iter().map(WalletOutput::output_key).collect();
    let stored_outputs = spendable.into_iter().map(Into::into).collect();
    Ok(PreparedInputs {
        stored_outputs,
        spent_output_keys,
        total_input,
        estimated_fee,
        fee_rate: None,
        fee_policy: None,
        further_inputs: Vec::new(),
    })
}

/// Prepare sweep inputs with a fixed tier-one fee estimate.
#[deprecated(
    note = "use prepare_sweep_inputs_with_rpc so preparation uses the construction fee tier"
)]
pub fn prepare_sweep_inputs(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    selected_output_keys: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
) -> Result<PreparedInputs, String> {
    prepare_sweep_inputs_at(
        wallet_outputs,
        wallet_height,
        current_time_secs(),
        selected_output_keys,
        excluded_key_images,
    )
}

/// Prepare a sweep using an explicit Unix clock, such as adjusted network time.
/// With no clock, timestamp-locked outputs remain locked.
#[deprecated(note = "use prepare_sweep_inputs_at_with_fee with the construction fee tier")]
pub fn prepare_sweep_inputs_at(
    wallet_outputs: &[WalletOutput],
    wallet_height: u64,
    now_secs: Option<u64>,
    selected_output_keys: Option<&[String]>,
    excluded_key_images: Option<&HashSet<String>>,
) -> Result<PreparedInputs, String> {
    let mut spendable: Vec<WalletOutput> = wallet_outputs
        .iter()
        .filter(|o| {
            !o.spent
                && !o.frozen
                && is_spendable_at(o, wallet_height, now_secs)
                && !excluded_key_images.map_or(false, |exc| exc.contains(&o.key_image))
        })
        .cloned()
        .collect();

    if let Some(keys) = selected_output_keys {
        spendable.retain(|o| keys.contains(&o.output_key()));
    }

    if spendable.is_empty() {
        return Err("No spendable outputs available for sweep".to_string());
    }

    let total_input = crate::tx_utils::checked_amount_sum(spendable.iter().map(|o| o.amount))
        .ok_or_else(|| "sweep input amounts overflow".to_string())?;
    // wallet2 adds a zero-amount dummy output to a sweep's destination.
    let estimated_fee = estimate_fee(spendable.len(), 2);
    let spent_output_keys: Vec<String> = spendable.iter().map(|o| o.output_key()).collect();
    let stored_outputs: Vec<StoredOutputData> = spendable.into_iter().map(Into::into).collect();

    Ok(PreparedInputs {
        stored_outputs,
        spent_output_keys,
        total_input,
        estimated_fee,
        fee_rate: None,
        fee_policy: None,
        further_inputs: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(deprecated)]

    use super::*;

    fn make_output(amount: u64, height: u64, tx_hash: &str) -> WalletOutput {
        WalletOutput {
            tx_hash: tx_hash.to_string(),
            output_index: 0,
            amount,
            key: "k".into(),
            key_offset: "ko".into(),
            commitment_mask: "cm".into(),
            subaddress_index: Some((0, 0)),
            payment_id: None,
            received_output_bytes: "".into(),
            global_index: None,
            ring: None,
            block_height: height,
            spent: false,
            spent_height: None,
            key_image: format!("ki_{}", tx_hash),
            is_coinbase: false,
            frozen: false,
            export_height: None,
        }
    }

    fn make_spent(amount: u64, height: u64, tx_hash: &str) -> WalletOutput {
        let mut o = make_output(amount, height, tx_hash);
        o.spent = true;
        o
    }

    fn make_coinbase(amount: u64, height: u64, tx_hash: &str) -> WalletOutput {
        let mut o = make_output(amount, height, tx_hash);
        o.is_coinbase = true;
        o
    }

    fn make_frozen(amount: u64, height: u64, tx_hash: &str) -> WalletOutput {
        let mut o = make_output(amount, height, tx_hash);
        o.frozen = true;
        o
    }

    // ---- prepare_send_inputs ----

    #[test]
    fn send_filters_out_spent_outputs() {
        let outputs = vec![
            make_output(5_000_000_000_000, 100, "tx1"),
            make_spent(3_000_000_000_000, 100, "tx2"),
        ];
        let result = prepare_send_inputs(&outputs, 200, 1_000_000_000_000, 1, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].tx_hash, "tx1");
    }

    #[test]
    fn send_filters_frozen_outputs() {
        let outputs = vec![
            make_output(5_000_000_000_000, 100, "tx1"),
            make_frozen(3_000_000_000_000, 100, "tx2"),
        ];
        let result = prepare_send_inputs(&outputs, 200, 1_000_000_000_000, 1, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].tx_hash, "tx1");
    }

    #[test]
    fn send_error_when_only_frozen() {
        let outputs = vec![make_frozen(5_000_000_000_000, 100, "tx1")];
        let result = prepare_send_inputs(&outputs, 200, 1_000_000_000_000, 1, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn send_filters_out_unconfirmed() {
        // height 105: output at 100 has only 5 confirmations (needs 10)
        let outputs = vec![make_output(5_000_000_000_000, 100, "tx1")];
        let result = prepare_send_inputs(&outputs, 105, 1_000_000_000_000, 1, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn send_includes_confirmed_outputs() {
        // height 110: output at 100 has 10 confirmations
        let outputs = vec![make_output(5_000_000_000_000, 100, "tx1")];
        let result = prepare_send_inputs(&outputs, 110, 1_000_000_000_000, 1, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
    }

    #[test]
    fn send_filters_immature_coinbase() {
        // height 150: coinbase at 100 has 50 confirmations (needs 60)
        let outputs = vec![
            make_coinbase(5_000_000_000_000, 100, "cb1"),
            make_output(2_000_000_000_000, 100, "tx1"),
        ];
        let result = prepare_send_inputs(&outputs, 150, 1_000_000_000_000, 1, None, None).unwrap();
        // Only the regular output should be selected (coinbase not spendable yet)
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].tx_hash, "tx1");
    }

    #[test]
    fn send_includes_mature_coinbase() {
        // height 160: coinbase at 100 has 60 confirmations
        let outputs = vec![make_coinbase(2_000_000_000_000, 100, "cb1")];
        let result = prepare_send_inputs(&outputs, 160, 1_000_000_000_000, 1, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
    }

    #[test]
    fn send_runs_coin_selection() {
        let outputs = vec![
            make_output(5_000_000_000_000, 100, "tx1"),
            make_output(2_000_000_000_000, 100, "tx2"),
            make_output(500_000_000_000, 100, "tx3"),
        ];
        // For 1 XMR, should pick the 2 XMR output (smallest sufficient)
        let result = prepare_send_inputs(&outputs, 200, 1_000_000_000_000, 1, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].amount, 2_000_000_000_000);
    }

    #[test]
    fn send_returns_correct_spent_keys() {
        let mut o1 = make_output(2_000_000_000_000, 100, "tx_abc");
        o1.output_index = 1;
        let outputs = vec![o1];
        let result = prepare_send_inputs(&outputs, 200, 1_000_000_000_000, 1, None, None).unwrap();
        assert_eq!(result.spent_output_keys, vec!["tx_abc:1".to_string()]);
    }

    #[test]
    fn send_converts_subaddress_field() {
        let mut o = make_output(5_000_000_000_000, 100, "tx1");
        o.subaddress_index = Some((1, 3));
        let result = prepare_send_inputs(&[o], 200, 1_000_000_000_000, 1, None, None).unwrap();
        // StoredOutputData uses `subaddress`, not `subaddress_index`
        assert_eq!(result.stored_outputs[0].subaddress, Some((1, 3)));
    }

    #[test]
    fn send_manual_selection() {
        let outputs = vec![
            make_output(1_000_000_000_000, 100, "tx1"),
            make_output(2_000_000_000_000, 100, "tx2"),
            make_output(3_000_000_000_000, 100, "tx3"),
        ];
        let keys = vec!["tx1:0".to_string(), "tx3:0".to_string()];
        let result =
            prepare_send_inputs(&outputs, 200, 500_000_000_000, 1, Some(&keys), None).unwrap();
        assert_eq!(result.stored_outputs.len(), 2);
        assert_eq!(result.total_input, 4_000_000_000_000);
    }

    #[test]
    fn send_error_when_no_spendable() {
        let outputs = vec![make_spent(5_000_000_000_000, 100, "tx1")];
        let result = prepare_send_inputs(&outputs, 200, 1_000_000_000_000, 1, None, None);
        assert!(result.is_err());
    }

    // ---- prepare_sweep_inputs ----

    #[test]
    fn sweep_returns_all_spendable() {
        let outputs = vec![
            make_output(1_000_000_000_000, 100, "tx1"),
            make_output(2_000_000_000_000, 100, "tx2"),
            make_output(3_000_000_000_000, 100, "tx3"),
        ];
        let result = prepare_sweep_inputs(&outputs, 200, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 3);
        assert_eq!(result.total_input, 6_000_000_000_000);
    }

    #[test]
    fn sweep_refuses_an_input_total_that_overflows() {
        let outputs = vec![
            make_output(u64::MAX, 100, "tx1"),
            make_output(1, 100, "tx2"),
        ];
        let error = prepare_sweep_inputs(&outputs, 200, None, None).unwrap_err();
        assert!(error.contains("overflow"), "{error}");
    }

    #[test]
    fn sweep_filters_frozen() {
        let outputs = vec![
            make_output(1_000_000_000_000, 100, "tx1"),
            make_frozen(2_000_000_000_000, 100, "tx2"),
        ];
        let result = prepare_sweep_inputs(&outputs, 200, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].tx_hash, "tx1");
    }

    #[test]
    fn sweep_error_when_only_frozen() {
        let outputs = vec![make_frozen(1_000_000_000_000, 100, "tx1")];
        let result = prepare_sweep_inputs(&outputs, 200, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn sweep_filters_spent() {
        let outputs = vec![
            make_output(1_000_000_000_000, 100, "tx1"),
            make_spent(2_000_000_000_000, 100, "tx2"),
        ];
        let result = prepare_sweep_inputs(&outputs, 200, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].tx_hash, "tx1");
    }

    #[test]
    fn sweep_filters_unconfirmed() {
        let outputs = vec![
            make_output(1_000_000_000_000, 100, "tx1"),
            make_output(2_000_000_000_000, 108, "tx2"), // only 2 confirmations at height 110
        ];
        let result = prepare_sweep_inputs(&outputs, 110, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].tx_hash, "tx1");
    }

    #[test]
    fn sweep_filters_by_output_keys() {
        let outputs = vec![
            make_output(1_000_000_000_000, 100, "tx1"),
            make_output(2_000_000_000_000, 100, "tx2"),
            make_output(3_000_000_000_000, 100, "tx3"),
        ];
        let keys = vec!["tx1:0".to_string(), "tx3:0".to_string()];
        let result = prepare_sweep_inputs(&outputs, 200, Some(&keys), None).unwrap();
        assert_eq!(result.stored_outputs.len(), 2);
        assert_eq!(result.total_input, 4_000_000_000_000);
    }

    #[test]
    fn sweep_error_when_empty() {
        let outputs = vec![make_spent(1_000_000_000_000, 100, "tx1")];
        let result = prepare_sweep_inputs(&outputs, 200, None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No spendable outputs"));
    }

    #[test]
    fn sweep_returns_correct_spent_keys() {
        let mut o1 = make_output(1_000_000_000_000, 100, "tx_a");
        o1.output_index = 0;
        let mut o2 = make_output(2_000_000_000_000, 100, "tx_b");
        o2.output_index = 2;
        let result = prepare_sweep_inputs(&[o1, o2], 200, None, None).unwrap();
        assert_eq!(
            result.spent_output_keys,
            vec!["tx_a:0".to_string(), "tx_b:2".to_string()]
        );
    }

    #[test]
    fn sweep_fee_scales_with_output_count() {
        let outputs = vec![
            make_output(1_000_000_000_000, 100, "tx1"),
            make_output(2_000_000_000_000, 100, "tx2"),
        ];
        let result = prepare_sweep_inputs(&outputs, 200, None, None).unwrap();
        assert_eq!(result.estimated_fee, estimate_fee(2, 2));
    }

    #[test]
    fn fee_aware_sweep_uses_two_outputs_and_one_account() {
        let mut account_one = make_output(2_000_000_000_000, 100, "tx1");
        account_one.subaddress_index = Some((1, 0));
        let outputs = vec![make_output(1_000_000_000_000, 100, "tx0"), account_one];
        let rct_type = monero_wallet::ringct::RctType::ClsagBulletproofPlus;
        let fee_rate = monero_wallet::interface::FeeRate::new(80_000, 10_000).unwrap();
        let result =
            prepare_sweep_inputs_with_fee(&outputs, 200, None, None, 1, rct_type, fee_rate)
                .unwrap();

        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].subaddress, Some((1, 0)));
        assert_eq!(
            result.estimated_fee,
            estimate_fee_with_rate(1, 2, rct_type, fee_rate).unwrap()
        );
    }

    fn sweep_fixture() -> Vec<WalletOutput> {
        [(0, "a"), (3, "b"), (3, "c"), (7, "d")]
            .into_iter()
            .map(|(minor, tx)| {
                let mut output = make_output(1_000_000_000_000, 100, tx);
                output.subaddress_index = Some((0, minor));
                output
            })
            .collect()
    }

    fn swept_from(
        seed: u64,
        outputs: &[WalletOutput],
        subaddresses: SweepSubaddresses<'_>,
    ) -> Vec<u32> {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut minors: Vec<u32> = prepare_sweep_inputs_from_subaddresses(
            &mut rng,
            outputs,
            200,
            Some(0),
            subaddresses,
            None,
            0,
            monero_wallet::ringct::RctType::ClsagBulletproofPlus,
            monero_wallet::interface::FeeRate::new(20_000, 10_000).unwrap(),
        )
        .unwrap()
        .stored_outputs
        .iter()
        .map(|stored| stored.subaddress.unwrap().1)
        .collect();
        minors.sort_unstable();
        minors
    }

    #[test]
    fn sweep_of_one_subaddress_at_random_leaves_the_first_address_for_last() {
        let outputs = sweep_fixture();
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..64 {
            seen.insert(swept_from(seed, &outputs, SweepSubaddresses::OneAtRandom));
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), [vec![3, 3], vec![7]]);

        assert_eq!(
            swept_from(1, &outputs[..1], SweepSubaddresses::OneAtRandom),
            [0]
        );
    }

    #[test]
    fn sweep_of_named_or_all_subaddresses() {
        let outputs = sweep_fixture();
        assert_eq!(
            swept_from(1, &outputs, SweepSubaddresses::Only(&[0, 7])),
            [0, 7]
        );
        assert_eq!(
            swept_from(1, &outputs, SweepSubaddresses::All),
            [0, 3, 3, 7]
        );
    }

    #[test]
    fn a_large_sweep_is_split_into_full_transactions() {
        use rand::SeedableRng;
        let outputs: Vec<WalletOutput> = (0..300u64)
            .map(|i| make_output(1_000_000_000_000, 100 + i, &format!("tx{i}")))
            .collect();
        let rct_type = monero_wallet::ringct::RctType::ClsagBulletproofPlus;
        let fee_rate = monero_wallet::interface::FeeRate::new(20_000, 10_000).unwrap();
        let mut rng = rand::rngs::StdRng::seed_from_u64(5);
        let transactions = prepare_sweep_transactions(
            &mut rng,
            &outputs,
            1_000,
            Some(0),
            SweepSubaddresses::All,
            None,
            0,
            rct_type,
            fee_rate,
        )
        .unwrap();
        let sizes: Vec<usize> = transactions
            .iter()
            .map(|transaction| transaction.stored_outputs.len())
            .collect();
        assert_eq!(sizes, [146, 146, 8]);
        let mut keys: Vec<&String> = transactions
            .iter()
            .flat_map(|transaction| &transaction.spent_output_keys)
            .collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 300);

        let one = |count: usize| {
            prepare_sweep_inputs_at_with_fee(
                &outputs[..count],
                1_000,
                Some(0),
                None,
                None,
                0,
                rct_type,
                fee_rate,
            )
        };
        assert_eq!(one(146).unwrap().stored_outputs.len(), 146);
        assert!(one(147).unwrap_err().contains("prepare_sweep_transactions"));
    }

    #[test]
    fn fee_aware_sweep_refuses_inputs_that_cannot_pay_the_fee() {
        let outputs = vec![make_output(100_000_000, 100, "tx0")];
        let error = prepare_sweep_inputs_with_fee(
            &outputs,
            200,
            None,
            None,
            0,
            monero_wallet::ringct::RctType::ClsagBulletproofPlus,
            monero_wallet::interface::FeeRate::new(80_000, 10_000).unwrap(),
        )
        .unwrap_err();
        assert!(error.contains("cannot pay"), "{error}");
    }

    #[test]
    fn sweep_immature_coinbase_excluded() {
        let outputs = vec![
            make_coinbase(5_000_000_000_000, 100, "cb1"),
            make_output(1_000_000_000_000, 100, "tx1"),
        ];
        // Only 50 confirmations for coinbase (needs 60)
        let result = prepare_sweep_inputs(&outputs, 150, None, None).unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].tx_hash, "tx1");
    }

    // ---- excluded_key_images tests ----

    #[test]
    fn send_excludes_pending_key_images() {
        let outputs = vec![
            make_output(5_000_000_000_000, 100, "tx1"),
            make_output(2_000_000_000_000, 100, "tx2"),
        ];
        let excluded: HashSet<String> = ["ki_tx2".to_string()].into();
        let result =
            prepare_send_inputs(&outputs, 200, 1_000_000_000_000, 1, None, Some(&excluded))
                .unwrap();
        assert_eq!(result.stored_outputs.len(), 1);
        assert_eq!(result.stored_outputs[0].tx_hash, "tx1");
    }

    #[test]
    fn sweep_excludes_pending_key_images() {
        let outputs = vec![
            make_output(5_000_000_000_000, 100, "tx1"),
            make_output(2_000_000_000_000, 100, "tx2"),
            make_output(1_000_000_000_000, 100, "tx3"),
        ];
        let excluded: HashSet<String> = ["ki_tx2".to_string()].into();
        let result = prepare_sweep_inputs(&outputs, 200, None, Some(&excluded)).unwrap();
        assert_eq!(result.stored_outputs.len(), 2);
        assert_eq!(result.total_input, 6_000_000_000_000); // tx1 + tx3
    }
}

fn with_policy(mut prepared: PreparedInputs, policy: FeePolicy, fee: Fee) -> PreparedInputs {
    prepared.fee_policy = Some(policy);
    prepared.fee_rate = Some(fee);
    prepared
}
