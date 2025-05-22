use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::wallet_output::WalletOutput;

const CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE: u64 = 10;
const CRYPTONOTE_MINED_MONEY_UNLOCK_WINDOW: u64 = 60;

/// Number of recent block hashes kept densely (every height).
const DENSE_HASH_WINDOW: u64 = 100;

/// Maximum reorg depth we'll handle. Deeper reorgs are treated as errors.
pub const MAX_REORG_DEPTH: u64 = 1000;

/// Balance breakdown for a wallet or account.
#[derive(Debug, Clone, Default)]
pub struct Balance {
    pub confirmed: u64,
    pub unconfirmed: u64,
}

/// Sparse chain of block hashes for fork-point detection.
///
/// Stores a compact set of block hashes: dense for recent blocks, exponentially
/// spaced for older ones. Mirrors wallet2's `get_short_chain_history()`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlockHashChain {
    hashes: BTreeMap<u64, String>,
    genesis_hash: Option<String>,
}

impl BlockHashChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_block(&mut self, height: u64, hash: String) {
        if height == 0 {
            self.genesis_hash = Some(hash.clone());
        }
        self.hashes.insert(height, hash);
    }

    pub fn get_hash(&self, height: u64) -> Option<&str> {
        self.hashes.get(&height).map(|s| s.as_str())
    }

    pub fn tip_height(&self) -> Option<u64> {
        self.hashes.keys().next_back().copied()
    }

    /// Remove all hashes at heights >= split_height.
    pub fn rollback_to(&mut self, split_height: u64) {
        // BTreeMap::split_off returns everything >= key
        let removed = self.hashes.split_off(&split_height);
        // If genesis was in the removed range, keep it
        if let Some(genesis) = &self.genesis_hash {
            if removed.get(&0).map(|h| h == genesis).unwrap_or(false) {
                self.hashes.insert(0, genesis.clone());
            }
        }
    }

    /// Prune to keep last DENSE_HASH_WINDOW dense + exponential anchors + genesis.
    pub fn compact(&mut self) {
        let tip = match self.tip_height() {
            Some(t) => t,
            None => return,
        };

        let dense_start = tip.saturating_sub(DENSE_HASH_WINDOW - 1);
        let mut keep = HashSet::new();

        // Keep dense window
        for h in dense_start..=tip {
            keep.insert(h);
        }

        // Keep exponential anchors below dense window
        if dense_start > 0 {
            let mut step = 1u64;
            let mut h = dense_start - 1;
            loop {
                keep.insert(h);
                step *= 2;
                if h < step {
                    break;
                }
                h -= step;
            }
        }

        // Always keep genesis
        keep.insert(0);

        self.hashes.retain(|h, _| keep.contains(h));
    }

    /// Build block_ids list for `/getblocks.bin`, matching wallet2's algorithm.
    ///
    /// Returns (height, hash_hex) pairs ordered from highest to lowest.
    pub fn get_short_chain_history(&self) -> Vec<(u64, String)> {
        let tip = match self.tip_height() {
            Some(t) => t,
            None => {
                // Only genesis available
                if let Some(g) = &self.genesis_hash {
                    return vec![(0, g.clone())];
                }
                return vec![];
            }
        };

        let mut result = Vec::new();
        let mut current = tip;
        let mut step = 1u64;
        let mut count = 0u64;

        loop {
            if let Some(hash) = self.hashes.get(&current) {
                result.push((current, hash.clone()));
            }

            if current == 0 {
                break;
            }

            // First 10 are dense (step=1), then exponential
            count += 1;
            if count >= 10 {
                step *= 2;
            }

            if current < step {
                // Jump to genesis
                if current != 0 {
                    if let Some(hash) = self.hashes.get(&0).or(self.genesis_hash.as_ref()) {
                        result.push((0, hash.clone()));
                    }
                }
                break;
            }
            current -= step;
        }

        result
    }

    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }
}

/// Result of rolling back wallet state after a reorg.
#[derive(Debug, Clone)]
pub struct RollbackResult {
    pub removed_outputs: Vec<WalletOutput>,
    pub removed_key_images: Vec<String>,
    pub unspent_key_images: Vec<String>,
    pub outputs_unspent: usize,
}

/// Core wallet state manager.
///
/// Holds outputs, tracks heights, and provides balance/spendability queries.
/// Applications supply the adapters; wallet lifecycle rules stay here.
pub struct WalletState {
    outputs: Vec<WalletOutput>,
    /// Maps key_image -> index in outputs vec for O(1) lookups
    key_image_index: HashMap<String, usize>,
    /// Stable identity remains available before and after key-image import.
    output_index: HashMap<String, usize>,
    pub current_height: u64,
    pub daemon_height: u64,
    pub block_hashes: BlockHashChain,
}

impl WalletState {
    pub fn new() -> Self {
        WalletState {
            outputs: Vec::new(),
            key_image_index: HashMap::new(),
            output_index: HashMap::new(),
            current_height: 0,
            daemon_height: 0,
            block_hashes: BlockHashChain::new(),
        }
    }

    /// Extend outputs in blockchain scan order (transaction order within each
    /// block, then output order within each transaction). Preserve this order
    /// when persisting outputs.
    ///
    /// Merge by transaction hash and output position, including when no key
    /// image is known. Preserve known key images and local spent state.
    /// Confirmations replace pool entries in scan order. Distinct outputs with
    /// an already known key image are not counted twice.
    pub fn add_outputs(&mut self, new_outputs: Vec<WalletOutput>) {
        let mut upgraded = HashSet::new();
        for mut output in new_outputs {
            if output.block_height > self.current_height {
                self.current_height = output.block_height;
            }
            let identity = output.output_key();
            if let Some(&existing_idx) = self.output_index.get(&identity) {
                // Learning an image during a rescan must enforce the same
                // uniqueness rule as adding an already identified output.
                // Otherwise the duplicate remains counted with an empty image
                // and cannot be marked spent when the shared image is observed.
                if self.outputs[existing_idx].key_image.is_empty() && !output.key_image.is_empty() {
                    if let Some(&other_idx) = self.key_image_index.get(&output.key_image) {
                        let duplicate = &self.outputs[existing_idx];
                        let spent = duplicate.spent || output.spent;
                        let spent_height = duplicate.spent_height.or(output.spent_height);
                        let kept = &mut self.outputs[other_idx];
                        kept.spent |= spent;
                        kept.spent_height = kept.spent_height.or(spent_height);
                        upgraded.insert(existing_idx);
                        self.output_index.remove(&identity);
                        continue;
                    }
                }
                let existing = &mut self.outputs[existing_idx];
                if !existing.key_image.is_empty() {
                    output.key_image = existing.key_image.clone();
                } else if !output.key_image.is_empty() {
                    existing.key_image = output.key_image.clone();
                    self.key_image_index
                        .insert(output.key_image.clone(), existing_idx);
                }
                existing.spent |= output.spent;
                existing.spent_height = existing.spent_height.or(output.spent_height);
                output.spent = existing.spent;
                output.spent_height = existing.spent_height;
                if self.outputs[existing_idx].block_height == 0 && output.block_height > 0 {
                    // Mempool arrival order need not match block order. Append
                    // confirmations in scan order and remove the old entries below.
                    upgraded.insert(existing_idx);
                    self.output_index.insert(identity, self.outputs.len());
                    if !output.key_image.is_empty() {
                        self.key_image_index
                            .insert(output.key_image.clone(), self.outputs.len());
                    }
                    self.outputs.push(output);
                }
                // Otherwise skip duplicate
            } else {
                if !output.key_image.is_empty()
                    && self.key_image_index.contains_key(&output.key_image)
                {
                    continue;
                }
                let idx = self.outputs.len();
                self.output_index.insert(identity, idx);
                if !output.key_image.is_empty() {
                    self.key_image_index.insert(output.key_image.clone(), idx);
                }
                self.outputs.push(output);
            }
        }
        if !upgraded.is_empty() {
            let mut index = 0;
            self.outputs.retain(|_| {
                let keep = !upgraded.contains(&index);
                index += 1;
                keep
            });
            self.rebuild_key_image_index();
        }
    }

    /// Replace all outputs (used when restoring from persistence).
    pub fn replace_outputs(&mut self, outputs: Vec<WalletOutput>) {
        self.outputs.clear();
        self.rebuild_key_image_index();
        let height = self.current_height;
        self.add_outputs(outputs);
        self.current_height = height;
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
        let key_set: HashSet<&str> = output_keys.iter().map(|s| s.as_str()).collect();
        let mut count = 0;
        for output in &mut self.outputs {
            if !output.spent {
                let key = output.output_key();
                if key_set.contains(key.as_str()) {
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

    /// Mark outputs as spent, recording the height at which the spend occurred.
    /// Returns the number of outputs newly marked as spent.
    pub fn mark_spent_by_key_images_at_height(&mut self, key_images: &[String], height: u64) -> usize {
        let mut count = 0;
        for ki in key_images {
            if let Some(&idx) = self.key_image_index.get(ki) {
                if !self.outputs[idx].spent {
                    self.outputs[idx].spent = true;
                    self.outputs[idx].spent_height = Some(height);
                    count += 1;
                }
            }
        }
        count
    }

    /// Roll back wallet state to just before `split_height`.
    ///
    /// 1. Un-spend outputs with spent_height >= split_height
    /// 2. Remove outputs with block_height >= split_height
    /// 3. Rebuild key_image_index
    /// 4. Roll back block_hashes
    /// 5. Update current_height
    pub fn rollback_to_height(&mut self, split_height: u64) -> RollbackResult {
        let mut unspent_key_images = Vec::new();
        let mut outputs_unspent = 0usize;

        // Un-spend outputs whose spend was in the reorged range
        for output in &mut self.outputs {
            if output.spent {
                if let Some(sh) = output.spent_height {
                    if sh >= split_height {
                        output.spent = false;
                        output.spent_height = None;
                        unspent_key_images.push(output.key_image.clone());
                        outputs_unspent += 1;
                    }
                }
                // Outputs with spent_height: None are NOT reverted (conservative)
            }
        }

        // Remove outputs received in the reorged range
        let mut removed_outputs = Vec::new();
        let mut removed_key_images = Vec::new();
        let mut kept = Vec::new();
        for output in self.outputs.drain(..) {
            if output.block_height >= split_height {
                removed_key_images.push(output.key_image.clone());
                removed_outputs.push(output);
            } else {
                kept.push(output);
            }
        }
        self.outputs = kept;

        self.rebuild_key_image_index();
        self.block_hashes.rollback_to(split_height);

        if split_height > 0 {
            self.current_height = split_height - 1;
        } else {
            self.current_height = 0;
        }

        RollbackResult {
            removed_outputs,
            removed_key_images,
            unspent_key_images,
            outputs_unspent,
        }
    }

    pub fn record_block_hash(&mut self, height: u64, hash: String) {
        self.block_hashes.record_block(height, hash);
    }

    pub fn get_short_chain_history(&self) -> Vec<(u64, String)> {
        self.block_hashes.get_short_chain_history()
    }

    fn rebuild_key_image_index(&mut self) {
        self.key_image_index.clear();
        self.output_index.clear();
        for (i, output) in self.outputs.iter().enumerate() {
            self.output_index.insert(output.output_key(), i);
            if !output.key_image.is_empty() {
                self.key_image_index.insert(output.key_image.clone(), i);
            }
        }
    }
}

pub fn is_spendable(output: &WalletOutput, height: u64) -> bool {
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
            spent_height: None,
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

    // ---- Deduplication tests ----

    #[test]
    fn test_add_outputs_deduplicates_by_key_image() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]); // duplicate
        assert_eq!(state.outputs().len(), 1);

        state.current_height = 200;
        let bal = state.balance();
        assert_eq!(bal.confirmed, 1_000_000_000_000); // not double-counted
    }

    fn output_without_key_image(identity: &str, height: u64) -> WalletOutput {
        let mut output = make_output(1_000_000_000_000, height, identity);
        output.key_image.clear();
        output
    }

    #[test]
    fn learned_shared_key_image_cannot_leave_a_phantom_balance() {
        let first = output_without_key_image("first", 100);
        let second = output_without_key_image("second", 101);
        let mut state = WalletState::new();
        state.add_outputs(vec![first.clone(), second.clone()]);
        assert_eq!(state.outputs().len(), 2);
        let mut first = first;
        let mut second = second;
        first.key_image = "shared".into();
        second.key_image = "shared".into();
        state.add_outputs(vec![first.clone(), second.clone()]);
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.balance_at_height(200).confirmed, first.amount);
        state.mark_spent_by_key_images_at_height(&["shared".into()], 150);
        assert_eq!(state.balance_at_height(200).confirmed, 0);
        state.add_outputs(vec![first, second]);
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.balance_at_height(200).confirmed, 0);
    }

    #[test]
    fn test_add_outputs_upgrades_mempool_to_confirmed() {
        let mut state = WalletState::new();
        // Add mempool output (height 0)
        let mut mempool_output = make_output(1_000_000_000_000, 0, "ki1");
        mempool_output.block_height = 0;
        state.add_outputs(vec![mempool_output]);
        assert_eq!(state.outputs()[0].block_height, 0);

        // Add confirmed version
        state.add_outputs(vec![make_output(1_000_000_000_000, 500, "ki1")]);
        assert_eq!(state.outputs().len(), 1); // still only 1 output
        assert_eq!(state.outputs()[0].block_height, 500); // upgraded to confirmed
    }

    #[test]
    fn test_spendability_boundary_exact() {
        // daemon_height = block count = top_block_height + 1.
        // Rust formula: confirmations = daemon_height - block_height.
        // At the exact boundary (10 confs required for normal outputs):
        let output = make_output(1_000_000_000_000, 100, "ki1");

        // daemon_height 109: 109 - 100 = 9 confirmations -> NOT spendable
        assert!(!is_spendable(&output, 109));

        // daemon_height 110: 110 - 100 = 10 confirmations -> spendable
        assert!(is_spendable(&output, 110));
    }

    #[test]
    fn test_coinbase_spendability_boundary_exact() {
        let output = make_coinbase_output(5_000_000_000_000, 100, "ki1");

        // daemon_height 159: 159 - 100 = 59 confirmations -> NOT spendable
        assert!(!is_spendable(&output, 159));

        // daemon_height 160: 160 - 100 = 60 confirmations -> spendable
        assert!(is_spendable(&output, 160));
    }

    #[test]
    fn test_add_outputs_skips_confirmed_duplicate() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);
        state.add_outputs(vec![make_output(2_000_000_000_000, 200, "ki1")]); // different amount, same ki
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.outputs()[0].block_height, 100); // original kept
        assert_eq!(state.outputs()[0].amount, 1_000_000_000_000); // original amount
    }

    // ---- BlockHashChain tests ----

    #[test]
    fn test_block_hash_chain_basics() {
        let mut chain = BlockHashChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.tip_height(), None);

        chain.record_block(100, "hash100".into());
        chain.record_block(101, "hash101".into());
        chain.record_block(102, "hash102".into());

        assert_eq!(chain.len(), 3);
        assert_eq!(chain.tip_height(), Some(102));
        assert_eq!(chain.get_hash(100), Some("hash100"));
        assert_eq!(chain.get_hash(101), Some("hash101"));
        assert_eq!(chain.get_hash(99), None);
    }

    #[test]
    fn test_block_hash_chain_rollback() {
        let mut chain = BlockHashChain::new();
        for h in 90..=100 {
            chain.record_block(h, format!("hash_{}", h));
        }
        assert_eq!(chain.tip_height(), Some(100));

        chain.rollback_to(98);
        assert_eq!(chain.tip_height(), Some(97));
        assert!(chain.get_hash(98).is_none());
        assert!(chain.get_hash(99).is_none());
        assert!(chain.get_hash(100).is_none());
        assert_eq!(chain.get_hash(97), Some("hash_97"));
    }

    #[test]
    fn test_block_hash_chain_rollback_preserves_genesis() {
        let mut chain = BlockHashChain::new();
        chain.record_block(0, "genesis".into());
        chain.record_block(1, "hash1".into());
        chain.record_block(2, "hash2".into());

        chain.rollback_to(1);
        assert_eq!(chain.get_hash(0), Some("genesis"));
        assert!(chain.get_hash(1).is_none());
    }

    #[test]
    fn test_short_chain_history_dense() {
        let mut chain = BlockHashChain::new();
        for h in 0..=5 {
            chain.record_block(h, format!("hash_{}", h));
        }

        let history = chain.get_short_chain_history();
        assert!(!history.is_empty());
        assert_eq!(history[0].0, 5);
        assert_eq!(history.last().unwrap().0, 0);
    }

    #[test]
    fn test_short_chain_history_exponential_spacing() {
        let mut chain = BlockHashChain::new();
        for h in 0..=200 {
            chain.record_block(h, format!("hash_{}", h));
        }

        let history = chain.get_short_chain_history();
        assert_eq!(history[0].0, 200);
        assert!(history.len() < 30);
        assert_eq!(history.last().unwrap().0, 0);
        for i in 0..10 {
            assert_eq!(history[i].0, 200 - i as u64);
        }
    }

    #[test]
    fn test_short_chain_history_empty() {
        let chain = BlockHashChain::new();
        let history = chain.get_short_chain_history();
        assert!(history.is_empty());
    }

    #[test]
    fn test_short_chain_history_genesis_only() {
        let mut chain = BlockHashChain::new();
        chain.record_block(0, "genesis".into());
        let history = chain.get_short_chain_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (0, "genesis".into()));
    }

    // ---- mark_spent_by_key_images_at_height tests ----

    #[test]
    fn test_mark_spent_at_height() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 100, "ki1"),
            make_output(2_000_000_000_000, 100, "ki2"),
        ]);

        let count = state.mark_spent_by_key_images_at_height(&["ki1".to_string()], 150);
        assert_eq!(count, 1);
        assert!(state.outputs[0].spent);
        assert_eq!(state.outputs[0].spent_height, Some(150));
        assert!(!state.outputs[1].spent);
        assert_eq!(state.outputs[1].spent_height, None);
    }

    // ---- Rollback tests ----

    #[test]
    fn test_rollback_removes_outputs_at_and_above_split() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 90, "ki1"),
            make_output(2_000_000_000_000, 100, "ki2"),
            make_output(3_000_000_000_000, 110, "ki3"),
        ]);

        let result = state.rollback_to_height(100);
        assert_eq!(result.removed_outputs.len(), 2);
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.outputs()[0].key_image, "ki1");
    }

    #[test]
    fn test_rollback_unspends_outputs_spent_in_reorg_range() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 60, "ki2"),
        ]);

        state.mark_spent_by_key_images_at_height(&["ki1".to_string()], 100);
        state.mark_spent_by_key_images_at_height(&["ki2".to_string()], 80);

        let result = state.rollback_to_height(90);
        assert_eq!(result.outputs_unspent, 1);
        assert_eq!(result.unspent_key_images, vec!["ki1".to_string()]);
        assert!(!state.outputs[0].spent);
        assert!(state.outputs[1].spent);
    }

    #[test]
    fn test_rollback_conservative_with_none_spent_height() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);

        state.mark_spent_by_key_images(&["ki1".to_string()]);
        assert_eq!(state.outputs[0].spent_height, None);

        let result = state.rollback_to_height(60);
        assert_eq!(result.outputs_unspent, 0);
        assert!(state.outputs[0].spent);
    }

    #[test]
    fn test_rollback_rebuilds_key_image_index() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 100, "ki2"),
        ]);

        state.rollback_to_height(100);
        assert_eq!(state.outputs().len(), 1);

        let count = state.mark_spent_by_key_images(&["ki1".to_string()]);
        assert_eq!(count, 1);
        let count = state.mark_spent_by_key_images(&["ki2".to_string()]);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_rollback_updates_current_height() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 200, "ki1")]);
        state.current_height = 200;

        state.rollback_to_height(150);
        assert_eq!(state.current_height, 149);
    }

    #[test]
    fn test_rollback_rolls_back_block_hashes() {
        let mut state = WalletState::new();
        for h in 90..=100 {
            state.record_block_hash(h, format!("hash_{}", h));
        }

        state.rollback_to_height(95);
        assert!(state.block_hashes.get_hash(95).is_none());
        assert_eq!(state.block_hashes.get_hash(94), Some("hash_94"));
    }

    #[test]
    fn test_rollback_below_all_outputs() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 100, "ki1"),
            make_output(2_000_000_000_000, 200, "ki2"),
        ]);

        let result = state.rollback_to_height(50);
        assert_eq!(result.removed_outputs.len(), 2);
        assert!(state.outputs().is_empty());
        assert_eq!(state.current_height, 49);
    }

    #[test]
    fn test_compact_keeps_dense_and_sparse() {
        let mut chain = BlockHashChain::new();
        for h in 0..=500 {
            chain.record_block(h, format!("hash_{}", h));
        }
        assert_eq!(chain.len(), 501);

        chain.compact();
        assert!(chain.len() < 150);
        assert_eq!(chain.get_hash(500), Some("hash_500"));
        assert_eq!(chain.get_hash(0), Some("hash_0"));
        assert_eq!(chain.get_hash(401), Some("hash_401"));
    }

    // ---- Complex rollback scenarios ----

    #[test]
    fn test_rollback_mixed_removals_and_unspends() {
        let mut state = WalletState::new();
        // Output below split: received early, spent in reorg range
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki_below")]);
        // Output at split: should be removed
        state.add_outputs(vec![make_output(2_000_000_000_000, 100, "ki_at")]);
        // Output above split: should be removed
        state.add_outputs(vec![make_output(3_000_000_000_000, 150, "ki_above")]);

        // Spend the below-split output at height 120 (in reorg range)
        state.mark_spent_by_key_images_at_height(&["ki_below".to_string()], 120);
        // Spend the at-split output at height 80 (below reorg range, stays spent)
        state.mark_spent_by_key_images_at_height(&["ki_at".to_string()], 80);

        let result = state.rollback_to_height(100);

        // ki_at and ki_above removed (block_height >= 100)
        assert_eq!(result.removed_outputs.len(), 2);
        // ki_below unspent (spent_height 120 >= 100), ki_at spent_height 80 < 100 not unspent
        assert_eq!(result.outputs_unspent, 1);
        assert_eq!(result.unspent_key_images, vec!["ki_below".to_string()]);

        // Only ki_below remains, now unspent
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.outputs()[0].key_image, "ki_below");
        assert!(!state.outputs()[0].spent);
        assert_eq!(state.outputs()[0].spent_height, None);

        // Balance should reflect the unspent output
        state.current_height = 99;
        state.daemon_height = 200;
        let bal = state.balance_at_height(200);
        assert_eq!(bal.confirmed, 1_000_000_000_000);
    }

    #[test]
    fn test_rollback_multi_account_outputs() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_account_output(1_000_000_000_000, 50, "ki_a0", 0),
            make_account_output(2_000_000_000_000, 50, "ki_a1", 1),
            make_account_output(3_000_000_000_000, 100, "ki_a0_high", 0),
            make_account_output(4_000_000_000_000, 100, "ki_a2_high", 2),
        ]);

        let result = state.rollback_to_height(100);
        assert_eq!(result.removed_outputs.len(), 2);
        assert_eq!(state.outputs().len(), 2);

        // Remaining: account 0 and account 1 outputs below split
        let accounts: Vec<u32> = state.outputs().iter()
            .map(|o| o.subaddress_index.unwrap().0)
            .collect();
        assert!(accounts.contains(&0));
        assert!(accounts.contains(&1));
    }

    #[test]
    fn test_consecutive_rollbacks() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 80, "ki2"),
            make_output(3_000_000_000_000, 100, "ki3"),
            make_output(4_000_000_000_000, 120, "ki4"),
        ]);
        for h in 50..=120 {
            state.record_block_hash(h, format!("hash_{}", h));
        }

        // First rollback: remove ki4
        let r1 = state.rollback_to_height(110);
        assert_eq!(r1.removed_outputs.len(), 1);
        assert_eq!(state.outputs().len(), 3);
        assert_eq!(state.current_height, 109);

        // Second rollback: remove ki3
        let r2 = state.rollback_to_height(90);
        assert_eq!(r2.removed_outputs.len(), 1);
        assert_eq!(state.outputs().len(), 2);
        assert_eq!(state.current_height, 89);

        // Third rollback: remove ki2
        let r3 = state.rollback_to_height(60);
        assert_eq!(r3.removed_outputs.len(), 1);
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.outputs()[0].key_image, "ki1");

        // Key image index still works
        let count = state.mark_spent_by_key_images(&["ki1".to_string()]);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_rollback_then_add_new_outputs() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 100, "ki2"),
        ]);

        state.rollback_to_height(100);
        assert_eq!(state.outputs().len(), 1);

        // Add new outputs from the new chain fork
        state.add_outputs(vec![
            make_output(5_000_000_000_000, 100, "ki_new"),
        ]);
        assert_eq!(state.outputs().len(), 2);
        assert_eq!(state.current_height, 100);

        // Both old (kept) and new outputs accessible via key image
        let c1 = state.mark_spent_by_key_images(&["ki1".to_string()]);
        assert_eq!(c1, 1);
        let c2 = state.mark_spent_by_key_images(&["ki_new".to_string()]);
        assert_eq!(c2, 1);
    }

    #[test]
    fn test_compact_then_rollback() {
        let mut chain = BlockHashChain::new();
        for h in 0..=500 {
            chain.record_block(h, format!("hash_{}", h));
        }
        chain.compact();

        // Rollback into the dense window should work fine
        chain.rollback_to(450);
        assert!(chain.get_hash(449).is_some());
        assert!(chain.get_hash(450).is_none());
        assert!(chain.get_hash(500).is_none());
        // Genesis preserved
        assert_eq!(chain.get_hash(0), Some("hash_0"));
    }

    #[test]
    fn test_rollback_to_height_zero() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 0, "ki_genesis"),
            make_output(2_000_000_000_000, 50, "ki1"),
        ]);
        state.record_block_hash(0, "genesis".into());

        let result = state.rollback_to_height(0);
        assert_eq!(result.removed_outputs.len(), 2);
        assert!(state.outputs().is_empty());
        assert_eq!(state.current_height, 0);
    }

    #[test]
    fn test_rollback_preserves_spent_below_split() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 60, "ki2"),
        ]);

        // Spend ki1 at height 70, ki2 at height 80
        state.mark_spent_by_key_images_at_height(&["ki1".to_string()], 70);
        state.mark_spent_by_key_images_at_height(&["ki2".to_string()], 80);

        // Rollback to 90: both spends are below split, should stay spent
        let result = state.rollback_to_height(90);
        assert_eq!(result.outputs_unspent, 0);
        assert!(state.outputs()[0].spent);
        assert!(state.outputs()[1].spent);
    }

    #[test]
    fn test_mark_spent_at_height_idempotent() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);

        let c1 = state.mark_spent_by_key_images_at_height(&["ki1".to_string()], 100);
        assert_eq!(c1, 1);
        assert_eq!(state.outputs()[0].spent_height, Some(100));

        // Marking again should return 0
        let c2 = state.mark_spent_by_key_images_at_height(&["ki1".to_string()], 200);
        assert_eq!(c2, 0);
        // Original spent_height preserved
        assert_eq!(state.outputs()[0].spent_height, Some(100));
    }
}
