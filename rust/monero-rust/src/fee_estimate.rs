//! Transaction weight and fee estimates made before a transaction exists.

use monero_wallet::{interface::FeeRate, ringct::RctType};

use crate::unsigned_tx::ring_len;

const BP_LOG_N: usize = 6;

fn varint_len(varint: usize) -> usize {
    ((usize::try_from(usize::BITS - varint.leading_zeros())
        .expect("bit count exceeds usize")
        .saturating_sub(1))
        / 7)
        + 1
}

fn input_weight(ring_len: usize) -> usize {
    // 1 byte VarInt amount (0), 1 byte input type, 1 byte ring length
    1 + 1 + 1 + (8 * ring_len) + 32
}

fn output_weight() -> usize {
    1 + 1 + 32 + 1
}

fn prefix_weight(ring_len: usize, inputs: usize, outputs: usize, extra: usize) -> usize {
    // Assumes Timelock::None
    1 + 1
        + varint_len(inputs)
        + (inputs * input_weight(ring_len))
        + 1
        + (outputs * output_weight())
        + varint_len(extra)
        + extra
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
    1 + 8 + (outputs * (8 + 32))
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
    let extra = extra_len_upper_bound(outputs, payment_id);
    Ok(prefix_weight(ring_len, inputs, outputs, extra)
        + rct_base_weight(outputs)
        + 1
        + bulletproofs_weight(plus, outputs)
        + (inputs * (clsag_weight(ring_len) + 32)))
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
    fn weight_bounds_are_unchanged() {
        let plus = RctType::ClsagBulletproofPlus;
        let original = RctType::ClsagBulletproof;
        for (rct_type, inputs, outputs, payment_id, weight) in [
            (plus, 1, 2, true, 1686),
            (plus, 2, 2, true, 2457),
            (plus, 1, 3, false, 2307),
            (plus, 10, 16, true, 13746),
            (plus, 200, 2, true, 155116),
            (original, 1, 2, true, 1582),
            (original, 2, 2, true, 2153),
            (original, 1, 3, false, 2280),
            (original, 10, 16, true, 12380),
            (original, 200, 2, true, 115212),
        ] {
            assert_eq!(
                transaction_weight_upper_bound(rct_type, inputs, outputs, payment_id),
                Ok(weight),
                "{rct_type:?} {inputs} in, {outputs} out"
            );
        }
    }

    #[test]
    fn weight_bound_refuses_shapes_no_transaction_has() {
        let plus = RctType::ClsagBulletproofPlus;
        assert!(transaction_weight_upper_bound(plus, 0, 2, true).is_err());
        assert!(transaction_weight_upper_bound(plus, 1, 0, true).is_err());
        assert!(transaction_weight_upper_bound(RctType::MlsagBulletproofs, 1, 2, true).is_err());
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
