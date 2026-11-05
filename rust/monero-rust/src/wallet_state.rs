use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::monero_rpc::RpcClientKey;
use crate::ring_db::{RingDatabase, RingPersister};
use crate::tx_builder::native::DecoySelection;
use crate::wallet_output::WalletOutput;

const CRYPTONOTE_DEFAULT_TX_SPENDABLE_AGE: u64 = 10;
const CRYPTONOTE_MINED_MONEY_UNLOCK_WINDOW: u64 = 60;

/// Pending spends expire after 2 hours if not confirmed.
pub const PENDING_SPEND_TTL_SECS: u64 = 7200;

/// Number of recent block hashes kept densely (every height).
const DENSE_HASH_WINDOW: u64 = 100;

/// Maximum reorg depth we'll handle. Deeper reorgs are treated as errors.
pub const MAX_REORG_DEPTH: u64 = 1000;

/// Balance breakdown for a wallet or account.
#[derive(Debug, Clone, Default)]
pub struct Balance {
    /// Unlocked funds this library can spend.
    pub confirmed: u64,
    /// Funds that are still locked or in the pool.
    pub unconfirmed: u64,
    /// Total amount of outputs that are pending-spent (broadcast but not yet confirmed).
    pub pending_spend: u64,
    /// Unspent pre-RingCT outputs, which this library cannot spend; wallet2's
    /// balance includes them.
    pub pre_ringct: u64,
}

/// Transaction lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxStatus {
    Created,
    Broadcast,
    Confirmed { height: u64 },
}

/// Reference to a change output expected from a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeOutputRef {
    pub tx_hash: String,
    pub output_index: u8,
    pub amount: u64,
}

/// A pending spend: an output used in a broadcast-but-unconfirmed transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSpend {
    pub tx_id: String,
    pub key_image: String,
    pub output_key: String,
    pub amount: u64,
    pub created_at_secs: u64,
}

/// A tracked transaction through its lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedTransaction {
    pub tx_id: String,
    pub status: TxStatus,
    pub spent_key_images: Vec<String>,
    pub spent_output_keys: Vec<String>,
    pub change_outputs: Vec<ChangeOutputRef>,
    pub fee: u64,
    pub created_at_secs: u64,
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

    /// The lowest height a hash is stored for.
    pub fn earliest_height(&self) -> Option<u64> {
        self.hashes.keys().next().copied()
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

        // Genesis, and the lowest height above it for a node to answer from after a deep reorg.
        keep.insert(0);
        if let Some((&base, _)) = self.hashes.range(1..).next() {
            keep.insert(base);
        }

        self.hashes.retain(|h, _| keep.contains(h));
    }

    /// Block ids for `/getblocks.bin`, newest first.
    pub fn get_short_chain_history(&self) -> Vec<(u64, String)> {
        let genesis = self.hashes.get(&0).or(self.genesis_hash.as_ref());
        let tip = match self.tip_height() {
            Some(t) => t,
            None => {
                // Only genesis available
                if let Some(g) = genesis {
                    return vec![(0, g.clone())];
                }
                return vec![];
            }
        };

        let mut result: Vec<(u64, String)> = Vec::new();
        let push = |result: &mut Vec<(u64, String)>, target: u64| {
            if target == 0 {
                return;
            }
            if let Some((&height, hash)) = self.hashes.range(1..=target).next_back() {
                if result.last().map_or(true, |(last, _)| height < *last) {
                    result.push((height, hash.clone()));
                }
            }
        };
        let mut current = tip;
        let mut step = 1u64;
        let mut count = 0u64;

        loop {
            push(&mut result, current);

            if current == 0 {
                break;
            }

            // First 10 are dense (step=1), then exponential
            count += 1;
            if count >= 10 {
                step *= 2;
            }

            if current < step {
                break;
            }
            current -= step;
        }
        if let Some((&base, _)) = self.hashes.range(1..).next() {
            push(&mut result, base);
        }
        if let Some(hash) = genesis {
            result.push((0, hash.clone()));
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

/// A conflict detected when a key image is spent at two different heights.
#[derive(Debug, Clone, PartialEq)]
pub struct SpentConflict {
    pub key_image: String,
    pub previous_spent_height: Option<u64>,
    pub new_height: u64,
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
    pub_key_index: HashMap<String, usize>,
    slot_heights: HashMap<String, u64>,
    /// Outputs set aside for a credited one with the same one-time key.
    superseded: Vec<WalletOutput>,
    /// Top scanned height; the chain length is one more. Advance it for blocks
    /// without outputs too.
    pub current_height: u64,
    /// The node's claimed height, unverified: for sync progress, never unlocks.
    pub daemon_height: u64,
    pub block_hashes: BlockHashChain,
    /// Outputs used in broadcast-but-unconfirmed transactions (key_image -> PendingSpend).
    pending_spends: HashMap<String, PendingSpend>,
    /// Transaction lifecycle tracking (tx_id -> TrackedTransaction).
    tracked_transactions: HashMap<String, TrackedTransaction>,
    /// Maps key_image -> tx_hash of the transaction that spent it, for conflict dedup.
    spent_by_tx: HashMap<String, String>,
    rings_restored: bool,
    /// Key images whose ring the stored ring database overrides once it is loaded.
    adopted_rings: HashSet<String>,
    /// Of `adopted_rings`, those learned from the chain.
    learned_before_restore: HashSet<String>,
    ring_conflicts: Vec<crate::ring_db::ChainRingConflict>,
    /// Rings by key image. Independent of `outputs`, so a ring outlives the
    /// output record it was chosen for.
    rings: RingDatabase,
    ring_persister: Option<Box<dyn RingPersister>>,
    rings_unsaved: bool,
    /// Loaded from storage before the ring database; see `check_ring_database_loaded`.
    loaded_without_rings: bool,
    rpc_client_key: RpcClientKey,
}

impl WalletState {
    pub fn new() -> Self {
        WalletState {
            outputs: Vec::new(),
            key_image_index: HashMap::new(),
            output_index: HashMap::new(),
            pub_key_index: HashMap::new(),
            superseded: Vec::new(),
            slot_heights: HashMap::new(),
            current_height: 0,
            daemon_height: 0,
            block_hashes: BlockHashChain::new(),
            pending_spends: HashMap::new(),
            tracked_transactions: HashMap::new(),
            spent_by_tx: HashMap::new(),
            rings_restored: false,
            adopted_rings: HashSet::new(),
            learned_before_restore: HashSet::new(),
            ring_conflicts: Vec::new(),
            rings: RingDatabase::default(),
            ring_persister: None,
            rings_unsaved: false,
            loaded_without_rings: false,
            rpc_client_key: RpcClientKey::random(),
        }
    }

    /// Refuse to record or store rings that would overwrite a stored ring database not yet loaded.
    fn check_ring_database_loaded(&self) -> Result<(), String> {
        let writes = self
            .ring_persister
            .as_ref()
            .is_some_and(|persister| persister.writes_storage());
        if self.loaded_without_rings && writes {
            return Err(
                "This wallet state was loaded from storage but its ring database was not: call \
                 restore_ring_database first (a new wallet restores RingDatabase::default()), \
                 or the stored rings would be overwritten"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// The key this wallet's daemon requests are signed with. wallet2 draws a new
    /// one per load unless `persistent_rpc_client_id` is set.
    pub fn rpc_client_key(&self) -> &RpcClientKey {
        &self.rpc_client_key
    }

    /// Sign this wallet's requests with a key stored earlier.
    pub fn restore_rpc_client_key(&mut self, key: RpcClientKey) {
        self.rpc_client_key = key;
    }

    /// Draw a new key, as wallet2 does for a new daemon, so it cannot link the
    /// wallet to the old one. Returns the new key.
    pub fn new_rpc_client_key(&mut self) -> &RpcClientKey {
        self.rpc_client_key = RpcClientKey::random();
        &self.rpc_client_key
    }

    /// Set where the ring database is stored; builders and relay store through it
    /// before anything leaves the process.
    pub fn set_ring_persister(&mut self, persister: Box<dyn RingPersister>) {
        self.ring_persister = Some(persister);
    }

    /// Whether the ring database has changes the persister has not stored.
    pub fn has_unsaved_rings(&self) -> bool {
        self.rings_unsaved
    }

    /// Hand a changed ring database to the persister. Builders and `broadcast_*`
    /// call it; call it after scans, `record_input_rings` and `clear_ring`.
    pub async fn persist_rings(&mut self) -> Result<(), String> {
        if !self.rings_unsaved {
            return Ok(());
        }
        self.check_ring_database_loaded()?;
        let persister = self.ring_persister.as_mut().ok_or_else(|| {
            "The ring database has changes and no ring persister to store them; call \
             WalletState::set_ring_persister first"
                .to_string()
        })?;
        persister
            .persist(&self.rings)
            .await
            .map_err(|e| format!("Failed to persist the ring database: {e}"))?;
        self.rings_unsaved = false;
        Ok(())
    }

    /// Record rings by key image, so rebuilt and conflicting spends reuse them.
    /// All or none; a ring that differs from the recorded one is refused.
    pub fn record_input_rings(
        &mut self,
        rings: &[crate::tx_builder::native::InputRing],
    ) -> Result<(), String> {
        let rings: Vec<crate::tx_builder::native::InputRing> = rings
            .iter()
            .map(|input_ring| crate::tx_builder::native::InputRing {
                key_image: crate::ring_db::canonical_key_image(&input_ring.key_image),
                ring: input_ring.ring.clone(),
            })
            .collect();
        let rings = rings.as_slice();
        self.check_ring_database_loaded()?;
        let mut seen = HashSet::with_capacity(rings.len());
        for input_ring in rings {
            if input_ring.key_image.is_empty() {
                return Err("Cannot persist a ring without its key image".to_string());
            }
            if !seen.insert(&input_ring.key_image) {
                return Err(format!(
                    "Transaction contains duplicate ring metadata for key image {}",
                    input_ring.key_image
                ));
            }
            match self.rings.ring(&input_ring.key_image) {
                Some(existing) if !existing.is_same_ring(&input_ring.ring) => {
                    return Err(format!(
                        "Refusing to replace the persisted ring for key image {}",
                        input_ring.key_image
                    ));
                }
                Some(_) => {}
                None if !self.key_image_index.contains_key(&input_ring.key_image) => {
                    return Err(format!(
                        "Cannot persist ring: key image {} is not in wallet state",
                        input_ring.key_image
                    ));
                }
                None => {
                    self.check_drawn_from_candidates(&input_ring.key_image, &input_ring.ring)?
                }
            }
        }

        for input_ring in rings {
            if self.keep_ring(&input_ring.key_image, &input_ring.ring) {
                self.rings_unsaved = true;
            }
        }
        self.attach_rings();
        for input_ring in rings {
            self.adopted_rings.remove(&input_ring.key_image);
        }
        Ok(())
    }

    /// Record candidate sets before the daemon is asked for them; never replace one.
    pub(crate) fn record_ring_candidates(
        &mut self,
        key_images: &[String],
        candidates: &[Vec<u64>],
    ) -> Result<(), String> {
        if key_images.len() != candidates.len() {
            return Err("candidate set count does not match input count".to_string());
        }
        self.check_ring_database_loaded()?;
        for (key_image, candidates) in key_images.iter().zip(candidates) {
            match self.rings.candidates(key_image) {
                Some(existing) if existing != candidates.as_slice() => {
                    return Err(format!(
                        "Refusing to replace the persisted candidate set for key image {key_image}"
                    ));
                }
                Some(_) => {}
                None => {
                    self.rings.set_candidates(key_image, candidates.clone());
                    self.rings_unsaved = true;
                }
            }
        }
        Ok(())
    }

    /// Refuse a new ring outside the recorded candidate set: with that set, it
    /// would show the daemon the real output.
    fn check_drawn_from_candidates(
        &self,
        key_image: &str,
        ring: &DecoySelection,
    ) -> Result<(), String> {
        let Some(candidates) = self.rings.candidates(key_image) else {
            return Ok(());
        };
        let indexes = ring.indexes()?;
        let real = self
            .key_image_index
            .get(key_image)
            .and_then(|&index| self.outputs[index].global_index);
        if !self.rings.is_within_candidates(key_image, ring)
            || real.is_some_and(|real| !indexes.contains(&real))
        {
            return Err(format!(
                "Refusing to record a ring for key image {key_image} that is not drawn from the \
                 {} candidates the daemon was already asked for: the two together would show \
                 the daemon the real output",
                candidates.len()
            ));
        }
        Ok(())
    }

    /// Whether the wallet holds the output of `key_image`, or a ring for it.
    pub(crate) fn can_record_ring(&self, key_image: &str) -> bool {
        let key_image = crate::ring_db::canonical_key_image(key_image);
        self.key_image_index.contains_key(&key_image) || self.rings.ring(&key_image).is_some()
    }

    pub(crate) fn key_image_of_output_key(&self, output_key: &str) -> Option<&str> {
        self.outputs
            .iter()
            .find(|output| output.key == output_key && !output.key_image.is_empty())
            .map(|output| output.key_image.as_str())
    }

    /// Store `ring` for a key image holding no different ring; returns whether it changed.
    fn keep_ring(&mut self, key_image: &str, ring: &DecoySelection) -> bool {
        let unchanged = self
            .rings
            .ring(key_image)
            .is_some_and(|existing| !existing.ring.is_empty() || existing == ring);
        if !unchanged {
            self.rings.set_ring(key_image, ring.clone());
        }
        !unchanged
    }

    /// Learn the rings of this wallet's own spends seen on chain. Unlike wallet2,
    /// a recorded ring is never replaced (a node can fake blocks); a different
    /// one is listed in [`Self::ring_conflicts`].
    pub fn learn_spent_rings(&mut self, key_images: &[String], rings: &[Vec<u64>]) -> usize {
        self.learn_spent_rings_of(&HashMap::new(), key_images, rings)
    }

    /// `learn_spent_rings`, also counting the batch's own new outputs `received`.
    pub(crate) fn learn_spent_rings_of(
        &mut self,
        received: &HashMap<&str, Option<u64>>,
        key_images: &[String],
        rings: &[Vec<u64>],
    ) -> usize {
        let mut learned = 0;
        for (key_image, offsets) in key_images.iter().zip(rings) {
            if offsets.is_empty() {
                continue;
            }
            let key_image = crate::ring_db::canonical_key_image(key_image);
            let global_index = match self.key_image_index.get(&key_image) {
                Some(&index) => self.outputs[index].global_index,
                None => match received.get(key_image.as_str()) {
                    Some(global_index) => *global_index,
                    None if self.rings.ring(&key_image).is_some() => None,
                    None => continue,
                },
            };
            let mut ring = DecoySelection::from_offsets(offsets, 0);
            let Ok(indexes) = ring.indexes() else {
                continue;
            };
            if let Some(global_index) = global_index {
                let Some(position) = indexes.iter().position(|i| *i == global_index) else {
                    continue;
                };
                ring.real_index = u8::try_from(position).unwrap_or(0);
            }
            match self.rings.ring(&key_image) {
                Some(existing) if existing.is_same_ring(&ring) => {}
                Some(existing) => {
                    let recorded = existing.indexes().unwrap_or_default();
                    self.note_ring_conflict(&key_image, recorded, indexes);
                }
                None if !self.rings.is_within_candidates(&key_image, &ring) => {
                    let recorded = self
                        .rings
                        .candidates(&key_image)
                        .unwrap_or_default()
                        .to_vec();
                    self.note_ring_conflict(&key_image, recorded, indexes);
                }
                None => {
                    if !self.rings_restored {
                        self.adopted_rings.insert(key_image.clone());
                        self.learned_before_restore.insert(key_image.clone());
                    }
                    self.rings.set_ring(&key_image, ring);
                    learned += 1;
                }
            }
        }
        if learned > 0 {
            self.rings_unsaved = true;
            self.attach_rings();
        }
        learned
    }

    /// Rings on chain that differ from the kept ones. `clear_ring` and a rescan
    /// adopt the chain's ring.
    pub fn ring_conflicts(&self) -> &[crate::ring_db::ChainRingConflict] {
        &self.ring_conflicts
    }

    /// Return the listed ring conflicts and empty the list.
    pub fn take_ring_conflicts(&mut self) -> Vec<crate::ring_db::ChainRingConflict> {
        std::mem::take(&mut self.ring_conflicts)
    }

    fn note_ring_conflict(&mut self, key_image: &str, recorded: Vec<u64>, seen: Vec<u64>) {
        let conflict = crate::ring_db::ChainRingConflict {
            key_image: key_image.to_string(),
            recorded,
            seen,
        };
        if !self.ring_conflicts.contains(&conflict) {
            self.ring_conflicts.push(conflict);
        }
    }

    /// Record a signed transaction's rings before relay; all or none, refusing a
    /// second ring or one outside the recorded candidate set.
    pub fn record_transaction_rings(&mut self, tx_blob: &[u8]) -> Result<usize, String> {
        self.check_ring_database_loaded()?;
        let mut new_rings = Vec::new();
        for (key_image, offsets) in crate::scanner::extract_spent_rings_from_raw_tx(tx_blob) {
            if !self.can_record_ring(&key_image) {
                continue;
            }
            let mut ring = DecoySelection::from_offsets(&offsets, 0);
            let indexes = ring.indexes()?;
            match self.rings.ring(&key_image) {
                Some(existing) if existing.is_same_ring(&ring) => continue,
                Some(_) => {
                    return Err(format!(
                        "Refusing to replace the persisted ring for key image {key_image}: the \
                         transaction carries a different one"
                    ));
                }
                None => self.check_drawn_from_candidates(&key_image, &ring)?,
            }
            let global_index = self
                .key_image_index
                .get(&key_image)
                .and_then(|&index| self.outputs[index].global_index);
            if let Some(position) = global_index
                .and_then(|global_index| indexes.iter().position(|i| *i == global_index))
            {
                ring.real_index = u8::try_from(position).unwrap_or(0);
            }
            new_rings.push((key_image, ring));
        }
        let recorded = new_rings.len();
        for (key_image, ring) in new_rings {
            self.rings.set_ring(&key_image, ring);
            self.rings_unsaved = true;
        }
        if recorded > 0 {
            self.attach_rings();
        }
        Ok(recorded)
    }

    /// Every recorded ring by key image, to be stored durably. Rollbacks and
    /// rescans leave it alone.
    pub fn ring_database(&self) -> &RingDatabase {
        &self.rings
    }

    /// Load a stored ring database; on a conflict with recorded rings nothing is
    /// loaded. Rings from output records give way to it.
    pub fn restore_ring_database(&mut self, stored: RingDatabase) -> Result<(), String> {
        let canonical = crate::ring_db::canonical_key_image;
        let mut stored_rings: BTreeMap<String, &DecoySelection> = BTreeMap::new();
        for (key_image, ring) in stored.rings() {
            let key_image = canonical(key_image);
            if stored_rings
                .insert(key_image.clone(), ring)
                .is_some_and(|other| !other.is_same_ring(ring))
            {
                return Err(format!(
                    "Stored ring database holds two rings for key image {key_image}"
                ));
            }
        }
        let mut stored_candidates: BTreeMap<String, &[u64]> = BTreeMap::new();
        for (key_image, candidates) in stored.candidate_sets() {
            let key_image = canonical(key_image);
            if stored_candidates
                .insert(key_image.clone(), candidates)
                .is_some_and(|other| other != candidates)
            {
                return Err(format!(
                    "Stored ring database holds two candidate sets for key image {key_image}"
                ));
            }
        }

        let mut merged = self.rings.clone();
        let mut conflicts = Vec::new();
        for key_image in &self.adopted_rings {
            let learned = self.learned_before_restore.contains(key_image);
            let Some(provisional) = merged.ring(key_image).cloned() else {
                continue;
            };
            let seen = provisional.indexes().unwrap_or_default();
            if let Some(ring) = stored_rings.get(key_image) {
                if learned && !ring.is_same_ring(&provisional) {
                    conflicts.push((key_image.clone(), ring.indexes()?, seen));
                }
                merged.remove_ring(key_image);
            } else if let Some(candidates) = stored_candidates.get(key_image) {
                if !seen.iter().all(|index| candidates.contains(index)) {
                    if learned {
                        conflicts.push((key_image.clone(), candidates.to_vec(), seen));
                    }
                    merged.remove_ring(key_image);
                }
            } else if stored.is_cleared(key_image) && !learned {
                merged.remove_ring(key_image);
            }
        }
        for (key_image, candidates) in &stored_candidates {
            match merged.candidates(key_image) {
                Some(existing) if existing != *candidates => {
                    return Err(format!(
                        "Stored ring database holds a different candidate set for key image \
                         {key_image}"
                    ));
                }
                Some(_) => {}
                None => merged.set_candidates(key_image, candidates.to_vec()),
            }
        }
        for (key_image, ring) in &stored_rings {
            match merged.ring(key_image) {
                Some(existing) if !existing.is_same_ring(ring) => {
                    return Err(format!(
                        "Stored ring database holds a different ring for key image {key_image}"
                    ));
                }
                Some(existing) if !existing.ring.is_empty() => {}
                _ if !merged.is_within_candidates(key_image, ring) => {
                    return Err(format!(
                        "Stored ring database holds a ring for key image {key_image} outside \
                         its candidate set"
                    ));
                }
                _ => merged.set_ring(key_image, (*ring).clone()),
            }
        }
        for key_image in stored.cleared_key_images() {
            if !merged.holds(key_image) {
                merged.mark_cleared(key_image);
            }
        }
        for (key_image, ring) in merged.rings() {
            if !merged.is_within_candidates(key_image, ring) {
                return Err(format!(
                    "Stored ring database holds a candidate set for key image {key_image} that \
                     does not contain the ring held for it"
                ));
            }
        }

        self.rings = merged;
        for (key_image, recorded, seen) in conflicts {
            self.note_ring_conflict(&key_image, recorded, seen);
        }
        self.adopted_rings.clear();
        self.learned_before_restore.clear();
        self.rings_restored = true;
        self.loaded_without_rings = false;
        self.attach_rings();
        Ok(())
    }

    /// Forget and return the ring of `key_image`. Ask the user: a new ring beside
    /// a published one shows which member is real.
    pub fn clear_ring(&mut self, key_image: &str) -> Option<DecoySelection> {
        let key_image = crate::ring_db::canonical_key_image(key_image);
        let key_image = key_image.as_str();
        let had_candidates = self.rings.candidates(key_image).is_some();
        let cleared = self.rings.remove_ring(key_image);
        self.adopted_rings.remove(key_image);
        if cleared.is_some() || had_candidates {
            self.rings.mark_cleared(key_image);
            self.rings_unsaved = true;
        }
        let cleared = cleared?;
        for output in &mut self.outputs {
            if output.key_image == key_image {
                output.ring = None;
            }
        }
        Some(cleared)
    }

    fn attach_rings(&mut self) {
        let mut adopted: Vec<(String, DecoySelection)> = Vec::new();
        for output in &mut self.outputs {
            if output.key_image.is_empty() {
                continue;
            }
            match self.rings.ring(&output.key_image) {
                Some(ring) => {
                    if output.ring.as_ref() != Some(ring) {
                        output.ring = Some(ring.clone());
                    }
                }
                None if self.rings.holds(&output.key_image) => output.ring = None,
                None => {
                    if let Some(ring) = &output.ring {
                        adopted.push((output.key_image.clone(), ring.clone()));
                    }
                }
            }
        }
        for (key_image, ring) in adopted {
            if !self.rings_restored {
                self.adopted_rings.insert(key_image.clone());
            }
            self.rings.set_ring(&key_image, ring);
            self.rings_unsaved = true;
        }
    }

    /// Extend outputs in scan order, which persistence must keep for positional
    /// key image imports. Like wallet2, credit one record per one-time key: a
    /// later output replaces it only if larger and it is unspent, or if it is a
    /// pool output and the later one is confirmed.
    pub fn add_outputs(&mut self, new_outputs: Vec<WalletOutput>) {
        let mut upgraded = HashSet::new();
        for mut output in new_outputs {
            output.key_image = crate::ring_db::canonical_key_image(&output.key_image);
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
                        let frozen = duplicate.frozen || output.frozen;
                        let spent_height = duplicate.spent_height.or(output.spent_height);
                        let kept = &mut self.outputs[other_idx];
                        kept.spent |= spent;
                        kept.frozen |= frozen;
                        kept.spent_height = kept.spent_height.or(spent_height);
                        upgraded.insert(existing_idx);
                        self.output_index.remove(&identity);
                        continue;
                    }
                }
                let existing = &mut self.outputs[existing_idx];
                // Take the latest scan's index, so a rescan corrects a wrong one.
                if output.global_index.is_some() && existing.global_index != output.global_index {
                    existing.global_index = output.global_index;
                    existing.received_output_bytes = output.received_output_bytes.clone();
                }
                if !existing.key_image.is_empty() {
                    output.key_image = existing.key_image.clone();
                } else if !output.key_image.is_empty() {
                    existing.key_image = output.key_image.clone();
                    self.key_image_index
                        .insert(output.key_image.clone(), existing_idx);
                }
                existing.spent |= output.spent;
                existing.frozen |= output.frozen;
                existing.spent_height = existing.spent_height.or(output.spent_height);
                output.spent = existing.spent;
                output.frozen = existing.frozen;
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
                    if let Some(key) = one_time_key(&output) {
                        self.pub_key_index
                            .insert(key.to_string(), self.outputs.len());
                    }
                    self.outputs.push(output);
                }
                // Otherwise skip duplicate
            } else {
                let held_idx = one_time_key(&output)
                    .and_then(|key| self.pub_key_index.get(key).copied())
                    .filter(|idx| !upgraded.contains(idx));
                if let Some(held_idx) = held_idx {
                    let held = &self.outputs[held_idx];
                    let held_in_pool = held.block_height == 0;
                    let new_in_pool = output.block_height == 0;
                    let replace = !held.spent
                        && if held_in_pool != new_in_pool {
                            held_in_pool
                        } else {
                            output.amount > held.amount
                        };
                    if replace {
                        if output.key_image.is_empty() {
                            output.key_image = held.key_image.clone();
                        }
                        output.frozen |= held.frozen;
                        self.output_index.remove(&held.output_key());
                        if held_in_pool {
                            upgraded.insert(held_idx);
                            let idx = self.outputs.len();
                            self.output_index.insert(identity, idx);
                            if !output.key_image.is_empty() {
                                self.key_image_index.insert(output.key_image.clone(), idx);
                            }
                            self.pub_key_index.insert(output.key.clone(), idx);
                            self.slot_heights.remove(&output.key);
                            self.outputs.push(output);
                            continue;
                        }
                        self.output_index.insert(identity, held_idx);
                        if !output.key_image.is_empty() {
                            self.key_image_index
                                .insert(output.key_image.clone(), held_idx);
                        }
                        let slot_height = self.export_height(held_idx);
                        let key = output.key.clone();
                        let displaced = std::mem::replace(&mut self.outputs[held_idx], output);
                        if displaced.block_height != 0 {
                            self.slot_heights.insert(key, slot_height);
                        }
                        self.set_aside(displaced);
                    } else {
                        self.set_aside(output);
                    }
                    continue;
                }
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
                if let Some(key) = one_time_key(&output) {
                    self.pub_key_index.insert(key.to_string(), idx);
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
        self.attach_rings();
    }

    fn set_aside(&mut self, mut output: WalletOutput) {
        output.key_image = crate::ring_db::canonical_key_image(&output.key_image);
        if output.block_height == 0 {
            return;
        }
        let identity = output.output_key();
        if !self.superseded.iter().any(|o| o.output_key() == identity) {
            self.superseded.push(output);
        }
    }

    /// Replace all outputs, as when restoring from persistence; the ring database is kept.
    pub fn replace_outputs(&mut self, outputs: Vec<WalletOutput>) {
        self.loaded_without_rings |= !self.rings_restored;
        self.superseded.clear();
        self.outputs.clear();
        self.rebuild_key_image_index();
        let height = self.current_height;
        self.add_outputs(outputs);
        self.current_height = height;
        self.spent_by_tx.clear();
    }

    /// Mark outputs as spent by matching key images.
    /// Returns the number of outputs newly marked as spent.
    pub fn mark_spent_by_key_images(&mut self, key_images: &[String]) -> usize {
        let mut count = 0;
        for ki in key_images {
            if let Some(&idx) = self
                .key_image_index
                .get(&crate::ring_db::canonical_key_image(ki))
            {
                if !self.outputs[idx].spent {
                    self.outputs[idx].spent = true;
                    count += 1;
                }
            }
        }
        count
    }

    /// Mark outputs spent by "txHash:outputIndex"; returns how many. No rollback
    /// reverts this: `release_unmined_spends` does.
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

    /// Unmark outputs ("txHash:outputIndex") spent by a never-mined transaction;
    /// a spend seen in a block stays. Returns how many.
    pub fn release_unmined_spends(&mut self, output_keys: &[String]) -> usize {
        let key_set: HashSet<&str> = output_keys.iter().map(|s| s.as_str()).collect();
        let mut released = Vec::new();
        for output in &mut self.outputs {
            if key_set.contains(output.output_key().as_str())
                && output.spent
                && output.spent_height.is_none()
            {
                output.spent = false;
                released.push(output.key_image.clone());
            }
        }
        for key_image in &released {
            self.pending_spends.remove(key_image);
            self.spent_by_tx.remove(key_image);
        }
        released.len()
    }

    /// Calculate balance at the wallet's own chain length, `current_height + 1`.
    pub fn balance(&self) -> Balance {
        self.balance_at_height(self.current_height.saturating_add(1))
    }

    /// Calculate balance at a specific height using the current system time.
    pub fn balance_at_height(&self, height: u64) -> Balance {
        self.balance_at(height, current_time_secs())
    }

    /// Balance at `height` with an explicit clock (`None` keeps time locks); frozen excluded.
    pub fn balance_at(&self, height: u64, now_secs: Option<u64>) -> Balance {
        let mut bal = Balance::default();
        for output in &self.outputs {
            if output.spent || output.frozen {
                continue;
            }
            if output.pre_ringct {
                bal.pre_ringct += output.amount;
                continue;
            }
            if self.pending_spends.contains_key(&output.key_image) {
                bal.pending_spend += output.amount;
                continue;
            }
            // Use the coin-selection predicate for confirmed balance too. Outputs
            // with an unsatisfied additional timelock count as unconfirmed.
            if is_spendable_at(output, height, now_secs) {
                bal.confirmed += output.amount;
            } else {
                bal.unconfirmed += output.amount;
            }
        }
        bal
    }

    /// `current_height + 1`, the height unlocks are measured at: never the node's.
    pub fn chain_height(&self) -> u64 {
        self.current_height.saturating_add(1)
    }

    /// Get all spendable (confirmed, unspent) outputs at the wallet's own chain height.
    pub fn spendable_outputs(&self) -> Vec<&WalletOutput> {
        self.spendable_outputs_at_height(self.chain_height())
    }

    /// Get spendable outputs at a specific height using the current system time.
    pub fn spendable_outputs_at_height(&self, height: u64) -> Vec<&WalletOutput> {
        self.spendable_outputs_at(height, current_time_secs())
    }

    /// Spendable outputs at `height` with an explicit clock; never pre-RingCT ones.
    pub fn spendable_outputs_at(&self, height: u64, now_secs: Option<u64>) -> Vec<&WalletOutput> {
        self.outputs
            .iter()
            .filter(|o| {
                !o.spent
                    && !o.frozen
                    && !o.pre_ringct
                    && !self.pending_spends.contains_key(&o.key_image)
                    && is_spendable_at(o, height, now_secs)
            })
            .collect()
    }

    /// Get spendable outputs filtered to specific accounts, at the wallet's own chain height.
    pub fn spendable_outputs_for_accounts(&self, accounts: &[u32]) -> Vec<&WalletOutput> {
        self.spendable_outputs_for_accounts_at(accounts, self.chain_height(), current_time_secs())
    }

    /// Get spendable outputs for specific accounts using an explicit height and
    /// Unix clock, with the same timelock rules as `spendable_outputs_at`.
    pub fn spendable_outputs_for_accounts_at(
        &self,
        accounts: &[u32],
        height: u64,
        now_secs: Option<u64>,
    ) -> Vec<&WalletOutput> {
        self.spendable_outputs_at(height, now_secs)
            .into_iter()
            .filter(|o| {
                let account = o.subaddress_index.map(|(a, _)| a).unwrap_or(0);
                accounts.contains(&account)
            })
            .collect()
    }

    /// Every (account, index) paid to; feed it to `cached_scanner_for_wallet`.
    pub fn used_subaddresses(&self) -> Vec<(u32, u32)> {
        let used: HashSet<(u32, u32)> = self
            .outputs
            .iter()
            .filter_map(|output| output.subaddress_index)
            .collect();
        let mut used: Vec<(u32, u32)> = used.into_iter().collect();
        used.sort_unstable();
        used
    }

    /// Get all outputs (including spent).
    pub fn outputs(&self) -> &[WalletOutput] {
        &self.outputs
    }

    /// The credited output with this key image, if any.
    pub(crate) fn output_by_key_image(&self, key_image: &str) -> Option<&WalletOutput> {
        self.key_image_index
            .get(key_image)
            .map(|&index| &self.outputs[index])
    }

    /// Modify outputs; indexes are rebuilt and each `ring` reset from the ring
    /// database afterwards.
    pub fn with_outputs_mut<R>(&mut self, f: impl FnOnce(&mut Vec<WalletOutput>) -> R) -> R {
        let result = f(&mut self.outputs);
        self.rebuild_key_image_index();
        self.attach_rings();
        result
    }

    /// Mark outputs as spent, recording the height at which the spend occurred.
    /// Returns the number of outputs newly marked as spent.
    pub fn mark_spent_by_key_images_at_height(
        &mut self,
        key_images: &[String],
        height: u64,
    ) -> usize {
        let mut count = 0;
        for ki in key_images {
            if let Some(&idx) = self
                .key_image_index
                .get(&crate::ring_db::canonical_key_image(ki))
            {
                if !self.outputs[idx].spent {
                    self.outputs[idx].spent = true;
                    self.outputs[idx].spent_height = Some(height);
                    count += 1;
                }
            }
        }
        count
    }

    /// Like `mark_spent_by_key_images_at_height` but also detects conflicts:
    /// an output spent by a *different transaction*.
    ///
    /// `tx_hashes` is parallel to `key_images`; the tx that contains each key
    /// image.  When a tx hash is known and matches the previously recorded
    /// spending tx, the spend is treated as identical (rescan) regardless of
    /// height.  A conflict is only raised when the spending tx differs.
    ///
    /// Not a conflict: broadcast-marked (`spent_height: None`) confirmed at any
    /// height, or same spending transaction (idempotent rescan).
    ///
    /// Returns (newly_spent_count, conflicts).
    pub fn mark_spent_detecting_conflicts(
        &mut self,
        key_images: &[String],
        tx_hashes: &[String],
        height: u64,
    ) -> (usize, Vec<SpentConflict>) {
        let mut count = 0;
        let mut conflicts = Vec::new();
        for (i, ki) in key_images.iter().enumerate() {
            let tx_hash = tx_hashes.get(i).map(|s| s.as_str()).unwrap_or("");
            if let Some(&idx) = self
                .key_image_index
                .get(&crate::ring_db::canonical_key_image(ki))
            {
                let output = &mut self.outputs[idx];
                if !output.spent {
                    output.spent = true;
                    output.spent_height = Some(height);
                    if !tx_hash.is_empty() {
                        self.spent_by_tx.insert(ki.clone(), tx_hash.to_string());
                    }
                    count += 1;
                } else if let Some(prev_h) = output.spent_height {
                    // Check if same spending transaction (rescan)
                    let same_tx = !tx_hash.is_empty()
                        && self
                            .spent_by_tx
                            .get(ki)
                            .map_or(false, |prev| prev == tx_hash);
                    if same_tx {
                        // Idempotent rescan; update height to latest observation
                        output.spent_height = Some(height);
                    } else if prev_h != height {
                        // Different tx or unknown tx at different height -> conflict
                        conflicts.push(SpentConflict {
                            key_image: ki.clone(),
                            previous_spent_height: Some(prev_h),
                            new_height: height,
                        });
                        // Update the recorded spending tx if we now know it
                        if !tx_hash.is_empty() {
                            self.spent_by_tx.insert(ki.clone(), tx_hash.to_string());
                        }
                    }
                    // Same height, unknown tx = idempotent, no conflict
                } else {
                    // spent_height: None = broadcast-marked, now confirmed on-chain
                    output.spent_height = Some(height);
                    if !tx_hash.is_empty() {
                        self.spent_by_tx.insert(ki.clone(), tx_hash.to_string());
                    }
                }
            }
        }
        (count, conflicts)
    }

    /// Read-only check: find conflicts where mempool key images match
    /// outputs already confirmed-spent at some height.
    pub fn check_spent_conflicts(&self, key_images: &[String]) -> Vec<SpentConflict> {
        let mut conflicts = Vec::new();
        for ki in key_images {
            if let Some(&idx) = self
                .key_image_index
                .get(&crate::ring_db::canonical_key_image(ki))
            {
                let output = &self.outputs[idx];
                if output.spent {
                    if let Some(prev_h) = output.spent_height {
                        conflicts.push(SpentConflict {
                            key_image: ki.clone(),
                            previous_spent_height: Some(prev_h),
                            new_height: 0, // 0 = mempool sentinel
                        });
                    }
                }
            }
        }
        conflicts
    }

    /// Roll back to just before `split_height`. The ring database is kept.
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
        let set_aside: Vec<WalletOutput> = std::mem::take(&mut self.superseded)
            .into_iter()
            .filter(|output| output.block_height < split_height)
            .collect();
        self.add_outputs(set_aside);
        self.block_hashes.rollback_to(split_height);

        // Clear pending spends and spending-tx records for removed/unspent key images
        for ki in removed_key_images.iter().chain(unspent_key_images.iter()) {
            self.pending_spends.remove(ki);
            self.spent_by_tx.remove(ki);
        }

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

    /// Freeze an output by key image, preventing it from being selected for spending.
    /// Returns true if the output was found and frozen.
    pub fn freeze_output(&mut self, key_image: &str) -> bool {
        if let Some(&idx) = self
            .key_image_index
            .get(&crate::ring_db::canonical_key_image(key_image))
        {
            self.outputs[idx].frozen = true;
            true
        } else {
            false
        }
    }

    /// Thaw (unfreeze) an output by key image, allowing it to be selected for spending.
    /// Returns true if the output was found and thawed.
    pub fn thaw_output(&mut self, key_image: &str) -> bool {
        if let Some(&idx) = self
            .key_image_index
            .get(&crate::ring_db::canonical_key_image(key_image))
        {
            self.outputs[idx].frozen = false;
            true
        } else {
            false
        }
    }

    // ---- Pending spend management ----

    /// Add pending spends for a broadcast transaction.
    pub fn add_pending_spends(&mut self, tx_id: &str, spends: Vec<PendingSpend>) {
        for spend in spends {
            self.pending_spends.insert(spend.key_image.clone(), spend);
        }
        // Advance tracked tx to Broadcast if it exists
        if let Some(tx) = self.tracked_transactions.get_mut(tx_id) {
            if tx.status == TxStatus::Created {
                tx.status = TxStatus::Broadcast;
            }
        }
    }

    /// Confirm a pending spend: remove it from pending and mark the output as spent.
    /// Returns true if the key image was pending.
    pub fn confirm_pending_spend(&mut self, key_image: &str, height: u64) -> bool {
        if self.pending_spends.remove(key_image).is_some() {
            if let Some(&idx) = self
                .key_image_index
                .get(&crate::ring_db::canonical_key_image(key_image))
            {
                if !self.outputs[idx].spent {
                    self.outputs[idx].spent = true;
                    self.outputs[idx].spent_height = Some(height);
                }
            }
            true
        } else {
            false
        }
    }

    /// Remove expired pending spends (older than PENDING_SPEND_TTL_SECS).
    /// Returns the tx_ids of transactions whose pending spends were all cleaned up.
    pub fn cleanup_expired_pending_spends(&mut self, now_secs: u64) -> Vec<String> {
        let expired: Vec<String> = self
            .pending_spends
            .iter()
            .filter(|(_, ps)| now_secs.saturating_sub(ps.created_at_secs) >= PENDING_SPEND_TTL_SECS)
            .map(|(ki, _)| ki.clone())
            .collect();

        let mut affected_txs: HashSet<String> = HashSet::new();
        for ki in &expired {
            if let Some(ps) = self.pending_spends.remove(ki) {
                affected_txs.insert(ps.tx_id.clone());
            }
        }

        // Return tx_ids where ALL pending spends are now gone
        affected_txs
            .into_iter()
            .filter(|tx_id| !self.pending_spends.values().any(|ps| &ps.tx_id == tx_id))
            .collect()
    }

    /// Check if an output is pending-spent.
    pub fn is_pending_spent(&self, key_image: &str) -> bool {
        self.pending_spends.contains_key(key_image)
    }

    /// Get all pending spends.
    pub fn pending_spends(&self) -> &HashMap<String, PendingSpend> {
        &self.pending_spends
    }

    /// Get pending spends for a specific transaction.
    pub fn pending_spends_for_tx(&self, tx_id: &str) -> Vec<&PendingSpend> {
        self.pending_spends
            .values()
            .filter(|ps| ps.tx_id == tx_id)
            .collect()
    }

    /// Collect all pending key images into a HashSet (for passing to coin selection).
    pub fn pending_key_images(&self) -> HashSet<String> {
        self.pending_spends.keys().cloned().collect()
    }

    // ---- Tracked transaction management ----

    /// Track a new transaction.
    pub fn track_transaction(&mut self, tx: TrackedTransaction) {
        self.tracked_transactions.insert(tx.tx_id.clone(), tx);
    }

    /// Advance a tracked transaction's status.
    /// Returns the new status if the transaction exists.
    pub fn advance_tx_status(&mut self, tx_id: &str, new_status: TxStatus) -> Option<&TxStatus> {
        if let Some(tx) = self.tracked_transactions.get_mut(tx_id) {
            tx.status = new_status;
            Some(&tx.status)
        } else {
            None
        }
    }

    /// Get a tracked transaction by ID.
    pub fn get_tracked_tx(&self, tx_id: &str) -> Option<&TrackedTransaction> {
        self.tracked_transactions.get(tx_id)
    }

    /// Get all tracked transactions.
    pub fn tracked_transactions(&self) -> &HashMap<String, TrackedTransaction> {
        &self.tracked_transactions
    }

    /// Find a tracked transaction by one of its spent key images.
    pub fn find_tx_by_key_image(&self, key_image: &str) -> Option<&TrackedTransaction> {
        self.tracked_transactions
            .values()
            .find(|tx| tx.spent_key_images.iter().any(|ki| ki == key_image))
    }

    /// Restore pending spends from persistence, filtering out expired entries.
    pub fn restore_pending_spends(&mut self, spends: HashMap<String, PendingSpend>, now_secs: u64) {
        self.loaded_without_rings |= !self.rings_restored;
        for (ki, ps) in spends {
            if now_secs.saturating_sub(ps.created_at_secs) < PENDING_SPEND_TTL_SECS {
                self.pending_spends.insert(ki, ps);
            }
        }
    }

    /// Restore tracked transactions from persistence.
    ///
    /// Also rebuilds `spent_by_tx` from confirmed transactions so that
    /// subsequent rescans can identify idempotent key-image observations.
    pub fn restore_tracked_transactions(&mut self, txs: HashMap<String, TrackedTransaction>) {
        self.loaded_without_rings |= !self.rings_restored;
        for (id, tx) in txs {
            if matches!(tx.status, TxStatus::Confirmed { .. } | TxStatus::Broadcast) {
                for ki in &tx.spent_key_images {
                    self.spent_by_tx.insert(ki.clone(), tx.tx_id.clone());
                }
            }
            self.tracked_transactions.insert(id, tx);
        }
    }

    /// Remove tracked transactions older than `max_age_secs` that are already confirmed.
    pub fn cleanup_old_tracked_txs(&mut self, now_secs: u64, max_age_secs: u64) -> usize {
        let before = self.tracked_transactions.len();
        self.tracked_transactions.retain(|_, tx| {
            if matches!(tx.status, TxStatus::Confirmed { .. }) {
                now_secs.saturating_sub(tx.created_at_secs) < max_age_secs
            } else {
                true
            }
        });
        before - self.tracked_transactions.len()
    }

    /// Import externally-provided key images into outputs that lack them
    /// (e.g. view-only wallets).  Key images are matched to outputs by
    /// position: `key_images[0]` maps to `outputs[offset]`, etc.
    ///
    /// This matches monero-wallet-rpc's `export_key_images` format, where
    /// `offset` and the ordered array of key image hex strings correspond to
    /// the wallet's internal transfer list (chronological output order).
    ///
    /// Return the number of updated outputs, or zero on rejection.
    /// Use `try_import_key_images` to receive the error.
    pub fn import_key_images(&mut self, offset: usize, key_images: &[String]) -> usize {
        self.try_import_key_images(offset, key_images).unwrap_or(0)
    }

    /// Import key images, returning an error if the list is invalid.
    pub fn try_import_key_images(
        &mut self,
        offset: usize,
        key_images: &[String],
    ) -> Result<usize, String> {
        let key_images: Vec<String> = key_images
            .iter()
            .map(|key_image| crate::ring_db::canonical_key_image(key_image))
            .collect();
        let key_images = key_images.as_slice();
        // Reject an export longer than the known output list, as Monero does.
        // The wallet may need to sync before it can apply the entire export.
        if offset.saturating_add(key_images.len()) > self.outputs.len() {
            return Err(format!(
                "import covers {} outputs but the wallet holds {}; rescan before importing",
                offset.saturating_add(key_images.len()),
                self.outputs.len()
            ));
        }

        // Reject duplicate key images. Sharing one image between two outputs
        // would leave only one reachable through the index when marking spends.
        let mut incoming: HashSet<&str> = HashSet::new();
        for ki in key_images.iter().filter(|k| !k.is_empty()) {
            if !incoming.insert(ki.as_str()) {
                return Err(format!(
                    "key image {ki} appears more than once in the import"
                ));
            }
        }
        let end = offset + key_images.len();
        for (idx, output) in self.outputs.iter().enumerate() {
            if (offset..end).contains(&idx) || output.key_image.is_empty() {
                continue;
            }
            if incoming.contains(output.key_image.as_str()) {
                return Err(format!(
                    "key image {} is already held by an output the import does not cover",
                    output.key_image
                ));
            }
        }

        let mut count = 0;
        for (i, ki) in key_images.iter().enumerate() {
            if ki.is_empty() {
                continue;
            }
            self.outputs[offset + i].key_image = ki.clone();
            count += 1;
        }

        // Rebuild after all assignments. Incremental removal could erase an
        // entry just inserted for another output when the images are permuted.
        self.rebuild_key_image_index();
        self.attach_rings();
        Ok(count)
    }

    /// Assign key images in confirmed blockchain scan order, then rebuild the
    /// index. Outputs within each height must retain their original scan order;
    /// an output index is only a position within its own transaction.
    ///
    /// This list has no signatures, so ownership cannot be verified. Prefer
    /// `assign_verified_key_images_in_export_order` for signed imports.
    pub fn assign_key_images_in_export_order(
        &mut self,
        key_images: &[String],
    ) -> Result<usize, String> {
        let confirmed = self.sort_into_export_order();
        if key_images.len() > confirmed {
            return Err(format!(
                "import covers {} outputs but the wallet holds {confirmed} confirmed outputs; rescan before importing",
                key_images.len()
            ));
        }
        self.try_import_key_images(0, key_images)
    }

    /// Verify each key image against its output before assigning any entries.
    ///
    /// Check the key-image subgroup and ring signature against the output's
    /// one-time public key, as Monero does. File authentication alone is
    /// insufficient: an untrusted online wallet has the view key and can reorder
    /// or replace entries.
    ///
    /// Reject the entire import if any entry fails verification.
    ///
    /// Apply entry `n` to output `n + offset`. Partial exports from
    /// `monero-wallet-cli export_key_images` record skipped leading outputs in
    /// this offset; ignoring it would pair signatures with the wrong outputs.
    pub fn assign_verified_key_images_in_export_order(
        &mut self,
        import: &crate::key_image_import::SignedKeyImageImport,
    ) -> Result<usize, String> {
        use curve25519_dalek::edwards::CompressedEdwardsY;

        let confirmed = self.sort_into_export_order();

        let offset = import.offset as usize;
        let signed = &import.key_images;

        // Check the offset before the count, as Monero does. Even an empty
        // export must not start beyond the wallet's known outputs.
        if offset > confirmed {
            return Err(format!(
                "the import starts at output {offset} but the wallet holds {}; rescan before \
                 importing",
                confirmed
            ));
        }

        if signed.len() > confirmed - offset {
            return Err(format!(
                "import covers outputs {}..{} but the wallet holds {}; rescan before importing",
                offset,
                offset + signed.len(),
                confirmed
            ));
        }

        for (i, ski) in signed.iter().enumerate() {
            let at = offset + i;
            let output = &self.outputs[at];

            let key_bytes: [u8; 32] = hex::decode(&output.key)
                .map_err(|e| format!("output {at} has an unreadable public key: {e}"))?
                .try_into()
                .map_err(|_| format!("output {at}'s public key is not 32 bytes"))?;
            let output_key = CompressedEdwardsY(key_bytes)
                .decompress()
                .ok_or_else(|| format!("output {at}'s public key is not a curve point"))?;

            let key_image = CompressedEdwardsY(ski.key_image)
                .decompress()
                .ok_or_else(|| format!("key image {i} is not a curve point"))?;

            if !crate::key_image_import::verify_key_image_signature(
                &output_key,
                &key_image,
                &ski.signature,
            ) {
                return Err(format!(
                    "key image {i} does not belong to the output it was paired with (output {at})"
                ));
            }
        }

        self.try_import_key_images(offset, &import.key_images_hex())
    }

    /// Confirmed outputs in wallet2's transfer order, then pool outputs; returns
    /// how many are confirmed.
    fn sort_into_export_order(&mut self) -> usize {
        let slot_heights = &self.slot_heights;
        self.outputs.sort_by_key(|o| {
            let height = one_time_key(o)
                .and_then(|key| slot_heights.get(key))
                .copied()
                .unwrap_or(o.block_height);
            (o.block_height == 0, height)
        });
        self.rebuild_key_image_index();
        self.outputs.partition_point(|o| o.block_height != 0)
    }

    fn rebuild_key_image_index(&mut self) {
        self.key_image_index.clear();
        self.output_index.clear();
        self.pub_key_index.clear();
        for (i, output) in self.outputs.iter_mut().enumerate() {
            output.key_image = crate::ring_db::canonical_key_image(&output.key_image);
            self.output_index.insert(output.output_key(), i);
            if let Some(key) = one_time_key(output) {
                self.pub_key_index.entry(key.to_string()).or_insert(i);
            }
            if !output.key_image.is_empty() {
                self.key_image_index.insert(output.key_image.clone(), i);
            }
        }
        let held = &self.pub_key_index;
        self.slot_heights.retain(|key, _| held.contains_key(key));
    }

    fn export_height(&self, index: usize) -> u64 {
        let output = &self.outputs[index];
        one_time_key(output)
            .and_then(|key| self.slot_heights.get(key))
            .copied()
            .unwrap_or(output.block_height)
    }
}

fn one_time_key(output: &WalletOutput) -> Option<&str> {
    let key = output.key.as_str();
    (key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())).then_some(key)
}

/// Read the platform clock once per balance/selection query. Keep unavailable
/// clocks explicit so time-locked funds fail closed on every target.
pub(crate) fn current_time_secs() -> Option<u64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let millis = js_sys::Date::now();
        if millis.is_finite() && millis >= 0.0 {
            Some((millis / 1000.0) as u64)
        } else {
            None
        }
    }
}

/// Check spendability at `height` using the current system time.
/// Use `is_spendable_at` to supply adjusted network time or an unavailable clock.
pub fn is_spendable(output: &WalletOutput, height: u64) -> bool {
    is_spendable_at(output, height, current_time_secs())
}

/// Whether `output` is unlocked at `height`; with no clock, time locks hold.
/// Pre-RingCT outputs pass: `WalletState::spendable_outputs` leaves them out.
pub fn is_spendable_at(output: &WalletOutput, height: u64, now_secs: Option<u64>) -> bool {
    use monero_wallet::transaction::Timelock;

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
    if confirmations < required {
        return false;
    }

    // In addition to spendable age, enforce the transaction's `unlock_time`,
    // as Monero's `is_transfer_unlocked` does. Selecting a still-locked output
    // would produce a transaction the daemon rejects.
    match output.additional_timelock() {
        Timelock::None => true,
        // Monero allows one block of slack against `height - 1`,
        // which is equivalent to `height >= unlock`.
        Timelock::Block(unlock) => height >= unlock as u64,
        // Monero allows 120 seconds of slack against adjusted time.
        Timelock::Time(unlock) => now_secs.is_some_and(|now| now.saturating_add(120) >= unlock),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output(amount: u64, height: u64, key_image: &str) -> WalletOutput {
        WalletOutput {
            tx_hash: format!("tx_{}", key_image),
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
            key_image: key_image.into(),
            is_coinbase: false,
            frozen: false,
            pre_ringct: false,
        }
    }

    /// Serialize a test output with the requested timelock.
    ///
    /// Assemble the `monero-wallet` fields directly because they are private.
    /// Read the fixture back to check its layout before using it in tests.
    fn output_bytes_with_timelock(timelock: monero_wallet::transaction::Timelock) -> String {
        use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;

        let point = ED25519_BASEPOINT_POINT.compress().to_bytes();
        let scalar = [0u8; 32];

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0u8; 32]); // absolute_id.transaction
        bytes.extend_from_slice(&0u64.to_le_bytes()); // absolute_id.index_in_transaction
        bytes.extend_from_slice(&0u64.to_le_bytes()); // relative_id.index_on_blockchain
        bytes.extend_from_slice(&point); // data.key
        bytes.extend_from_slice(&scalar); // data.key_offset
        bytes.extend_from_slice(&scalar); // data.commitment.mask
        bytes.extend_from_slice(&1u64.to_le_bytes()); // data.commitment.amount
        bytes.extend_from_slice(&timelock.serialize()); // metadata.additional_timelock
        bytes.push(0); // metadata.subaddress: None
        bytes.push(0); // metadata.payment_id: None
        bytes.push(0); // metadata.arbitrary_data: varint 0

        let hex_bytes = hex::encode(&bytes);

        // Verify the serialized timelock before using this fixture.
        let parsed = monero_wallet::WalletOutput::read(&mut bytes.as_slice())
            .expect("the hand-assembled fixture must parse with oxide's reader");
        assert_eq!(
            parsed.additional_timelock(),
            timelock,
            "the fixture must carry the timelock it was built with"
        );

        hex_bytes
    }

    fn make_timelocked_output(
        amount: u64,
        height: u64,
        key_image: &str,
        timelock: monero_wallet::transaction::Timelock,
    ) -> WalletOutput {
        let mut o = make_output(amount, height, key_image);
        o.received_output_bytes = output_bytes_with_timelock(timelock);
        o
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
        use crate::tx_prepare::{prepare_send_inputs_at, prepare_sweep_inputs_at};
        for is_coinbase in [false, true] {
            let mut pool = make_output(1_000_000_000_000, 0, "pool");
            pool.is_coinbase = is_coinbase;
            let mut state = WalletState::new();
            state.add_outputs(vec![pool.clone()]);
            for height in [10, 60, 3_000_000, u64::MAX] {
                assert!(!is_spendable_at(&pool, height, Some(u64::MAX)));
                assert_eq!(
                    state.balance_at(height, Some(u64::MAX)).unconfirmed,
                    pool.amount
                );
                assert_eq!(state.balance_at(height, Some(u64::MAX)).confirmed, 0);
                assert!(state
                    .spendable_outputs_at(height, Some(u64::MAX))
                    .is_empty());
                let selected = [pool.output_key()];
                for manual in [None, Some(selected.as_slice())] {
                    assert!(prepare_send_inputs_at(
                        state.outputs(),
                        height,
                        Some(u64::MAX),
                        1,
                        1,
                        manual,
                        None
                    )
                    .is_err());
                    assert!(prepare_sweep_inputs_at(
                        state.outputs(),
                        height,
                        Some(u64::MAX),
                        manual,
                        None
                    )
                    .is_err());
                }
            }
        }
    }

    #[test]
    fn pre_ringct_outputs_are_counted_apart_and_never_offered() {
        let mut state = WalletState::new();
        let mut old = make_output(3_000, 50, "ki_old");
        old.pre_ringct = true;
        let mut old_locked = make_output(5_000, 195, "ki_old_locked");
        old_locked.pre_ringct = true;
        state.add_outputs(vec![
            old,
            old_locked,
            make_output(1_000, 100, "ki_new"),
            make_output(2_000, 195, "ki_new_locked"),
        ]);

        let balance = state.balance_at(200, None);
        assert_eq!(balance.confirmed, 1_000);
        assert_eq!(balance.unconfirmed, 2_000);
        assert_eq!(balance.pre_ringct, 8_000);
        let offered: Vec<&str> = state
            .spendable_outputs_at(200, None)
            .iter()
            .map(|o| o.key_image.as_str())
            .collect();
        assert_eq!(offered, ["ki_new"]);
        assert!(is_spendable_at(&state.outputs()[0], 200, None));

        assert_eq!(
            state.mark_spent_by_key_images_at_height(&["ki_old".to_string()], 210),
            1
        );
        assert_eq!(state.outputs()[0].spent_height, Some(210));
        assert_eq!(state.balance_at(220, None).pre_ringct, 5_000);
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
            make_output(1_000_000_000_000, 80, "ki1"), // 20 confs -> confirmed
            make_output(2_000_000_000_000, 95, "ki2"), // 5 confs -> unconfirmed
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
    fn recorded_rings_follow_key_images_into_prepared_inputs() {
        use crate::tx_builder::native::{DecoySelection, InputRing, RingMember};

        let ring = DecoySelection {
            real_index: 0,
            offsets: vec![42; 16],
            ring: vec![
                RingMember {
                    key: "00".repeat(32),
                    commitment: "00".repeat(32),
                };
                16
            ],
        };
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 100, "ki1")]);
        state
            .record_input_rings(&[InputRing {
                key_image: "ki1".into(),
                ring: ring.clone(),
            }])
            .unwrap();
        let stored: crate::tx_builder::native::StoredOutputData = state.outputs()[0].clone().into();
        assert_eq!(stored.key_image, "ki1");
        assert_eq!(stored.ring, Some(ring));
    }

    #[test]
    fn recording_rings_is_atomic_and_never_replaces_a_ring() {
        use crate::tx_builder::native::{DecoySelection, InputRing, RingMember};

        let ring = |offset| DecoySelection {
            real_index: 0,
            offsets: vec![offset; 16],
            ring: vec![
                RingMember {
                    key: "00".repeat(32),
                    commitment: "00".repeat(32),
                };
                16
            ],
        };
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1"), make_output(1, 100, "ki2")]);
        state
            .record_input_rings(&[InputRing {
                key_image: "ki1".into(),
                ring: ring(1),
            }])
            .unwrap();

        let error = state
            .record_input_rings(&[
                InputRing {
                    key_image: "ki1".into(),
                    ring: ring(2),
                },
                InputRing {
                    key_image: "missing".into(),
                    ring: ring(3),
                },
            ])
            .unwrap_err();
        assert!(error.contains("replace") || error.contains("not in wallet"));
        assert_eq!(state.outputs()[0].ring, Some(ring(1)));
        assert!(state.outputs()[1].ring.is_none());
    }

    fn test_ring(offset: u64) -> DecoySelection {
        use crate::tx_builder::native::RingMember;
        DecoySelection {
            real_index: 0,
            offsets: vec![offset; 16],
            ring: vec![
                RingMember {
                    key: "00".repeat(32),
                    commitment: "00".repeat(32),
                };
                16
            ],
        }
    }

    fn input_ring(key_image: &str, offset: u64) -> crate::tx_builder::native::InputRing {
        crate::tx_builder::native::InputRing {
            key_image: key_image.into(),
            ring: test_ring(offset),
        }
    }

    #[test]
    fn ring_survives_a_reorg_of_the_receiving_block() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1"), make_output(1, 90, "ki2")]);
        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();

        let rollback = state.rollback_to_height(100);
        assert_eq!(rollback.removed_key_images, vec!["ki1".to_string()]);
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.ring_database().ring("ki1"), Some(&test_ring(7)));
        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();
        state
            .record_input_rings(&[input_ring("ki1", 8)])
            .unwrap_err();

        state.add_outputs(vec![make_output(1, 101, "ki1")]);
        let found = state
            .outputs()
            .iter()
            .find(|o| o.key_image == "ki1")
            .unwrap();
        assert_eq!(found.block_height, 101);
        assert_eq!(found.ring, Some(test_ring(7)));
    }

    #[test]
    fn replacing_outputs_keeps_the_ring_database() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1")]);
        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();

        state.replace_outputs(vec![make_output(1, 100, "ki1")]);
        assert_eq!(state.outputs()[0].ring, Some(test_ring(7)));
    }

    #[test]
    fn ring_stored_on_an_output_record_is_taken_into_the_database() {
        let mut stored = make_output(1, 100, "ki1");
        stored.ring = Some(test_ring(7));
        let mut state = WalletState::new();
        state.replace_outputs(vec![stored]);
        assert_eq!(state.ring_database().ring("ki1"), Some(&test_ring(7)));

        let mut stale = make_output(1, 100, "ki1");
        stale.ring = Some(test_ring(8));
        state.replace_outputs(vec![stale]);
        assert_eq!(state.outputs()[0].ring, Some(test_ring(7)));
        state.with_outputs_mut(|outputs| outputs[0].ring = None);
        assert_eq!(state.outputs()[0].ring, Some(test_ring(7)));
    }

    #[test]
    fn stored_ring_database_wins_over_output_records_in_either_order() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1")]);
        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();
        let stored = state.ring_database().clone();
        let mut stale = make_output(1, 100, "ki1");
        stale.ring = Some(test_ring(8));

        let mut outputs_first = WalletState::new();
        outputs_first.replace_outputs(vec![stale.clone()]);
        outputs_first.restore_ring_database(stored.clone()).unwrap();
        assert_eq!(
            outputs_first.ring_database().ring("ki1"),
            Some(&test_ring(7))
        );
        assert_eq!(outputs_first.outputs()[0].ring, Some(test_ring(7)));

        let mut database_first = WalletState::new();
        database_first.restore_ring_database(stored).unwrap();
        database_first.replace_outputs(vec![stale]);
        assert_eq!(
            database_first.ring_database(),
            outputs_first.ring_database()
        );
        assert_eq!(database_first.outputs()[0].ring, Some(test_ring(7)));
    }

    #[test]
    fn key_images_are_stored_and_compared_in_lower_case() {
        let upper = "AB".repeat(32);
        let lower = "ab".repeat(32);
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, &upper)]);
        assert_eq!(state.outputs()[0].key_image, lower);
        state.record_input_rings(&[input_ring(&upper, 7)]).unwrap();
        assert_eq!(state.ring_database().ring(&lower), Some(&test_ring(7)));
        assert_eq!(state.ring_database().ring(&upper), Some(&test_ring(7)));
        state
            .record_input_rings(&[input_ring(&lower, 8)])
            .unwrap_err();

        let json = serde_json::to_string(state.ring_database()).unwrap();
        let stored: RingDatabase = serde_json::from_str(&json.replace(&lower, &upper)).unwrap();
        let mut reloaded = WalletState::new();
        reloaded.restore_ring_database(stored).unwrap();
        reloaded.add_outputs(vec![make_output(1, 100, &lower)]);
        assert_eq!(reloaded.outputs()[0].ring, Some(test_ring(7)));

        let mut view_only = WalletState::new();
        view_only.add_outputs(vec![make_output(1, 100, "")]);
        view_only.try_import_key_images(0, &[upper]).unwrap();
        assert_eq!(view_only.outputs()[0].key_image, lower);
    }

    #[test]
    fn clearing_a_ring_removes_it_from_the_database_and_the_record() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1"), make_output(1, 100, "ki2")]);
        state
            .record_input_rings(&[input_ring("ki1", 7), input_ring("ki2", 9)])
            .unwrap();

        assert_eq!(state.clear_ring("ki1"), Some(test_ring(7)));
        assert_eq!(state.clear_ring("ki1"), None);
        assert!(state.ring_database().ring("ki1").is_none());
        assert!(state.outputs()[0].ring.is_none());
        assert_eq!(state.outputs()[1].ring, Some(test_ring(9)));

        state.add_outputs(vec![make_output(1, 100, "ki1")]);
        assert!(state.outputs()[0].ring.is_none());
        state.record_input_rings(&[input_ring("ki1", 8)]).unwrap();
        assert_eq!(state.outputs()[0].ring, Some(test_ring(8)));
    }

    #[test]
    fn a_cleared_ring_stays_cleared_against_copies_saved_before() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1")]);
        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();
        let saved_outputs = state.outputs().to_vec();
        state.clear_ring("ki1").unwrap();
        assert!(state.ring_database().is_cleared("ki1"));
        let json = serde_json::to_string(state.ring_database()).unwrap();
        let stored: RingDatabase = serde_json::from_str(&json).unwrap();
        assert_eq!(&stored, state.ring_database());

        let mut outputs_first = WalletState::new();
        outputs_first.replace_outputs(saved_outputs.clone());
        outputs_first.restore_ring_database(stored.clone()).unwrap();
        let mut database_first = WalletState::new();
        database_first.restore_ring_database(stored).unwrap();
        database_first.replace_outputs(saved_outputs.clone());
        for restarted in [&mut outputs_first, &mut database_first] {
            assert!(restarted.ring_database().ring("ki1").is_none());
            assert!(restarted.outputs()[0].ring.is_none());
            restarted.replace_outputs(saved_outputs.clone());
            assert!(restarted.ring_database().ring("ki1").is_none());
            restarted
                .record_input_rings(&[input_ring("ki1", 8)])
                .unwrap();
            assert!(!restarted.ring_database().is_cleared("ki1"));
            assert_eq!(restarted.outputs()[0].ring, Some(test_ring(8)));
        }
    }

    #[test]
    fn spent_rings_are_learned_for_own_key_images_only() {
        let mut state = WalletState::new();
        let mut owned = make_output(1, 100, "ki1");
        owned.global_index = Some(12);
        state.add_outputs(vec![owned, make_output(1, 100, "ki2")]);
        state.record_input_rings(&[input_ring("ki2", 9)]).unwrap();

        let key_images: Vec<String> = ["ki1", "other", "ki2", "ki1"]
            .iter()
            .map(|k| k.to_string())
            .collect();
        let rings = vec![vec![7, 5, 10], vec![1, 1, 1], vec![9; 16], vec![]];
        assert_eq!(state.learn_spent_rings(&key_images, &rings), 1);

        let learned = state.ring_database().ring("ki1").unwrap();
        assert_eq!(learned.offsets, vec![7, 5, 10]);
        assert_eq!(learned.real_index, 1);
        assert!(learned.ring.is_empty());
        assert!(state.ring_database().ring("other").is_none());
        assert_eq!(state.ring_database().ring("ki2"), Some(&test_ring(9)));
        assert_eq!(state.learn_spent_rings(&key_images, &rings), 0);

        assert!(state.ring_conflicts().is_empty());
        assert_eq!(
            state.learn_spent_rings(&["ki2".to_string()], &[vec![8; 16]]),
            0
        );
        assert_eq!(state.ring_database().ring("ki2"), Some(&test_ring(9)));
        let seen = DecoySelection::from_offsets(&[8; 16], 0).indexes().unwrap();
        assert_eq!(
            state.take_ring_conflicts(),
            vec![crate::ring_db::ChainRingConflict {
                key_image: "ki2".into(),
                recorded: test_ring(9).indexes().unwrap(),
                seen,
            }]
        );
        assert!(state.ring_conflicts().is_empty());

        let mut known = make_output(1, 100, "ki3");
        known.global_index = Some(40);
        state.add_outputs(vec![known]);
        assert_eq!(
            state.learn_spent_rings(&["ki3".to_string()], &[vec![30, 5, 6]]),
            0
        );
        assert!(state.ring_database().ring("ki3").is_none());
        assert!(state.ring_conflicts().is_empty());

        let mut with_keys = test_ring(1);
        with_keys.offsets = vec![7, 5, 10];
        with_keys.ring.truncate(3);
        with_keys.real_index = 1;
        state
            .record_input_rings(&[crate::tx_builder::native::InputRing {
                key_image: "ki1".into(),
                ring: with_keys.clone(),
            }])
            .unwrap();
        assert_eq!(state.ring_database().ring("ki1"), Some(&with_keys));
        assert_eq!(state.outputs()[0].ring, Some(with_keys));
    }

    #[test]
    fn a_ring_learned_before_the_stored_database_is_loaded_gives_way_to_it() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1")]);
        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();
        let stored = state.ring_database().clone();

        let mut restarted = WalletState::new();
        restarted.add_outputs(vec![make_output(1, 100, "ki1")]);
        assert_eq!(
            restarted.learn_spent_rings(&["ki1".to_string()], &[vec![8; 16]]),
            1
        );
        restarted.restore_ring_database(stored).unwrap();
        assert_eq!(restarted.ring_database().ring("ki1"), Some(&test_ring(7)));
        assert_eq!(restarted.outputs()[0].ring, Some(test_ring(7)));
        let conflicts = restarted.ring_conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].recorded, test_ring(7).indexes().unwrap());
    }

    #[test]
    fn candidate_sets_are_fixed_once_recorded() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1"), make_output(1, 100, "ki2")]);
        let first: Vec<u64> = (1..=75).collect();
        let key = |k: &str| vec![k.to_string()];
        state
            .record_ring_candidates(&key("ki1"), &[first.clone()])
            .unwrap();
        state
            .record_ring_candidates(&key("ki1"), &[first.clone()])
            .unwrap();
        let other: Vec<u64> = (2..=76).collect();
        state
            .record_ring_candidates(&key("ki1"), &[other.clone()])
            .unwrap_err();
        assert_eq!(
            state.ring_database().candidates("ki1"),
            Some(first.as_slice())
        );

        let mut within = test_ring(1);
        within.offsets = vec![1; 16];
        state
            .record_input_rings(&[crate::tx_builder::native::InputRing {
                key_image: "ki1".into(),
                ring: within,
            }])
            .unwrap();
        assert_eq!(
            state.ring_database().candidates("ki1"),
            Some(first.as_slice())
        );
        let stored: RingDatabase =
            serde_json::from_str(&serde_json::to_string(state.ring_database()).unwrap()).unwrap();
        let mut reloaded = WalletState::new();
        reloaded.restore_ring_database(stored.clone()).unwrap();
        assert_eq!(reloaded.ring_database(), state.ring_database());
        let mut conflicting = WalletState::new();
        conflicting.add_outputs(vec![make_output(1, 100, "ki1")]);
        conflicting
            .record_ring_candidates(&key("ki1"), &[other.clone()])
            .unwrap();
        conflicting.restore_ring_database(stored).unwrap_err();

        state
            .record_ring_candidates(&key("ki2"), &[first.clone()])
            .unwrap();
        let error = state
            .record_input_rings(&[input_ring("ki2", 9)])
            .unwrap_err();
        assert!(
            error.contains("not drawn from the 75 candidates"),
            "{error}"
        );
        assert_eq!(state.learn_spent_rings(&key("ki2"), &[vec![9; 16]]), 0);
        assert_eq!(state.ring_conflicts().len(), 1);
        assert_eq!(state.ring_conflicts()[0].recorded, first);
        assert!(state.ring_database().ring("ki2").is_none());
        assert_eq!(
            state.ring_database().candidates("ki2"),
            Some(first.as_slice())
        );

        state.clear_ring("ki1").unwrap();
        assert!(state.ring_database().candidates("ki1").is_none());
        state.record_ring_candidates(&key("ki1"), &[other]).unwrap();
    }

    #[test]
    fn restore_refuses_or_repairs_a_ring_outside_the_stored_candidates() {
        let candidates: Vec<u64> = (1..=75).collect();
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1")]);
        state
            .record_ring_candidates(&["ki1".to_string()], &[candidates])
            .unwrap();
        let stored = state.ring_database().clone();

        let mut recorded = WalletState::new();
        recorded.add_outputs(vec![make_output(1, 100, "ki1")]);
        recorded
            .record_input_rings(&[input_ring("ki1", 9)])
            .unwrap();
        let before = recorded.ring_database().clone();
        let error = recorded.restore_ring_database(stored.clone()).unwrap_err();
        assert!(error.contains("does not contain the ring"), "{error}");
        assert_eq!(recorded.ring_database(), &before);

        let mut record = make_output(1, 100, "ki1");
        record.ring = Some(test_ring(9));
        let mut adopted = WalletState::new();
        adopted.add_outputs(vec![record]);
        adopted.restore_ring_database(stored.clone()).unwrap();
        assert!(adopted.ring_database().ring("ki1").is_none());
        assert_eq!(
            adopted.ring_database().candidates("ki1"),
            stored.candidates("ki1")
        );
        assert!(adopted.outputs()[0].ring.is_none());
    }

    struct CountingPersister {
        stored: std::sync::Arc<std::sync::Mutex<Vec<RingDatabase>>>,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl RingPersister for CountingPersister {
        async fn persist(&mut self, rings: &RingDatabase) -> Result<(), String> {
            if self.fail {
                return Err("disk full".to_string());
            }
            self.stored.lock().unwrap().push(rings.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn ring_database_is_handed_to_the_persister_only_when_it_changed() {
        let stored = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1")]);

        state.persist_rings().await.unwrap();

        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();
        assert!(state.has_unsaved_rings());
        let error = state.persist_rings().await.unwrap_err();
        assert!(error.contains("no ring persister"), "{error}");

        state.set_ring_persister(Box::new(CountingPersister {
            stored: stored.clone(),
            fail: true,
        }));
        let error = state.persist_rings().await.unwrap_err();
        assert!(error.contains("disk full"), "{error}");
        assert!(
            state.has_unsaved_rings(),
            "a failed write leaves the changes unsaved"
        );

        state.set_ring_persister(Box::new(CountingPersister {
            stored: stored.clone(),
            fail: false,
        }));
        state.persist_rings().await.unwrap();
        state.persist_rings().await.unwrap();
        assert_eq!(
            stored.lock().unwrap().as_slice(),
            &[state.ring_database().clone()]
        );

        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();
        assert!(!state.has_unsaved_rings());
        state.clear_ring("ki1").unwrap();
        assert!(state.has_unsaved_rings());
        state.persist_rings().await.unwrap();
        assert_eq!(
            state.learn_spent_rings(&["ki1".to_string()], &[vec![8; 16]]),
            1
        );
        assert!(state.has_unsaved_rings());
        state.persist_rings().await.unwrap();
        assert_eq!(stored.lock().unwrap().len(), 3);

        let mut reloaded = WalletState::new();
        reloaded
            .restore_ring_database(stored.lock().unwrap()[1].clone())
            .unwrap();
        assert!(!reloaded.has_unsaved_rings());
    }

    #[tokio::test]
    async fn a_state_loaded_without_its_ring_database_stores_no_rings() {
        let stored = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut state = WalletState::new();
        state.set_ring_persister(Box::new(CountingPersister {
            stored: stored.clone(),
            fail: false,
        }));
        state.replace_outputs(vec![make_output(1, 100, "ki1")]);
        let error = state
            .record_input_rings(&[input_ring("ki1", 7)])
            .unwrap_err();
        assert!(error.contains("restore_ring_database"), "{error}");
        state
            .record_ring_candidates(&["ki1".to_string()], &[(1..=75).collect()])
            .unwrap_err();
        assert!(state.ring_database().is_empty());

        state
            .restore_ring_database(RingDatabase::default())
            .unwrap();
        state.record_input_rings(&[input_ring("ki1", 7)]).unwrap();
        state.persist_rings().await.unwrap();
        assert_eq!(stored.lock().unwrap().len(), 1);

        let mut throwaway = WalletState::new();
        throwaway.set_ring_persister(Box::new(crate::ring_db::RingsNotPersisted));
        throwaway.replace_outputs(vec![make_output(1, 100, "ki1")]);
        throwaway
            .record_input_rings(&[input_ring("ki1", 7)])
            .unwrap();
        throwaway.persist_rings().await.unwrap();
    }

    #[test]
    fn transaction_rings_are_recorded_or_refused_as_a_whole() {
        let raw_tx = |inputs: &[(&[u8], [u8; 32])]| {
            let mut blob = vec![2, 0, inputs.len() as u8];
            for (offsets, key_image) in inputs {
                blob.extend_from_slice(&[0x02, 0, offsets.len() as u8]);
                blob.extend_from_slice(offsets);
                blob.extend_from_slice(key_image);
            }
            blob.extend_from_slice(&[0, 0]);
            blob
        };
        let (ki1, ki2, foreign) = ([0x11; 32], [0x22; 32], [0x33; 32]);
        let mut state = WalletState::new();
        let mut first = make_output(1, 100, &hex::encode(ki1));
        first.global_index = Some(12);
        state.add_outputs(vec![first, make_output(1, 100, &hex::encode(ki2))]);

        let tx = raw_tx(&[(&[7, 5, 10], ki1), (&[1, 2, 3], foreign)]);
        assert_eq!(state.record_transaction_rings(&tx), Ok(1));
        let recorded = state
            .ring_database()
            .ring(&hex::encode(ki1))
            .unwrap()
            .clone();
        assert_eq!(recorded.offsets, vec![7, 5, 10]);
        assert_eq!(recorded.real_index, 1);
        assert_eq!(state.ring_database().len(), 1);
        assert!(state.has_unsaved_rings());
        assert_eq!(state.record_transaction_rings(&tx), Ok(0));

        let conflicting = raw_tx(&[(&[4, 4, 4], ki2), (&[7, 5, 11], ki1)]);
        let error = state.record_transaction_rings(&conflicting).unwrap_err();
        assert!(
            error.contains("Refusing to replace the persisted ring"),
            "{error}"
        );
        assert_eq!(state.ring_database().len(), 1);
        assert_eq!(
            state.ring_database().ring(&hex::encode(ki1)),
            Some(&recorded)
        );
    }

    #[test]
    fn ring_database_round_trips_and_never_replaces_a_ring_on_restore() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1, 100, "ki1"), make_output(1, 100, "ki2")]);
        state
            .record_input_rings(&[input_ring("ki1", 7), input_ring("ki2", 9)])
            .unwrap();
        let stored = serde_json::to_string(state.ring_database()).unwrap();
        let stored: RingDatabase = serde_json::from_str(&stored).unwrap();
        assert_eq!(&stored, state.ring_database());

        let mut reloaded = WalletState::new();
        reloaded.restore_ring_database(stored.clone()).unwrap();
        reloaded.add_outputs(vec![make_output(1, 100, "ki2")]);
        assert_eq!(reloaded.outputs()[0].ring, Some(test_ring(9)));

        let mut other = WalletState::new();
        other.add_outputs(vec![make_output(1, 100, "ki1"), make_output(1, 100, "ki3")]);
        other
            .record_input_rings(&[input_ring("ki1", 8), input_ring("ki3", 5)])
            .unwrap();
        let before = other.ring_database().clone();
        other.restore_ring_database(stored).unwrap_err();
        assert_eq!(
            other.ring_database(),
            &before,
            "nothing is loaded on a conflict"
        );
    }

    #[test]
    fn rescan_upgrades_a_placeholder_global_index() {
        let mut state = WalletState::new();
        let mut placeholder = make_output(1_000_000_000_000, 100, "ki1");
        placeholder.global_index = None;
        placeholder.received_output_bytes = "placeholder".into();
        state.add_outputs(vec![placeholder]);

        let mut refreshed = make_output(1_000_000_000_000, 100, "ki1");
        refreshed.global_index = Some(424_242);
        refreshed.received_output_bytes = "authoritative".into();
        state.add_outputs(vec![refreshed]);

        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.outputs()[0].global_index, Some(424_242));
        assert_eq!(state.outputs()[0].received_output_bytes, "authoritative");
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
    fn rescan_replaces_a_stored_global_index() {
        let mut state = WalletState::new();
        let mut first = make_output(1_000_000_000_000, 80, "ki1");
        first.global_index = Some(1_000_706);
        state.add_outputs(vec![first]);

        let mut rescanned = make_output(1_000_000_000_000, 80, "ki1");
        rescanned.global_index = Some(706);
        state.add_outputs(vec![rescanned]);
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.outputs()[0].global_index, Some(706));

        state.add_outputs(vec![make_output(1_000_000_000_000, 80, "ki1")]);
        assert_eq!(state.outputs()[0].global_index, Some(706));
    }

    #[test]
    fn test_spendable_outputs() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"), // spendable at height 100
            make_output(2_000_000_000_000, 95, "ki2"), // not spendable (5 confs)
        ]);
        state.current_height = 99;

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
        state.current_height = 99;
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
        state.current_height = 99;

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
    fn scan_and_import_keep_stable_output_identity() {
        let first = output_without_key_image("first", 100);
        let second = output_without_key_image("second", 100);
        let mut state = WalletState::new();
        state.add_outputs(vec![first.clone(), second.clone()]);
        assert_eq!(state.outputs().len(), 2);
        assert!(!state.freeze_output(""));
        assert_eq!(state.mark_spent_by_key_images(&[String::new()]), 0);
        state
            .try_import_key_images(0, &["known-first".into(), "known-second".into()])
            .unwrap();
        state.freeze_output("known-first");
        state.mark_spent_by_key_images_at_height(&["known-second".into()], 115);
        state.add_outputs(vec![first.clone(), second.clone()]);
        assert_eq!(state.outputs().len(), 2);
        assert!(state.outputs()[0].frozen);
        assert_eq!(state.outputs()[0].key_image, "known-first");
        assert_eq!(state.outputs()[1].spent_height, Some(115));
        assert_eq!(state.balance_at(200, None).confirmed, 0);
        assert!(state.thaw_output("known-first"));
        assert_eq!(state.balance_at(200, None).confirmed, first.amount);
        assert!(state.freeze_output("known-first"));

        // Restoring a cache merges duplicate records without losing local state.
        let mut persisted = state.outputs().to_vec();
        persisted.extend([first, second]);
        state.replace_outputs(persisted);
        assert_eq!(state.outputs().len(), 2);
        assert_eq!(state.balance_at(200, None).confirmed, 0);
        assert!(state.thaw_output("known-first"));
        assert_eq!(state.balance_at(200, None).confirmed, 1_000_000_000_000);
    }

    #[test]
    fn empty_image_confirmations_preserve_scan_order_and_local_state() {
        let first = output_without_key_image("first", 0);
        let second = output_without_key_image("second", 0);
        let mut state = WalletState::new();
        state.add_outputs(vec![second.clone(), first.clone()]);
        state
            .try_import_key_images(0, &["second-ki".into(), "first-ki".into()])
            .unwrap();
        state.freeze_output("first-ki");
        state.mark_spent_by_key_images_at_height(&["second-ki".into()], 111);
        let mut first = first;
        let mut second = second;
        first.block_height = 100;
        second.block_height = 100;
        state.add_outputs(vec![first.clone(), second]);
        assert_eq!(state.outputs().len(), 2);
        assert_eq!(state.outputs()[0].output_key(), first.output_key());
        assert!(state.outputs()[0].frozen);
        assert_eq!(state.outputs()[1].spent_height, Some(111));
        assert!(state.freeze_output("second-ki"));
        assert!(state.outputs()[1].frozen);
        state.rollback_to_height(100);
        assert!(state.outputs().is_empty());
        state.add_outputs(vec![first]);
        assert_eq!(state.outputs().len(), 1);
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
        second.frozen = true;
        state.add_outputs(vec![first.clone(), second.clone()]);
        assert_eq!(state.outputs().len(), 1);
        assert!(state.outputs()[0].frozen);
        assert_eq!(state.balance_at(200, None).confirmed, 0);
        assert!(state.thaw_output("shared"));
        assert_eq!(state.balance_at(200, None).confirmed, first.amount);
        state.mark_spent_by_key_images_at_height(&["shared".into()], 150);
        assert_eq!(state.balance_at(200, None).confirmed, 0);
        state.add_outputs(vec![first, second]);
        assert_eq!(state.outputs().len(), 1);
        assert_eq!(state.balance_at(200, None).confirmed, 0);
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
    fn short_chain_history_after_compaction_still_reaches_down_to_the_base() {
        let mut chain = BlockHashChain::new();
        chain.record_block(0, "genesis".into());
        for h in 2_990_000..=3_000_000u64 {
            chain.record_block(h, format!("hash_{}", h));
        }
        chain.compact();
        let history = chain.get_short_chain_history();
        let heights: Vec<u64> = history.iter().map(|(height, _)| *height).collect();
        assert!(heights.windows(2).all(|pair| pair[0] > pair[1]));
        assert!(history
            .iter()
            .all(|(height, hash)| chain.get_hash(*height) == Some(hash.as_str())));
        assert!(heights.iter().filter(|h| **h < 3_000_000 - 99).count() > 5);
        assert_eq!(heights[heights.len() - 2..], [2_990_000, 0]);
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
        let accounts: Vec<u32> = state
            .outputs()
            .iter()
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
        state.add_outputs(vec![make_output(5_000_000_000_000, 100, "ki_new")]);
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

    // ---- Freeze/thaw tests ----

    #[test]
    fn test_frozen_output_excluded_from_spendable() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"),
            make_output(2_000_000_000_000, 80, "ki2"),
        ]);
        state.current_height = 99;

        state.freeze_output("ki1");
        let spendable = state.spendable_outputs();
        assert_eq!(spendable.len(), 1);
        assert_eq!(spendable[0].key_image, "ki2");
    }

    #[test]
    fn test_thaw_output_restores_spendability() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 80, "ki1")]);
        state.current_height = 99;

        state.freeze_output("ki1");
        assert_eq!(state.spendable_outputs().len(), 0);

        state.thaw_output("ki1");
        assert_eq!(state.spendable_outputs().len(), 1);
    }

    #[test]
    fn test_freeze_unknown_key_image() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 80, "ki1")]);
        assert!(!state.freeze_output("unknown"));
        assert!(!state.thaw_output("unknown"));
    }

    #[test]
    fn test_frozen_output_excluded_from_balance() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 80, "ki1")]);
        state.current_height = 100;

        state.freeze_output("ki1");
        let bal = state.balance();
        assert_eq!(bal.confirmed, 0);
        assert_eq!(bal.unconfirmed, 0);

        state.thaw_output("ki1");
        assert_eq!(state.balance().confirmed, 1_000_000_000_000);
    }

    #[test]
    fn test_freeze_idempotent() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 80, "ki1")]);
        state.current_height = 99;

        assert!(state.freeze_output("ki1"));
        assert!(state.freeze_output("ki1")); // second freeze is fine
        assert_eq!(state.spendable_outputs().len(), 0);
        assert!(state.outputs()[0].frozen);
    }

    #[test]
    fn test_thaw_idempotent() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 80, "ki1")]);
        state.current_height = 99;

        // Thaw an already-thawed output
        assert!(state.thaw_output("ki1"));
        assert!(!state.outputs()[0].frozen);
        assert_eq!(state.spendable_outputs().len(), 1);
    }

    #[test]
    fn test_frozen_survives_rollback() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"),
            make_output(2_000_000_000_000, 90, "ki2"),
        ]);
        state.current_height = 119;

        state.freeze_output("ki1");
        state.rollback_to_height(105);
        // ki1 should still be frozen after rollback
        assert!(
            state
                .outputs()
                .iter()
                .find(|o| o.key_image == "ki1")
                .unwrap()
                .frozen
        );
        assert_eq!(state.spendable_outputs().len(), 1);
        assert_eq!(state.spendable_outputs()[0].key_image, "ki2");
    }

    #[test]
    fn test_frozen_excluded_from_account_spendable() {
        let mut state = WalletState::new();
        let mut o1 = make_output(1_000_000_000_000, 80, "ki1");
        o1.subaddress_index = Some((1, 0));
        let mut o2 = make_output(2_000_000_000_000, 80, "ki2");
        o2.subaddress_index = Some((1, 1));
        state.add_outputs(vec![o1, o2]);
        state.current_height = 99;

        state.freeze_output("ki1");
        let acct1 = state.spendable_outputs_for_accounts(&[1]);
        assert_eq!(acct1.len(), 1);
        assert_eq!(acct1[0].key_image, "ki2");
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

    // ---- Double-spend conflict detection tests ----

    #[test]
    fn test_conflict_different_tx() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);

        let (count, conflicts) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 100);
        assert_eq!(count, 1);
        assert!(conflicts.is_empty());

        // Same key image, different spending tx -> conflict
        let (count2, conflicts2) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_b".to_string()], 200);
        assert_eq!(count2, 0);
        assert_eq!(conflicts2.len(), 1);
        assert_eq!(
            conflicts2[0],
            SpentConflict {
                key_image: "ki1".to_string(),
                previous_spent_height: Some(100),
                new_height: 200,
            }
        );
    }

    #[test]
    fn test_no_conflict_same_tx_different_height() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);

        let (count, conflicts) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 100);
        assert_eq!(count, 1);
        assert!(conflicts.is_empty());

        // Same spending tx at different height (rescan with different batch boundary) -> no conflict
        let (count2, conflicts2) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 200);
        assert_eq!(count2, 0);
        assert!(conflicts2.is_empty());
    }

    #[test]
    fn test_no_conflict_same_height() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);

        let (count, conflicts) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 100);
        assert_eq!(count, 1);
        assert!(conflicts.is_empty());

        // Same height = idempotent, no conflict (even with empty tx hash)
        let (count2, conflicts2) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &[], 100);
        assert_eq!(count2, 0);
        assert!(conflicts2.is_empty());
    }

    #[test]
    fn test_no_conflict_broadcast_then_confirmed() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);

        // Mark spent without height (broadcast)
        state.mark_spent_by_key_images(&["ki1".to_string()]);
        assert!(state.outputs()[0].spent);
        assert_eq!(state.outputs()[0].spent_height, None);

        // Now confirmed at some height -> NOT a conflict, and updates spent_height
        let (count, conflicts) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 150);
        assert_eq!(count, 0);
        assert!(conflicts.is_empty());
        assert_eq!(state.outputs()[0].spent_height, Some(150));

        // Subsequent rescan with same tx at different height -> still no conflict
        let (count2, conflicts2) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 200);
        assert_eq!(count2, 0);
        assert!(conflicts2.is_empty());
    }

    #[test]
    fn test_mempool_conflict_check() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 60, "ki2"),
        ]);

        // Confirm ki1 spent at height 100
        state.mark_spent_by_key_images_at_height(&["ki1".to_string()], 100);

        // Check mempool key images against confirmed spends
        let conflicts = state.check_spent_conflicts(&["ki1".to_string(), "ki2".to_string()]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            conflicts[0],
            SpentConflict {
                key_image: "ki1".to_string(),
                previous_spent_height: Some(100),
                new_height: 0,
            }
        );
    }

    // ---- Pending spend tests ----

    fn make_pending_spend(
        tx_id: &str,
        key_image: &str,
        amount: u64,
        created_at: u64,
    ) -> PendingSpend {
        PendingSpend {
            tx_id: tx_id.to_string(),
            key_image: key_image.to_string(),
            output_key: format!("tx_{}:0", key_image),
            amount,
            created_at_secs: created_at,
        }
    }

    #[test]
    fn test_pending_spend_excludes_from_balance() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"),
            make_output(2_000_000_000_000, 80, "ki2"),
        ]);
        state.current_height = 200;

        state.add_pending_spends(
            "tx_abc",
            vec![make_pending_spend("tx_abc", "ki1", 1_000_000_000_000, 1000)],
        );

        let bal = state.balance();
        assert_eq!(bal.confirmed, 2_000_000_000_000);
        assert_eq!(bal.pending_spend, 1_000_000_000_000);
    }

    #[test]
    fn test_pending_spend_excludes_from_spendable() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"),
            make_output(2_000_000_000_000, 80, "ki2"),
        ]);
        state.current_height = 199;

        state.add_pending_spends(
            "tx_abc",
            vec![make_pending_spend("tx_abc", "ki1", 1_000_000_000_000, 1000)],
        );

        let spendable = state.spendable_outputs();
        assert_eq!(spendable.len(), 1);
        assert_eq!(spendable[0].key_image, "ki2");
    }

    #[test]
    fn test_confirm_pending_spend() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 80, "ki1")]);
        state.current_height = 200;

        state.add_pending_spends(
            "tx_abc",
            vec![make_pending_spend("tx_abc", "ki1", 1_000_000_000_000, 1000)],
        );
        assert!(state.is_pending_spent("ki1"));

        let confirmed = state.confirm_pending_spend("ki1", 150);
        assert!(confirmed);
        assert!(!state.is_pending_spent("ki1"));
        assert!(state.outputs()[0].spent);
        assert_eq!(state.outputs()[0].spent_height, Some(150));
    }

    #[test]
    fn test_confirm_pending_spend_unknown() {
        let mut state = WalletState::new();
        assert!(!state.confirm_pending_spend("unknown", 100));
    }

    #[test]
    fn test_cleanup_expired_pending_spends() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 80, "ki1"),
            make_output(2_000_000_000_000, 80, "ki2"),
        ]);

        state.add_pending_spends(
            "tx_old",
            vec![make_pending_spend("tx_old", "ki1", 1_000_000_000_000, 1000)],
        );
        state.add_pending_spends(
            "tx_new",
            vec![make_pending_spend("tx_new", "ki2", 2_000_000_000_000, 5000)],
        );

        // now=8200: tx_old expired (1000 + 7200 = 8200), tx_new not (5000 + 7200 = 12200)
        let cleaned = state.cleanup_expired_pending_spends(8200);
        assert_eq!(cleaned.len(), 1);
        assert!(cleaned.contains(&"tx_old".to_string()));
        assert!(!state.is_pending_spent("ki1"));
        assert!(state.is_pending_spent("ki2"));
    }

    #[test]
    fn test_pending_spends_for_tx() {
        let mut state = WalletState::new();
        state.add_pending_spends(
            "tx1",
            vec![
                make_pending_spend("tx1", "ki_a", 100, 1000),
                make_pending_spend("tx1", "ki_b", 200, 1000),
            ],
        );
        state.add_pending_spends("tx2", vec![make_pending_spend("tx2", "ki_c", 300, 1000)]);

        let tx1_spends = state.pending_spends_for_tx("tx1");
        assert_eq!(tx1_spends.len(), 2);
        let tx2_spends = state.pending_spends_for_tx("tx2");
        assert_eq!(tx2_spends.len(), 1);
    }

    #[test]
    fn test_pending_key_images() {
        let mut state = WalletState::new();
        state.add_pending_spends(
            "tx1",
            vec![
                make_pending_spend("tx1", "ki_a", 100, 1000),
                make_pending_spend("tx1", "ki_b", 200, 1000),
            ],
        );

        let ki = state.pending_key_images();
        assert_eq!(ki.len(), 2);
        assert!(ki.contains("ki_a"));
        assert!(ki.contains("ki_b"));
    }

    #[test]
    fn test_rollback_clears_pending_spends_for_removed_outputs() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 100, "ki2"),
        ]);
        state.add_pending_spends(
            "tx_abc",
            vec![make_pending_spend("tx_abc", "ki2", 2_000_000_000_000, 1000)],
        );

        state.rollback_to_height(80);
        // ki2 was removed (block_height 100 >= 80), so its pending spend should be cleared
        assert!(!state.is_pending_spent("ki2"));
    }

    // ---- Tracked transaction tests ----

    #[test]
    fn test_track_and_advance_transaction() {
        let mut state = WalletState::new();
        state.track_transaction(TrackedTransaction {
            tx_id: "tx1".into(),
            status: TxStatus::Created,
            spent_key_images: vec!["ki1".into()],
            spent_output_keys: vec!["tx_ki1:0".into()],
            change_outputs: vec![],
            fee: 50_000_000,
            created_at_secs: 1000,
        });

        assert_eq!(
            state.get_tracked_tx("tx1").unwrap().status,
            TxStatus::Created
        );

        state.advance_tx_status("tx1", TxStatus::Broadcast);
        assert_eq!(
            state.get_tracked_tx("tx1").unwrap().status,
            TxStatus::Broadcast
        );

        state.advance_tx_status("tx1", TxStatus::Confirmed { height: 500 });
        assert_eq!(
            state.get_tracked_tx("tx1").unwrap().status,
            TxStatus::Confirmed { height: 500 }
        );
    }

    #[test]
    fn test_find_tx_by_key_image() {
        let mut state = WalletState::new();
        state.track_transaction(TrackedTransaction {
            tx_id: "tx1".into(),
            status: TxStatus::Broadcast,
            spent_key_images: vec!["ki_a".into(), "ki_b".into()],
            spent_output_keys: vec![],
            change_outputs: vec![],
            fee: 0,
            created_at_secs: 1000,
        });

        assert_eq!(state.find_tx_by_key_image("ki_a").unwrap().tx_id, "tx1");
        assert_eq!(state.find_tx_by_key_image("ki_b").unwrap().tx_id, "tx1");
        assert!(state.find_tx_by_key_image("ki_c").is_none());
    }

    #[test]
    fn test_cleanup_old_tracked_txs() {
        let mut state = WalletState::new();
        state.track_transaction(TrackedTransaction {
            tx_id: "old_confirmed".into(),
            status: TxStatus::Confirmed { height: 100 },
            spent_key_images: vec![],
            spent_output_keys: vec![],
            change_outputs: vec![],
            fee: 0,
            created_at_secs: 1000,
        });
        state.track_transaction(TrackedTransaction {
            tx_id: "recent_confirmed".into(),
            status: TxStatus::Confirmed { height: 200 },
            spent_key_images: vec![],
            spent_output_keys: vec![],
            change_outputs: vec![],
            fee: 0,
            created_at_secs: 5000,
        });
        state.track_transaction(TrackedTransaction {
            tx_id: "broadcast".into(),
            status: TxStatus::Broadcast,
            spent_key_images: vec![],
            spent_output_keys: vec![],
            change_outputs: vec![],
            fee: 0,
            created_at_secs: 1000,
        });

        // max_age=3600, now=6000: old_confirmed (age 5000) removed, recent_confirmed (age 1000) kept
        let removed = state.cleanup_old_tracked_txs(6000, 3600);
        assert_eq!(removed, 1);
        assert!(state.get_tracked_tx("old_confirmed").is_none());
        assert!(state.get_tracked_tx("recent_confirmed").is_some());
        assert!(state.get_tracked_tx("broadcast").is_some()); // non-confirmed kept regardless
    }

    #[test]
    fn test_restore_pending_spends_filters_expired() {
        let mut state = WalletState::new();
        let mut spends = HashMap::new();
        spends.insert(
            "ki_fresh".to_string(),
            PendingSpend {
                tx_id: "tx1".into(),
                key_image: "ki_fresh".into(),
                output_key: "ok1".into(),
                amount: 100,
                created_at_secs: 5000,
            },
        );
        spends.insert(
            "ki_expired".to_string(),
            PendingSpend {
                tx_id: "tx2".into(),
                key_image: "ki_expired".into(),
                output_key: "ok2".into(),
                amount: 200,
                created_at_secs: 1000,
            },
        );

        // now=8000: ki_fresh age=3000 < 7200 (kept), ki_expired age=7000 < 7200 (kept)
        state.restore_pending_spends(spends.clone(), 8000);
        assert!(state.is_pending_spent("ki_fresh"));
        assert!(state.is_pending_spent("ki_expired"));

        // Reset and test with later time: ki_expired age=8200 >= 7200 (filtered)
        let mut state2 = WalletState::new();
        state2.restore_pending_spends(spends, 9200);
        assert!(state2.is_pending_spent("ki_fresh"));
        assert!(!state2.is_pending_spent("ki_expired"));
    }

    #[test]
    fn test_restore_tracked_transactions() {
        let mut state = WalletState::new();
        let mut txs = HashMap::new();
        txs.insert(
            "tx1".to_string(),
            TrackedTransaction {
                tx_id: "tx1".into(),
                status: TxStatus::Broadcast,
                spent_key_images: vec!["ki1".into()],
                spent_output_keys: vec![],
                change_outputs: vec![],
                fee: 50_000_000,
                created_at_secs: 1000,
            },
        );
        txs.insert(
            "tx2".to_string(),
            TrackedTransaction {
                tx_id: "tx2".into(),
                status: TxStatus::Confirmed { height: 500 },
                spent_key_images: vec![],
                spent_output_keys: vec![],
                change_outputs: vec![],
                fee: 0,
                created_at_secs: 2000,
            },
        );

        state.restore_tracked_transactions(txs);
        assert_eq!(
            state.get_tracked_tx("tx1").unwrap().status,
            TxStatus::Broadcast
        );
        assert_eq!(
            state.get_tracked_tx("tx2").unwrap().status,
            TxStatus::Confirmed { height: 500 }
        );
        assert!(state.get_tracked_tx("tx3").is_none());
    }

    #[test]
    fn test_add_pending_spends_advances_tracked_tx() {
        let mut state = WalletState::new();
        state.track_transaction(TrackedTransaction {
            tx_id: "tx1".into(),
            status: TxStatus::Created,
            spent_key_images: vec!["ki1".into()],
            spent_output_keys: vec![],
            change_outputs: vec![],
            fee: 0,
            created_at_secs: 1000,
        });

        state.add_pending_spends(
            "tx1",
            vec![make_pending_spend("tx1", "ki1", 1_000_000_000_000, 1000)],
        );

        assert_eq!(
            state.get_tracked_tx("tx1").unwrap().status,
            TxStatus::Broadcast
        );
    }

    #[test]
    fn test_restore_tracked_transactions_rebuilds_spent_by_tx() {
        let mut state = WalletState::new();
        state.add_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 60, "ki2"),
        ]);

        // Mark both as spent at height 100 with known tx hashes
        state.mark_spent_detecting_conflicts(
            &["ki1".to_string(), "ki2".to_string()],
            &["tx_a".to_string(), "tx_b".to_string()],
            100,
        );

        // Simulate save/restore: replace outputs (clears spent_by_tx), then restore tracked txs
        let mut txs = HashMap::new();
        txs.insert(
            "tx_a".to_string(),
            TrackedTransaction {
                tx_id: "tx_a".into(),
                status: TxStatus::Confirmed { height: 100 },
                spent_key_images: vec!["ki1".into()],
                spent_output_keys: vec![],
                change_outputs: vec![],
                fee: 0,
                created_at_secs: 1000,
            },
        );
        txs.insert(
            "tx_b".to_string(),
            TrackedTransaction {
                tx_id: "tx_b".into(),
                status: TxStatus::Broadcast,
                spent_key_images: vec!["ki2".into()],
                spent_output_keys: vec![],
                change_outputs: vec![],
                fee: 0,
                created_at_secs: 1000,
            },
        );

        // Clear and rebuild
        state.replace_outputs(vec![
            make_output(1_000_000_000_000, 50, "ki1"),
            make_output(2_000_000_000_000, 60, "ki2"),
        ]);
        // Mark spent again (simulating output restore with spent flag)
        state.mark_spent_detecting_conflicts(
            &["ki1".to_string(), "ki2".to_string()],
            &["tx_a".to_string(), "tx_b".to_string()],
            100,
        );

        // Now clear spent_by_tx and restore from tracked txs
        state.spent_by_tx.clear();
        state.restore_tracked_transactions(txs);

        // Rescan at different height should NOT produce conflict (spent_by_tx rebuilt)
        let (_, conflicts) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 200);
        assert!(
            conflicts.is_empty(),
            "expected no conflict after restore, got {:?}",
            conflicts
        );
    }

    #[test]
    fn test_rollback_clears_spent_by_tx() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);
        state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 100);

        // Rollback unspends ki1 and clears its spent_by_tx entry
        state.rollback_to_height(90);
        assert!(!state.outputs()[0].spent);

        // Re-spend with a DIFFERENT tx should not conflict (clean slate)
        let (count, conflicts) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_b".to_string()], 95);
        assert_eq!(count, 1);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_replace_outputs_clears_spent_by_tx() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);
        state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 100);

        // Replace outputs clears spent_by_tx
        state.replace_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);

        // First spend after replace works cleanly
        let (count, conflicts) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 100);
        assert_eq!(count, 1);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_conflict_after_conflict_updates_spent_by_tx() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(1_000_000_000_000, 50, "ki1")]);

        state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_a".to_string()], 100);

        // Different tx -> conflict, spent_by_tx updated to tx_b
        let (_, conflicts) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_b".to_string()], 200);
        assert_eq!(conflicts.len(), 1);

        // Same tx_b again at different height -> no conflict (idempotent)
        let (_, conflicts2) =
            state.mark_spent_detecting_conflicts(&["ki1".to_string()], &["tx_b".to_string()], 300);
        assert!(conflicts2.is_empty());
    }

    // Key-image import and index consistency

    /// Reimporting permuted key images must preserve both index entries.
    /// Incremental updates could remove an entry inserted by the previous iteration.
    #[test]
    fn import_key_images_keeps_every_output_findable_after_a_permuted_reimport() {
        let mut state = WalletState::new();
        state.replace_outputs(vec![
            make_output(1_000_000_000_000, 10, "A"),
            make_output(2_000_000_000_000, 20, "B"),
        ]);

        // Reverse the key-image order on the second import.
        let updated = state.import_key_images(0, &["B".to_string(), "A".to_string()]);
        assert_eq!(updated, 2);

        // Both images must still resolve to outputs when marking them spent.
        assert_eq!(state.mark_spent_by_key_images(&["B".to_string()]), 1);
        assert_eq!(state.mark_spent_by_key_images(&["A".to_string()]), 1);
        assert!(state.outputs().iter().all(|o| o.spent));
    }

    /// Reject duplicate key images so every image identifies one output in the index.
    #[test]
    fn import_key_images_refuses_a_duplicated_key_image() {
        let mut state = WalletState::new();
        state.replace_outputs(vec![
            output_without_key_image("first", 10),
            output_without_key_image("second", 20),
        ]);

        let updated = state.import_key_images(0, &["Z".to_string(), "Z".to_string()]);

        assert_eq!(
            updated, 0,
            "a duplicated key image must be refused outright"
        );
        assert!(
            state.outputs().iter().all(|o| o.key_image.is_empty()),
            "a refused import must not half-apply"
        );
    }

    /// Assigning key images in export order sorts the outputs. Rebuild the index
    /// as part of that operation so lookups follow the new positions.
    #[test]
    fn assign_key_images_in_export_order_reindexes_after_sorting() {
        let mut state = WalletState::new();
        // Put the later output first to exercise sorting.
        state.replace_outputs(vec![
            make_output(2_000_000_000_000, 20, "later"),
            make_output(1_000_000_000_000, 10, "earlier"),
        ]);

        let updated = state
            .assign_key_images_in_export_order(&["ki_earlier".to_string(), "ki_later".to_string()])
            .expect("a well-formed list should apply");
        assert_eq!(updated, 2);

        let outputs = state.outputs();
        assert_eq!(outputs[0].block_height, 10);
        assert_eq!(outputs[0].key_image, "ki_earlier");
        assert_eq!(outputs[1].key_image, "ki_later");

        // Check that key-image lookups find the correct outputs after sorting.
        assert!(state.freeze_output("ki_earlier"));
        let frozen: Vec<u64> = state
            .outputs()
            .iter()
            .filter(|o| o.frozen)
            .map(|o| o.block_height)
            .collect();
        assert_eq!(frozen, vec![10], "freeze must hit the output it named");
    }

    #[test]
    fn larger_copy_of_a_one_time_key_keeps_the_export_position() {
        let with_key = |amount, height, name: &str, key: &str| {
            let mut output = make_output(amount, height, name);
            output.key = key.repeat(32);
            output.key_image.clear();
            output
        };
        let mut state = WalletState::new();
        state.add_outputs(vec![with_key(1, 10, "small", "aa")]);
        state.add_outputs(vec![with_key(7, 11, "other", "cc")]);
        state.add_outputs(vec![with_key(5, 12, "large", "aa")]);
        state.add_outputs(vec![with_key(2, 13, "last", "dd")]);
        state.add_outputs(vec![with_key(9, 14, "larger", "aa")]);

        let images =
            |names: &[&str]| -> Vec<String> { names.iter().map(|n| n.to_string()).collect() };
        assert_eq!(
            state
                .assign_key_images_in_export_order(&images(&["ki_aa", "ki_cc", "ki_dd"]))
                .unwrap(),
            3
        );
        let held: Vec<(u64, u64, &str)> = state
            .outputs()
            .iter()
            .map(|o| (o.block_height, o.amount, o.key_image.as_str()))
            .collect();
        assert_eq!(
            held,
            vec![(14, 9, "ki_aa"), (11, 7, "ki_cc"), (13, 2, "ki_dd")]
        );

        state.rollback_to_height(12);
        assert_eq!(
            state
                .assign_key_images_in_export_order(&images(&["ki_aa", "ki_cc"]))
                .unwrap(),
            2
        );
        let held: Vec<(u64, u64, &str)> = state
            .outputs()
            .iter()
            .map(|o| (o.block_height, o.amount, o.key_image.as_str()))
            .collect();
        assert_eq!(held, vec![(10, 1, "ki_aa"), (11, 7, "ki_cc")]);
    }

    #[test]
    fn signed_import_pairs_a_replaced_record_at_its_wallet2_position() {
        use crate::key_image_import::{SignedKeyImage, SignedKeyImageImport};
        use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
        use sha3::{Digest, Keccak256};
        use zeroize::Zeroizing;

        let signed = |tag: &str| {
            let secret = Zeroizing::new(Scalar::from_bytes_mod_order(
                Keccak256::digest(tag.as_bytes()).into(),
            ));
            let public = &*secret * ED25519_BASEPOINT_TABLE;
            let image = crate::crypto::generate_key_image(&secret)
                .compress()
                .to_bytes();
            let (c, r) = crate::key_image_signing::generate_key_image_ring_signature(
                &image, &public, &secret,
            );
            let mut signature = [0u8; 64];
            signature[..32].copy_from_slice(&c);
            signature[32..].copy_from_slice(&r);
            (
                hex::encode(public.compress().to_bytes()),
                SignedKeyImage {
                    key_image: image,
                    signature,
                },
            )
        };
        let (key_a, image_a) = signed("a");
        let (key_c, image_c) = signed("c");
        let with_key = |amount, height, name: &str, key: &str| {
            let mut output = make_output(amount, height, name);
            output.key = key.to_string();
            output.key_image.clear();
            output
        };

        let mut state = WalletState::new();
        state.add_outputs(vec![with_key(1, 10, "small", &key_a)]);
        state.add_outputs(vec![with_key(1, 11, "other", &key_c)]);
        state.add_outputs(vec![with_key(5, 12, "large", &key_a)]);
        let wallet2_export = SignedKeyImageImport {
            offset: 0,
            key_images: vec![image_a, image_c],
        };
        assert_eq!(
            state.assign_verified_key_images_in_export_order(&wallet2_export),
            Ok(2)
        );
        assert_eq!(state.outputs()[0].amount, 5);
    }
    #[test]
    fn confirmations_replace_mempool_arrival_order_with_scan_order() {
        let mut state = WalletState::new();
        state.add_outputs(vec![make_output(3, 0, "third"), make_output(1, 0, "first")]);
        state.add_outputs(vec![
            make_output(1, 100, "first"),
            make_output(2, 100, "second"),
            make_output(3, 100, "third"),
        ]);
        state
            .assign_key_images_in_export_order(&["first".into(), "second".into(), "third".into()])
            .unwrap();
        assert_eq!(
            state.outputs().iter().map(|o| o.amount).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(state.freeze_output("third"));
        assert!(state.outputs()[2].frozen);
        // Rescans must neither duplicate nor move existing confirmed outputs.
        state.add_outputs(vec![make_output(1, 100, "first")]);
        assert_eq!(state.outputs().len(), 3);
        assert_eq!(state.outputs()[0].key_image, "first");
    }

    // Additional timelocks

    /// An additional timelock prevents spending even after the normal age
    /// requirement has been met.
    #[test]
    fn a_future_block_timelock_makes_an_aged_output_unspendable() {
        use monero_wallet::transaction::Timelock;

        let output =
            make_timelocked_output(1_000_000_000_000, 100, "ki_locked", Timelock::Block(5_000));

        // 900 confirmations satisfy the normal age requirement.
        assert!(
            !is_spendable(&output, 1_000),
            "an output under a future timelock must not be spendable"
        );
    }

    #[test]
    fn a_block_timelock_that_has_elapsed_is_spendable() {
        use monero_wallet::transaction::Timelock;

        let output =
            make_timelocked_output(1_000_000_000_000, 100, "ki_unlocked", Timelock::Block(500));

        // The output becomes spendable at the unlock height.
        assert!(!is_spendable(&output, 499));
        assert!(is_spendable(&output, 500));
        assert!(is_spendable(&output, 501));
    }

    /// Accept mature outputs without an additional timelock.
    #[test]
    fn an_output_without_an_additional_timelock_is_unaffected() {
        use monero_wallet::transaction::Timelock;

        let output = make_timelocked_output(1_000_000_000_000, 100, "ki_plain", Timelock::None);
        assert!(
            !is_spendable(&output, 105),
            "the 10-block age still applies"
        );
        assert!(is_spendable(&output, 110));
    }

    /// Keep a time-locked output unspendable until a clock is supplied and
    /// the unlock time is reached.
    #[test]
    fn a_time_timelock_needs_a_clock_to_be_satisfied() {
        use monero_wallet::transaction::Timelock;

        let unlock = 2_000_000_000u64;
        let output =
            make_timelocked_output(1_000_000_000_000, 100, "ki_timed", Timelock::Time(unlock));

        assert!(
            !is_spendable_at(&output, 1_000, None),
            "without a clock a time lock must be treated as unsatisfied"
        );
        assert!(!is_spendable_at(&output, 1_000, Some(unlock - 1_000)));
        // Account for Monero's 120-second allowance.
        assert!(is_spendable_at(&output, 1_000, Some(unlock - 120)));
        assert!(is_spendable_at(&output, 1_000, Some(unlock + 1)));
    }

    #[test]
    fn timestamp_locks_mature_through_balance_and_transaction_preparation() {
        use crate::tx_prepare::{prepare_send_inputs_at, prepare_sweep_inputs_at};
        use monero_wallet::transaction::Timelock;

        let unlock = 2_000_000_000;
        let amount = 5_000_000_000_000;
        let mut state = WalletState::new();
        state.replace_outputs(vec![make_timelocked_output(
            amount,
            100,
            "timed",
            Timelock::Time(unlock),
        )]);
        let selected = vec![state.outputs()[0].output_key()];
        for (clock, expected_unlocked) in [
            (None, false),
            (Some(unlock - 121), false),
            (Some(unlock - 120), true),
            (Some(unlock + 1), true),
        ] {
            let balance = state.balance_at(200, clock);
            assert_eq!(
                balance.confirmed,
                if expected_unlocked { amount } else { 0 }
            );
            assert_eq!(
                balance.unconfirmed,
                if expected_unlocked { 0 } else { amount }
            );
            assert_eq!(
                !state.spendable_outputs_at(200, clock).is_empty(),
                expected_unlocked
            );
            assert_eq!(
                !state
                    .spendable_outputs_for_accounts_at(&[0], 200, clock)
                    .is_empty(),
                expected_unlocked
            );
            for manual in [None, Some(selected.as_slice())] {
                assert_eq!(
                    prepare_send_inputs_at(
                        state.outputs(),
                        200,
                        clock,
                        1_000_000_000_000,
                        1,
                        manual,
                        None
                    )
                    .is_ok(),
                    expected_unlocked
                );
                assert_eq!(
                    prepare_sweep_inputs_at(state.outputs(), 200, clock, manual, None).is_ok(),
                    expected_unlocked
                );
            }
        }
        // A matured timestamp must not bypass confirmations or other exclusions.
        let clock = Some(unlock);
        assert!(prepare_sweep_inputs_at(state.outputs(), 109, clock, None, None).is_err());
        let excluded = HashSet::from(["timed".to_string()]);
        assert!(
            prepare_send_inputs_at(state.outputs(), 200, clock, 1, 1, None, Some(&excluded))
                .is_err()
        );
        assert!(
            prepare_sweep_inputs_at(state.outputs(), 200, clock, None, Some(&excluded)).is_err()
        );
        state.freeze_output("timed");
        assert!(state.spendable_outputs_at(200, clock).is_empty());
        assert!(prepare_sweep_inputs_at(state.outputs(), 200, clock, None, None).is_err());
    }

    #[test]
    fn default_wallet_apis_use_the_platform_clock_for_timestamp_locks() {
        use crate::tx_prepare::{prepare_send_inputs, prepare_sweep_inputs};
        use monero_wallet::transaction::Timelock;

        let now = current_time_secs().expect("the test host must provide a Unix clock");
        let amount = 5_000_000_000_000;
        let mut state = WalletState::new();
        state.replace_outputs(vec![
            make_timelocked_output(amount, 100, "mature", Timelock::Time(now - 3600)),
            make_timelocked_output(amount, 100, "future", Timelock::Time(now + 86400)),
        ]);
        state.current_height = 200;
        state.current_height = 199;
        assert!(is_spendable(&state.outputs()[0], 200));
        assert!(!is_spendable(&state.outputs()[1], 200));
        assert_eq!(state.balance().confirmed, amount);
        assert_eq!(state.balance_at_height(200).unconfirmed, amount);
        for outputs in [
            state.spendable_outputs(),
            state.spendable_outputs_at_height(200),
            state.spendable_outputs_for_accounts(&[0]),
        ] {
            assert_eq!(outputs.len(), 1);
            assert_eq!(outputs[0].key_image, "mature");
        }
        let send =
            prepare_send_inputs(state.outputs(), 200, 1_000_000_000_000, 1, None, None).unwrap();
        let sweep = prepare_sweep_inputs(state.outputs(), 200, None, None).unwrap();
        assert_eq!(
            send.spent_output_keys,
            vec![state.outputs()[0].output_key()]
        );
        assert_eq!(sweep.spent_output_keys, send.spent_output_keys);
        assert_eq!(sweep.total_input, amount);
    }

    /// Balance and coin selection must use the same spendability rules.
    #[test]
    fn a_timelocked_output_is_not_counted_as_confirmed_balance() {
        use monero_wallet::transaction::Timelock;

        let mut state = WalletState::new();
        state.replace_outputs(vec![
            make_timelocked_output(7, 100, "ki_locked", Timelock::Block(5_000)),
            make_timelocked_output(3, 100, "ki_plain", Timelock::None),
        ]);

        let bal = state.balance_at_height(1_000);
        assert_eq!(bal.confirmed, 3, "only the unlocked output is spendable");
        assert_eq!(bal.unconfirmed, 7);
        assert!(state
            .spendable_outputs_at_height(1_000)
            .iter()
            .all(|o| o.key_image == "ki_plain"));
    }
}
