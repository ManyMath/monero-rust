//! Compatibility facade for the current Monero protocol backend.
//!
//! Maps the handful of symbols this crate needs onto monero-oxide/monero-wallet,
//! keeping the rest of the crate agnostic to the underlying backend.

use std_shims::sync::LazyLock;

use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};

pub(crate) use monero_oxide::transaction;
pub(crate) use monero_wallet as wallet;

/// The Pedersen commitment generator `H`, decompressed.
///
/// monero-oxide keeps its own `H` private, so we recover it from the public
/// `CompressedPoint::H` constant.
pub(crate) static H: LazyLock<EdwardsPoint> = LazyLock::new(|| {
  CompressedEdwardsY(monero_oxide::ed25519::CompressedPoint::H.to_bytes())
    .decompress()
    .expect("couldn't decompress `CompressedPoint::H`")
});
