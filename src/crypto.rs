//! Monero hashing and key-image helpers.

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use zeroize::Zeroizing;

/// Hash bytes to a scalar using Monero's Keccak-256 hash.
pub fn hash_to_scalar(data: &[u8]) -> Scalar {
    Scalar::from_bytes_mod_order(monero_oxide::primitives::keccak256(data))
}

/// Apply Monero's biased hash-to-point function, `H_p(point)`.
pub fn hash_to_point(point: EdwardsPoint) -> EdwardsPoint {
    monero_wallet::ed25519::Point::biased_hash(point.compress().to_bytes()).into()
}

/// Derive the key image `x * H_p(xG)` from a one-time private key.
pub fn generate_key_image(secret: &Zeroizing<Scalar>) -> EdwardsPoint {
    let public = &**secret * ED25519_BASEPOINT_TABLE;
    hash_to_point(public) * **secret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_image_is_deterministic() {
        let secret = Zeroizing::new(Scalar::from(7u64));
        assert_eq!(generate_key_image(&secret), generate_key_image(&secret));
    }

    #[test]
    fn hash_to_scalar_is_deterministic() {
        assert_eq!(hash_to_scalar(b"monero"), hash_to_scalar(b"monero"));
        assert_ne!(hash_to_scalar(b"monero"), hash_to_scalar(b"Monero"));
    }
}
