//! The wallet's ring database: one ring per key image, kept apart from the
//! output records as in wallet2's `ringdb.cpp`. Spending an output with two
//! different rings lets an observer intersect them and find it.
//! The application stores it: serialize `WalletState::ring_database` and
//! load it back with `WalletState::restore_ring_database`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::tx_builder::native::DecoySelection;

/// Rings by key image (hex).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingDatabase {
    rings: BTreeMap<String, DecoySelection>,
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

    /// The number of key images with a ring.
    pub fn len(&self) -> usize {
        self.rings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rings.is_empty()
    }

    pub(crate) fn holds(&self, key_image: &str) -> bool {
        self.ring(key_image).is_some()
    }

    pub(crate) fn set_ring(&mut self, key_image: &str, ring: DecoySelection) {
        self.rings.insert(canonical_key_image(key_image), ring);
    }
}
