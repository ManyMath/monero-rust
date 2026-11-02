//! The wallet's ring database: one ring per key image, kept apart from the
//! output records as in wallet2's `ringdb.cpp`. Spending an output with two
//! different rings lets an observer intersect them and find it.
//! The application stores it: serialize `WalletState::ring_database` and
//! load it back with `WalletState::restore_ring_database`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::tx_builder::native::DecoySelection;

/// Rings, and the candidate sets they were chosen from, by key image (hex).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingDatabase {
    rings: BTreeMap<String, DecoySelection>,
    /// The candidate set first requested for each key image. Every later
    /// request repeats it, so the daemon has no two sets to intersect.
    #[serde(default)]
    candidates: BTreeMap<String, Vec<u64>>,
    #[serde(default)]
    cleared: BTreeSet<String>,
}

/// A ring seen on chain for one of the wallet's key images that differs from
/// the recorded one, which is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainRingConflict {
    pub key_image: String,
    /// The global indexes of the recorded ring, or, for a key image with no
    /// ring, of the candidate set the daemon was asked for.
    pub recorded: Vec<u64>,
    /// The global indexes of the ring the chain showed.
    pub seen: Vec<u64>,
}

pub(crate) fn canonical_key_image(key_image: &str) -> String {
    key_image.to_ascii_lowercase()
}

impl RingDatabase {
    /// The ring recorded for `key_image`, either case.
    pub fn ring(&self, key_image: &str) -> Option<&DecoySelection> {
        self.rings.get(&canonical_key_image(key_image))
    }

    /// Every recorded ring with its key image.
    pub fn rings(&self) -> impl Iterator<Item = (&str, &DecoySelection)> {
        self.rings
            .iter()
            .map(|(key_image, ring)| (key_image.as_str(), ring))
    }

    /// The candidate set recorded for `key_image`.
    pub fn candidates(&self, key_image: &str) -> Option<&[u64]> {
        self.candidates
            .get(&canonical_key_image(key_image))
            .map(Vec::as_slice)
    }

    /// Every recorded candidate set with its key image.
    pub fn candidate_sets(&self) -> impl Iterator<Item = (&str, &[u64])> {
        self.candidates
            .iter()
            .map(|(key_image, candidates)| (key_image.as_str(), candidates.as_slice()))
    }

    /// The number of key images with a ring.
    pub fn len(&self) -> usize {
        self.rings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rings.is_empty()
    }

    pub(crate) fn holds(&self, key_image: &str) -> bool {
        self.ring(key_image).is_some()
            || self.candidates(key_image).is_some()
            || self.is_cleared(key_image)
    }

    pub(crate) fn is_within_candidates(&self, key_image: &str, ring: &DecoySelection) -> bool {
        self.candidates(key_image).is_none_or(|candidates| {
            ring.indexes()
                .is_ok_and(|indexes| indexes.iter().all(|index| candidates.contains(index)))
        })
    }

    /// Whether the ring of `key_image` was given up with `clear_ring` and no
    /// other has been recorded since.
    pub fn is_cleared(&self, key_image: &str) -> bool {
        self.cleared.contains(&canonical_key_image(key_image))
    }

    /// Every key image whose ring was cleared and not replaced.
    pub fn cleared_key_images(&self) -> impl Iterator<Item = &str> {
        self.cleared.iter().map(String::as_str)
    }

    pub(crate) fn mark_cleared(&mut self, key_image: &str) {
        self.cleared.insert(canonical_key_image(key_image));
    }

    pub(crate) fn set_ring(&mut self, key_image: &str, ring: DecoySelection) {
        debug_assert!(self.is_within_candidates(key_image, &ring));
        self.cleared.remove(&canonical_key_image(key_image));
        self.rings.insert(canonical_key_image(key_image), ring);
    }

    pub(crate) fn remove_ring(&mut self, key_image: &str) -> Option<DecoySelection> {
        self.candidates.remove(&canonical_key_image(key_image));
        self.rings.remove(&canonical_key_image(key_image))
    }

    pub(crate) fn set_candidates(&mut self, key_image: &str, candidates: Vec<u64>) {
        self.cleared.remove(&canonical_key_image(key_image));
        self.candidates
            .insert(canonical_key_image(key_image), candidates);
    }
}
