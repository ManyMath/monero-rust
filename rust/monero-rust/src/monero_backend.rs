//! Compatibility facade for the current Monero protocol backend.
//!
//! This module re-exports the backend surface used by `monero-rust` from the
//! monero-oxide crates and the in-crate ports (RPC client, wallet compat
//! layer, seed handling), so callers keep one import boundary.

pub mod block {
    pub use monero_oxide::block::*;
}

pub mod ringct {
    pub use monero_oxide::ringct::*;

    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
    use zeroize::Zeroizing;

    /// Monero's biased hash-to-point, `H_p(key)`.
    pub fn hash_to_point(key: EdwardsPoint) -> EdwardsPoint {
        monero_wallet::ed25519::Point::biased_hash(key.compress().to_bytes()).into()
    }

    /// Compute the key image `x * H_p(xG)` for the given one-time key.
    pub fn generate_key_image(secret: &Zeroizing<Scalar>) -> EdwardsPoint {
        let public = &**secret * ED25519_BASEPOINT_TABLE;
        hash_to_point(public) * **secret
    }
}

pub mod rpc {
    pub use crate::monero_rpc::*;
}

pub mod transaction {
    pub use monero_oxide::transaction::*;
}

pub mod wallet {
    pub use crate::wallet_compat::*;

    pub mod address {
        pub use crate::wallet_compat::AddressSpec;
        pub use monero_wallet::address::*;
    }

    pub mod seed {
        pub use crate::seed_compat::*;
    }
}

pub use crate::wallet_compat::{Commitment, Protocol};

/// Hash the provided data to a scalar via keccak256.
pub fn hash_to_scalar(data: &[u8]) -> curve25519_dalek::Scalar {
    curve25519_dalek::Scalar::from_bytes_mod_order(monero_oxide::primitives::keccak256(data))
}
