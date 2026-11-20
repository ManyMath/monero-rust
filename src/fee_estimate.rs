//! Transaction weight and fee estimates made before a transaction exists.

use monero_wallet::{interface::FeeRate, ringct::RctType};

use crate::unsigned_tx::ring_len;

const BP_LOG_N: usize = 6;
const MAX_OUTPUTS: usize = 16;
const MAX_U64_VARINT_LEN: usize = 10;

fn varint_len(varint: usize) -> usize {
    ((usize::try_from(usize::BITS - varint.leading_zeros())
        .expect("bit count exceeds usize")
        .saturating_sub(1))
        / 7)
        + 1
}

fn input_weight(ring_len: usize) -> usize {
    // 1 byte VarInt amount (0), 1 byte input type, 1 byte ring length
    1 + 1 + 1 + (MAX_U64_VARINT_LEN * ring_len) + 32
}

fn output_weight() -> usize {
    1 + 1 + 32 + 1
}

fn prefix_weight(ring_len: usize, inputs: usize, outputs: usize, extra: usize) -> Option<usize> {
    // Assumes Timelock::None
    inputs.checked_mul(input_weight(ring_len))?.checked_add(
        1 + 1 + varint_len(inputs) + 1 + (outputs * output_weight()) + varint_len(extra) + extra,
    )
}

fn clsag_weight(ring_len: usize) -> usize {
    (ring_len * 32) + 32 + 32
}

fn bulletproofs_weight(plus: bool, outputs: usize) -> usize {
    let fields = if plus { 6 } else { 9 };

    #[allow(non_snake_case)]
    let mut LR_len = usize::try_from(usize::BITS - (outputs - 1).leading_zeros())
        .expect("bit count exceeds usize");
    let padded_outputs = 1 << LR_len;
    LR_len += BP_LOG_N;

    let len = (fields + (2 * LR_len)) * 32;
    len + if padded_outputs <= 2 {
        0
    } else {
        let base = ((fields + (2 * (BP_LOG_N + 1))) * 32) / 2;
        let size = (fields + (2 * LR_len)) * 32;
        ((base * padded_outputs) - size) * 4 / 5
    }
}

fn rct_base_weight(outputs: usize) -> usize {
    1 + MAX_U64_VARINT_LEN + (outputs * (8 + 32))
}

/// The largest `extra` a transaction with `outputs` outputs can carry.
pub fn extra_len_upper_bound(outputs: usize, payment_id: bool) -> usize {
    // PublicKey, key
    (1 + 32) +
        // PublicKeys, length, additional keys
        (1 + 1 + (outputs * 32)) +
        // PaymentId (Nonce), length, encrypted, ID
        (if payment_id { 1 + 1 + 1 + 8 } else { 0 })
}

/// The weight bound for this many inputs and outputs, with an encrypted
/// payment ID if `payment_id`.
pub fn transaction_weight_upper_bound(
    rct_type: RctType,
    inputs: usize,
    outputs: usize,
    payment_id: bool,
) -> Result<usize, String> {
    let ring_len =
        usize::from(ring_len(rct_type).ok_or_else(|| "Unsupported RingCT type".to_string())?);
    let plus = matches!(rct_type, RctType::ClsagBulletproofPlus);
    if inputs == 0 || outputs == 0 {
        return Err("A transaction has at least one input and one output".to_string());
    }
    if outputs > MAX_OUTPUTS {
        return Err(format!("A transaction has at most {MAX_OUTPUTS} outputs"));
    }
    let extra = extra_len_upper_bound(outputs, payment_id);
    prefix_weight(ring_len, inputs, outputs, extra)
        .zip(inputs.checked_mul(clsag_weight(ring_len) + 32))
        .and_then(|(prefix, signatures)| prefix.checked_add(signatures))
        .and_then(|weight| {
            weight.checked_add(rct_base_weight(outputs) + 1 + bulletproofs_weight(plus, outputs))
        })
        .ok_or_else(|| "weight exceeds usize".to_string())
}

/// wallet2's `estimate_tx_weight`. Not a bound: only for making wallet2's
/// selection decisions.
pub fn wallet2_weight_estimate(
    rct_type: RctType,
    inputs: usize,
    outputs: usize,
) -> Result<usize, String> {
    let ring = u128::from(ring_len(rct_type).ok_or_else(|| "Unsupported RingCT type".to_string())?);
    let plus = matches!(rct_type, RctType::ClsagBulletproofPlus);
    if inputs == 0 || outputs == 0 {
        return Err("A transaction has at least one input and one output".to_string());
    }
    let inputs = u128::try_from(inputs).map_err(|_| "input count exceeds u128".to_string())?;
    let outputs = u128::try_from(outputs).map_err(|_| "output count exceeds u128".to_string())?;
    let mut log_padded_outputs = 0u32;
    while (1u128 << log_padded_outputs) < outputs {
        log_padded_outputs += 1;
    }
    let proof_fields: u128 = if plus { 6 } else { 4 + 5 };

    let mut size = 1 + 6;
    size += inputs * (1 + 6 + (ring * 2) + 32);
    size += outputs * (6 + 32);
    size += 1;
    size += ((2 * (6 + u128::from(log_padded_outputs))) + proof_fields) * 32 + 3;
    size += inputs * ((32 * ring) + 64);
    if plus {
        size += outputs;
    }
    size += 32 * inputs;
    size += 8 * outputs;
    size += 32 * outputs;
    size += 4;

    if outputs > 2 {
        let log_padded_outputs = log_padded_outputs.max(2);
        let base = (32 * (proof_fields + (7 * 2))) / 2;
        let proof = 32 * (proof_fields + (2 * (6 + u128::from(log_padded_outputs))));
        size += ((base * (1u128 << log_padded_outputs)) - proof) * 4 / 5;
    }
    usize::try_from(size).map_err(|_| "weight exceeds usize".to_string())
}

/// `get_transaction_weight_limit` since hard fork 8.
pub(crate) const TRANSACTION_WEIGHT_LIMIT: usize = (300_000 / 2) - 600;

/// wallet2's `TX_WEIGHT_TARGET`: the estimated weight at which it closes a
/// transaction.
pub const WALLET2_TX_WEIGHT_TARGET: usize = TRANSACTION_WEIGHT_LIMIT * 2 / 3;

/// Whether wallet2 would put no further input into a transaction of this
/// shape, by its own weight estimate.
pub fn wallet2_transaction_is_full(
    rct_type: RctType,
    inputs: usize,
    outputs: usize,
) -> Result<bool, String> {
    Ok(wallet2_weight_estimate(rct_type, inputs, outputs)? >= WALLET2_TX_WEIGHT_TARGET)
}

/// Refuse inputs that cannot be one transaction, judged as CLSAG with
/// Bulletproofs+, before any decoy is asked for.
pub(crate) fn check_one_transaction(
    inputs: usize,
    outputs: usize,
    sweep: bool,
) -> Result<(), String> {
    let rct_type = RctType::ClsagBulletproofPlus;
    if transaction_weight_upper_bound(rct_type, inputs, outputs, true)? >= TRANSACTION_WEIGHT_LIMIT
    {
        return Err(format!(
            "{inputs} inputs and {outputs} outputs do not fit in one transaction"
        ));
    }
    let before_last = if sweep { inputs - 1 } else { inputs };
    if before_last > 0 && wallet2_transaction_is_full(rct_type, before_last, outputs)? {
        return Err(if sweep {
            format!(
                "This sweep has {inputs} inputs, more than wallet2 puts in one transaction; \
                 prepare_sweep_transactions splits it the way wallet2 does"
            )
        } else {
            format!(
                "This payment has {inputs} inputs, more than wallet2 puts in one transaction. \
                 wallet2 would split it over several transactions, which is not supported"
            )
        });
    }
    Ok(())
}

/// The fee `fee_rate` asks of a transaction weighing `weight`.
pub fn fee_for_weight(fee_rate: FeeRate, weight: usize) -> Result<u64, String> {
    let weight = u64::try_from(weight).map_err(|_| "weight exceeds u64".to_string())?;
    u64::try_from(fee_rate.calculate_fee_from_weight(weight))
        .map_err(|_| "fee exceeds u64".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_len_counts_seven_bits_per_byte() {
        assert_eq!(varint_len(0), 1);
        assert_eq!(varint_len(0x7f), 1);
        assert_eq!(varint_len(0x80), 2);
        assert_eq!(varint_len(0x3fff), 2);
        assert_eq!(varint_len(0x4000), 3);
        assert_eq!(varint_len(0x1fffff), 3);
        assert_eq!(varint_len(0x200000), 4);
        assert_eq!(varint_len(0xfffffff), 4);
        assert_eq!(varint_len(0x10000000), 5);
    }

    #[test]
    fn extra_bound_without_payment_id() {
        assert_eq!(extra_len_upper_bound(2, false), 99);
    }

    #[test]
    fn extra_bound_with_payment_id() {
        assert_eq!(extra_len_upper_bound(2, true), 110);
        assert_eq!(extra_len_upper_bound(16, true), 558);
    }

    #[test]
    fn weight_bounds_cover_maximum_width_u64_varints() {
        let plus = RctType::ClsagBulletproofPlus;
        let original = RctType::ClsagBulletproof;
        for (rct_type, inputs, outputs, payment_id, weight) in [
            (plus, 1, 2, true, 1720),
            (plus, 2, 2, true, 2523),
            (plus, 1, 3, false, 2341),
            (plus, 10, 16, true, 14068),
            (plus, 200, 2, true, 161518),
            (original, 1, 2, true, 1606),
            (original, 2, 2, true, 2199),
            (original, 1, 3, false, 2304),
            (original, 10, 16, true, 12602),
            (original, 200, 2, true, 119614),
        ] {
            assert_eq!(
                transaction_weight_upper_bound(rct_type, inputs, outputs, payment_id),
                Ok(weight),
                "{rct_type:?} {inputs} in, {outputs} out"
            );
        }
    }

    #[test]
    fn wallet2_estimate_follows_wallet2() {
        let plus = RctType::ClsagBulletproofPlus;
        let original = RctType::ClsagBulletproof;
        for (rct_type, inputs, outputs, weight) in [
            (plus, 1, 2, 1492),
            (plus, 2, 2, 2171),
            (plus, 146, 2, 99_947),
            (plus, 1, 3, 2095),
            (plus, 1, 16, 6220),
            (original, 1, 2, 1416),
            (original, 2, 2, 1925),
        ] {
            assert_eq!(
                wallet2_weight_estimate(rct_type, inputs, outputs),
                Ok(weight),
                "{rct_type:?} {inputs} in, {outputs} out"
            );
        }
        assert!(wallet2_weight_estimate(plus, 0, 2).is_err());
        assert!(wallet2_weight_estimate(plus, 1, 0).is_err());
        assert!(wallet2_weight_estimate(RctType::MlsagBulletproofs, 1, 2).is_err());
        assert!(wallet2_weight_estimate(plus, usize::MAX, usize::MAX).is_err());
    }

    #[test]
    fn a_two_output_transaction_is_full_at_146_inputs() {
        let plus = RctType::ClsagBulletproofPlus;
        assert_eq!(WALLET2_TX_WEIGHT_TARGET, 99_600);
        assert_eq!(wallet2_transaction_is_full(plus, 145, 2), Ok(false));
        assert_eq!(wallet2_transaction_is_full(plus, 146, 2), Ok(true));
    }

    #[test]
    fn construction_refuses_what_cannot_be_one_transaction() {
        assert_eq!(TRANSACTION_WEIGHT_LIMIT, 149_400);
        assert!(check_one_transaction(145, 2, false).is_ok());
        assert!(check_one_transaction(146, 2, false).is_err());
        assert!(check_one_transaction(146, 2, true).is_ok());
        assert!(check_one_transaction(147, 2, true).is_err());
        assert!(check_one_transaction(1, 16, false).is_ok());
        assert!(check_one_transaction(usize::MAX, 2, true).is_err());
        let plus = RctType::ClsagBulletproofPlus;
        let first_over_limit = (1..)
            .find(|inputs| {
                transaction_weight_upper_bound(plus, *inputs, 2, true).unwrap()
                    >= TRANSACTION_WEIGHT_LIMIT
            })
            .unwrap();
        assert!(first_over_limit > 146);
        let error = check_one_transaction(first_over_limit, 2, true).unwrap_err();
        assert!(error.contains("do not fit"), "{error}");
    }

    #[test]
    fn weight_bound_refuses_shapes_no_transaction_has() {
        let plus = RctType::ClsagBulletproofPlus;
        assert!(transaction_weight_upper_bound(plus, 0, 2, true).is_err());
        assert!(transaction_weight_upper_bound(plus, 1, 0, true).is_err());
        assert!(transaction_weight_upper_bound(RctType::MlsagBulletproofs, 1, 2, true).is_err());
        assert!(transaction_weight_upper_bound(plus, 1, 17, true).is_err());
        assert!(transaction_weight_upper_bound(plus, 1, usize::MAX, true).is_err());
        assert!(transaction_weight_upper_bound(plus, usize::MAX, 2, true).is_err());
        assert!(transaction_weight_upper_bound(plus, usize::MAX / 800, 2, true).is_err());
    }

    #[test]
    fn fee_rounds_up_to_the_mask() {
        let fee_rate = FeeRate::new(8000, 10000).unwrap();
        assert_eq!(
            fee_for_weight(fee_rate, 1500),
            Ok(((8000u64 * 1500 - 1) / 10000 + 1) * 10000)
        );
        assert_eq!(fee_for_weight(fee_rate, 0), Ok(0));
        assert!(fee_for_weight(FeeRate::new(u64::MAX, 1).unwrap(), 2).is_err());
    }
}
