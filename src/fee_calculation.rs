use crate::{WalletError, TransactionPriority};
use monero_wallet::rpc::{Rpc, FeeRate};

pub struct WeightEstimator {
    pub ring_size: usize,
    pub num_inputs: usize,
    pub num_outputs: usize,
    pub has_payment_id: bool,
    pub use_bulletproofs_plus: bool,
}

impl WeightEstimator {
    pub fn new(num_inputs: usize, num_outputs: usize) -> Self {
        Self {
            ring_size: 16,
            num_inputs,
            num_outputs,
            has_payment_id: false,
            use_bulletproofs_plus: true,
        }
    }

    pub fn estimate_weight(&self) -> usize {
        let base_weight = 90;

        // per-input: key image (32) + ring offsets + CLSAG sig (~200)
        let avg_offset_bytes = 5;
        let ring_offset_size = self.ring_size.saturating_mul(avg_offset_bytes);
        let per_input_weight = 32usize
            .saturating_add(ring_offset_size)
            .saturating_add(200);
        let input_weight = self.num_inputs.saturating_mul(per_input_weight);

        // per-output: pubkey (32) + encrypted commitment (32) + tag (1)
        let output_weight = self.num_outputs * 65;

        // extra: tx pubkey (33) + optional payment id (33)
        let extra_weight = 33 + if self.has_payment_id { 33 } else { 0 };

        let range_proof_weight = if self.num_outputs == 0 {
            0
        } else if self.use_bulletproofs_plus {
            let log_outputs = (self.num_outputs as f64).log2().ceil() as usize;
            100usize
                .saturating_add(self.num_outputs.saturating_mul(128))
                .saturating_add(log_outputs.saturating_mul(32))
        } else {
            let log_outputs = (self.num_outputs as f64).log2().ceil() as usize;
            150usize
                .saturating_add(self.num_outputs.saturating_mul(160))
                .saturating_add(log_outputs.saturating_mul(64))
        };

        let total_size = base_weight + input_weight + output_weight + extra_weight + range_proof_weight;

        // bulletproof clawback adjustment
        let clawback = if self.use_bulletproofs_plus {
            (self.num_outputs * 16).saturating_sub(128)
        } else {
            (self.num_outputs * 32).saturating_sub(256)
        };

        total_size + clawback
    }

    pub fn estimate_sweep_weight(num_inputs: usize, num_destinations: usize) -> usize {
        Self::new(num_inputs, num_destinations).estimate_weight()
    }
}

pub fn estimate_fee(
    num_inputs: usize,
    num_destinations: usize,
    fee_rate: &FeeRate,
    has_payment_id: bool,
) -> u64 {
    let num_outputs = num_destinations + 1; // +1 for change
    let mut estimator = WeightEstimator::new(num_inputs, num_outputs);
    estimator.has_payment_id = has_payment_id;
    fee_rate.calculate_fee_from_weight(estimator.estimate_weight())
}

pub fn estimate_sweep_fee(
    num_inputs: usize,
    num_destinations: usize,
    fee_rate: &FeeRate,
) -> u64 {
    let weight = WeightEstimator::estimate_sweep_weight(num_inputs, num_destinations);
    fee_rate.calculate_fee_from_weight(weight)
}

pub async fn get_fee_rate_for_priority(
    rpc: &impl Rpc,
    priority: TransactionPriority,
) -> Result<FeeRate, WalletError> {
    rpc.get_fee_rate(priority.to_fee_priority())
        .await
        .map_err(WalletError::RpcError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weight_estimation_basic() {
        let estimator = WeightEstimator::new(2, 2);
        let weight = estimator.estimate_weight();
        assert!(weight > 1000 && weight < 10000, "weight {} out of range", weight);
    }

    #[test]
    fn test_weight_scales_with_inputs() {
        let w1 = WeightEstimator::new(1, 2).estimate_weight();
        let w2 = WeightEstimator::new(2, 2).estimate_weight();
        let w5 = WeightEstimator::new(5, 2).estimate_weight();
        assert!(w2 > w1);
        assert!(w5 > w2);
    }

    #[test]
    fn test_bulletproofs_plus_smaller() {
        let mut bp = WeightEstimator::new(2, 2);
        bp.use_bulletproofs_plus = false;
        let mut bp_plus = WeightEstimator::new(2, 2);
        bp_plus.use_bulletproofs_plus = true;
        assert!(bp_plus.estimate_weight() < bp.estimate_weight());
    }

    #[test]
    fn test_payment_id_adds_weight() {
        let mut without = WeightEstimator::new(2, 2);
        without.has_payment_id = false;
        let mut with = WeightEstimator::new(2, 2);
        with.has_payment_id = true;
        assert_eq!(with.estimate_weight() - without.estimate_weight(), 33);
    }
}
