use std::collections::HashMap;

use crate::wallet_output::WalletOutput;

const CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE: u64 = 10;
const CRYPTONOTE_MINED_MONEY_UNLOCK_WINDOW: u64 = 60;

/// Balance breakdown for a wallet or account.
#[derive(Debug, Clone, Default)]
pub struct Balance {
    pub confirmed: u64,
    pub unconfirmed: u64,
}

/// Core wallet state manager.
///
/// Holds outputs, tracks heights, and provides balance/spendability queries.
/// Applications supply the adapters; wallet lifecycle rules stay here.
pub struct WalletState {
    outputs: Vec<WalletOutput>,
    /// Maps key_image -> index in outputs vec for O(1) lookups
    key_image_index: HashMap<String, usize>,
    pub current_height: u64,
    pub daemon_height: u64,
}

impl WalletState {
    pub fn new() -> Self {
        WalletState {
            outputs: Vec::new(),
            key_image_index: HashMap::new(),
            current_height: 0,
            daemon_height: 0,
        }
    }

    /// Extend outputs (used when scanning finds new outputs).
    pub fn add_outputs(&mut self, new_outputs: Vec<WalletOutput>) {
        let base = self.outputs.len();
        for (i, output) in new_outputs.into_iter().enumerate() {
            if output.block_height > self.current_height {
                self.current_height = output.block_height;
            }
            self.key_image_index
                .insert(output.key_image.clone(), base + i);
            self.outputs.push(output);
        }
    }

    /// Replace all outputs (used when restoring from persistence).
    pub fn replace_outputs(&mut self, outputs: Vec<WalletOutput>) {
        self.outputs = outputs;
        self.rebuild_key_image_index();
    }

    /// Mark outputs as spent by matching key images.
    /// Returns the number of outputs newly marked as spent.
    pub fn mark_spent_by_key_images(&mut self, key_images: &[String]) -> usize {
        let mut count = 0;
        for ki in key_images {
            if let Some(&idx) = self.key_image_index.get(ki) {
                if !self.outputs[idx].spent {
                    self.outputs[idx].spent = true;
                    count += 1;
                }
            }
        }
        count
    }

    /// Mark outputs as spent by output keys ("txHash:outputIndex" format).
    /// Returns the number of outputs newly marked as spent.
    pub fn mark_spent_by_output_keys(&mut self, output_keys: &[String]) -> usize {
        let mut count = 0;
        for output in &mut self.outputs {
            if !output.spent {
                let key = output.output_key();
                if output_keys.contains(&key) {
                    output.spent = true;
                    count += 1;
                }
            }
        }
        count
    }

    /// Calculate balance using current_height for confirmation depth.
    pub fn balance(&self) -> Balance {
        self.balance_at_height(self.current_height)
    }

    /// Calculate balance at a specific height.
    pub fn balance_at_height(&self, height: u64) -> Balance {
        let mut bal = Balance::default();
        for output in &self.outputs {
            if output.spent {
                continue;
            }
            let confirmations = height.saturating_sub(output.block_height);
            let required = if output.is_coinbase {
                CRYPTONOTE_MINED_MONEY_UNLOCK_WINDOW
            } else {
                CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE
            };
            if output.block_height > 0 && confirmations >= required {
                bal.confirmed += output.amount;
            } else {
                bal.unconfirmed += output.amount;
            }
        }
        bal
    }

    /// Get all spendable (confirmed, unspent) outputs.
    pub fn spendable_outputs(&self) -> Vec<&WalletOutput> {
        self.spendable_outputs_at_height(self.daemon_height)
    }

    /// Get spendable outputs at a specific height.
    pub fn spendable_outputs_at_height(&self, height: u64) -> Vec<&WalletOutput> {
        self.outputs
            .iter()
            .filter(|o| !o.spent && is_spendable(o, height))
            .collect()
    }

    /// Get spendable outputs filtered to specific accounts.
    pub fn spendable_outputs_for_accounts(&self, accounts: &[u32]) -> Vec<&WalletOutput> {
        self.spendable_outputs()
            .into_iter()
            .filter(|o| {
                let account = o.subaddress_index.map(|(a, _)| a).unwrap_or(0);
                accounts.contains(&account)
            })
            .collect()
    }

    /// Get all outputs (including spent).
    pub fn outputs(&self) -> &[WalletOutput] {
        &self.outputs
    }

    /// Get mutable access to outputs.
    pub fn outputs_mut(&mut self) -> &mut Vec<WalletOutput> {
        &mut self.outputs
    }

    fn rebuild_key_image_index(&mut self) {
        self.key_image_index.clear();
        for (i, output) in self.outputs.iter().enumerate() {
            self.key_image_index
                .insert(output.key_image.clone(), i);
        }
    }
}

fn is_spendable(output: &WalletOutput, height: u64) -> bool {
    // The scanner uses zero for unconfirmed pool outputs, not their age.
    if output.block_height == 0 {
        return false;
    }
    let confirmations = height.saturating_sub(output.block_height);
    let required = if output.is_coinbase {
        CRYPTONOTE_MINED_MONEY_UNLOCK_WINDOW
    } else {
        CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE
    };
    confirmations >= required
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output(amount: u64, height: u64, key_image: &str) -> WalletOutput {
        WalletOutput {
            tx_hash: format!("tx_{}", key_image),
            output_index: 0,
            amount,
            amount_xmr: format!("{:.12}", amount as f64 / 1_000_000_000_000.0),
            key: "k".into(),
            key_offset: "ko".into(),
            commitment_mask: "cm".into(),
            subaddress_index: Some((0, 0)),
            payment_id: None,
            received_output_bytes: "".into(),
            block_height: height,
            spent: false,
            key_image: key_image.into(),
            is_coinbase: false,
        }
    }

    fn make_coinbase_output(amount: u64, height: u64, key_image: &str) -> WalletOutput {
        let mut o = make_output(amount, height, key_image);
        o.is_coinbase = true;
        o
    }

    fn make_account_output(
        amount: u64,
        height: u64,
        key_image: &str,
        account: u32,
    ) -> WalletOutput {
        let mut o = make_output(amount, height, key_image);
        o.subaddress_index = Some((account, 0));
        o
    }

    // ---- Balance tests ----

    #[test]
    fn test_empty_state_zero_balance() {
        let state = WalletState::new();
        let bal = state.balance();
        assert_eq!(bal.confirmed, 0);
        assert_eq!(bal.unconfirmed, 0);
    }

    #[test]
    fn test_confirmed_balance_after_10_blocks() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);
        state.current_height = 110;

        let bal = state.balance();
        assert_eq!(bal.confirmed, 1_000_000_000_000);
        assert_eq!(bal.unconfirmed, 0);
    }

    #[test]
    fn test_unconfirmed_balance_under_10_blocks() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);
        state.current_height = 105;

        let bal = state.balance();
        assert_eq!(bal.confirmed, 0);
        assert_eq!(bal.unconfirmed, 1_000_000_000_000);
    }

    #[test]
    fn test_coinbase_needs_60_confirmations() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_coinbase_output(5_000_000_000_000, 100, "ki1")]);

        // At 50 confirmations: still unconfirmed
        state.current_height = 150;
        let bal = state.balance();
        assert_eq!(bal.confirmed, 0);
        assert_eq!(bal.unconfirmed, 5_000_000_000_000);

        // At 60 confirmations: confirmed
        state.current_height = 160;
        let bal = state.balance();
        assert_eq!(bal.confirmed, 5_000_000_000_000);
        assert_eq!(bal.unconfirmed, 0);
    }

    #[test]
    fn mempool_outputs_remain_unconfirmed_and_unselectable() {
        for is_coinbase in [false, true] {
            let mut pool = make_output(1_000_000_000_000, 0, "pool");
            pool.is_coinbase = is_coinbase;
            let mut state = WalletState::new();
            state.add_outputs(vec![pool.clone()]);
            for height in [10, 60, 3_000_000, u64::MAX] {
                assert!(!is_spendable(&pool, height));
                assert_eq!(
                    state.balance_at_height(height).unconfirmed,
                    pool.amount
                );
                assert_eq!(state.balance_at_height(height).confirmed, 0);
                assert!(state
                    .spendable_outputs_at_height(height)
                    .is_empty());
            }
        }
    }

    #[test]
    fn test_spent_outputs_excluded_from_balance() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 100, "ki1"),
            make_output(2_000_000_000_000, 100, "ki2"),
        ]);
        state.current_height = 200;

        // Mark one as spent
        state.mark_spent_by_key_images(&["ki1".to_string()]);

        let bal = state.balance();
        assert_eq!(bal.confirmed, 2_000_000_000_000);
    }

    #[test]
    fn test_mixed_confirmed_and_unconfirmed() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"),  // 20 confs -> confirmed
            make_output(2_000_000_000_000, 95, "ki2"),  // 5 confs -> unconfirmed
        ]);
        state.current_height = 100;

        let bal = state.balance();
        assert_eq!(bal.confirmed, 1_000_000_000_000);
        assert_eq!(bal.unconfirmed, 2_000_000_000_000);
    }

    // ---- Spent marking tests ----

    #[test]
    fn test_mark_spent_by_key_images() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 100, "ki1"),
            make_output(2_000_000_000_000, 100, "ki2"),
            make_output(3_000_000_000_000, 100, "ki3"),
        ]);

        let count = state.mark_spent_by_key_images(&["ki2".to_string()]);
        assert_eq!(count, 1);
        assert!(!state.outputs[0].spent);
        assert!(state.outputs[1].spent);
        assert!(!state.outputs[2].spent);
    }

    #[test]
    fn test_mark_spent_duplicate_key_images() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);

        let count1 = state.mark_spent_by_key_images(&["ki1".to_string()]);
        assert_eq!(count1, 1);

        // Marking again should return 0 (already spent)
        let count2 = state.mark_spent_by_key_images(&["ki1".to_string()]);
        assert_eq!(count2, 0);
    }

    #[test]
    fn test_mark_spent_unknown_key_images() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);

        let count = state.mark_spent_by_key_images(&["unknown_ki".to_string()]);
        assert_eq!(count, 0);
        assert!(!state.outputs[0].spent);
    }

    #[test]
    fn test_mark_spent_by_output_keys() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 100, "ki1"),
            make_output(2_000_000_000_000, 100, "ki2"),
        ]);

        let count = state.mark_spent_by_output_keys(&["tx_ki1:0".to_string()]);
        assert_eq!(count, 1);
        assert!(state.outputs[0].spent);
        assert!(!state.outputs[1].spent);
    }

    // ---- add_outputs vs replace_outputs ----

    #[test]
    fn test_add_outputs_extends() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);
        assert_eq!(state.outputs().len(), 1);

        state.add_outputs(vec![make_output(2_000_000_000_000, 101, "ki2")]);
        assert_eq!(state.outputs().len(), 2);
    }

    #[test]
    fn test_replace_outputs_replaces() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 100, "ki1"),
            make_output(2_000_000_000_000, 101, "ki2"),
        ]);
        assert_eq!(state.outputs().len(), 2);

        state.replace_outputs(vec![make_output(3_000_000_000_000, 200, "ki3")]);
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.outputs()[0].key_image, "ki3");
    }

    #[test]
    fn test_replace_rebuilds_key_image_index() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);

        state.replace_outputs(vec![make_output(2_000_000_000_000, 200, "ki2")]);

        // Old key image should not match
        let count = state.mark_spent_by_key_images(&["ki1".to_string()]);
        assert_eq!(count, 0);

        // New key image should match
        let count = state.mark_spent_by_key_images(&["ki2".to_string()]);
        assert_eq!(count, 1);
    }

    // ---- Spendable output queries ----

    #[test]
    fn test_spendable_outputs() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"),  // spendable at height 100
            make_output(2_000_000_000_000, 95, "ki2"),  // not spendable (5 confs)
        ]);
        state.daemon_height = 100;

        let spendable = state.spendable_outputs();
        assert_eq!(spendable.len(), 1);
        assert_eq!(spendable[0].key_image, "ki1");
    }

    #[test]
    fn test_spendable_excludes_spent() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"),
            make_output(2_000_000_000_000, 80, "ki2"),
        ]);
        state.daemon_height = 100;
        state.mark_spent_by_key_images(&["ki1".to_string()]);

        let spendable = state.spendable_outputs();
        assert_eq!(spendable.len(), 1);
        assert_eq!(spendable[0].key_image, "ki2");
    }

    #[test]
    fn test_spendable_outputs_for_accounts() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_account_output(1_000_000_000_000, 80, "ki1", 0),
            make_account_output(2_000_000_000_000, 80, "ki2", 1),
            make_account_output(3_000_000_000_000, 80, "ki3", 2),
        ]);
        state.daemon_height = 100;

        let spendable = state.spendable_outputs_for_accounts(&[0, 2]);
        assert_eq!(spendable.len(), 2);
        let ki: Vec<&str> = spendable.iter().map(|o| o.key_image.as_str()).collect();
        assert!(ki.contains(&"ki1"));
        assert!(ki.contains(&"ki3"));
    }

    // ---- Height tracking ----

    #[test]
    fn test_add_outputs_updates_current_height() {
        let mut state = WalletState::new();
        assert_eq!(state.current_height, 0);

        state.add_outputs(vec![make_output(1_000_000_000_000, 500, "ki1")]);
        assert_eq!(state.current_height, 500);

        state.add_outputs(vec![make_output(1_000_000_000_000, 300, "ki2")]);
        // Should not decrease
        assert_eq!(state.current_height, 500);

        state.add_outputs(vec![make_output(1_000_000_000_000, 600, "ki3")]);
        assert_eq!(state.current_height, 600);
    }
}
