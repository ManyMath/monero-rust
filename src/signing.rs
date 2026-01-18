//! CLSAG signatures for Monero transactions (v14+).

use curve25519_dalek::{
    constants::{ED25519_BASEPOINT_TABLE, ED25519_BASEPOINT_POINT},
    edwards::EdwardsPoint,
    scalar::Scalar,
    traits::VartimeMultiscalarMul,
};
use monero_generators::biased_hash_to_point;
use rand_core::{CryptoRng, RngCore};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use crate::WalletError;

/// CLSAG signature for a tx input
#[derive(Debug, Clone)]
pub struct ClsagSignature {
    pub start_index: usize,
    pub c1: Scalar,
    pub responses: Vec<Scalar>,
    pub key_image: EdwardsPoint,
}

/// Params for signing a tx input
#[derive(Clone)]
pub struct SigningParameters {
    pub spend_key: Zeroizing<Scalar>,
    pub real_output_index: usize,
    pub ring_members: Vec<EdwardsPoint>,
    pub commitment: EdwardsPoint,
    pub pseudo_commitment: Option<EdwardsPoint>,
    pub message: Vec<u8>,
}

/// Signed input result
#[derive(Debug, Clone)]
pub struct SignedInput {
    pub signature: ClsagSignature,
    pub key_image: EdwardsPoint,
    pub real_index: usize,
}

/// Generates key image I = x * Hp(P)
pub fn generate_key_image(
    secret_key: &Scalar,
    public_key: &EdwardsPoint,
) -> EdwardsPoint {
    let hash_point = biased_hash_to_point(public_key.compress().to_bytes());
    secret_key * hash_point
}

/// Signs a tx input with CLSAG
pub fn sign_clsag<R: RngCore + CryptoRng>(
    rng: &mut R,
    params: SigningParameters,
) -> Result<SignedInput, WalletError> {
    let ring_size = params.ring_members.len();

    if ring_size < 2 {
        return Err(WalletError::Other(
            "ring size must be at least 2".to_string()
        ));
    }

    if params.real_output_index >= ring_size {
        return Err(WalletError::Other(
            format!("real output index {} out of bounds for ring size {}",
                params.real_output_index, ring_size)
        ));
    }

    let real_idx = params.real_output_index;
    let real_pubkey = params.ring_members[real_idx];

    let key_image = generate_key_image(&params.spend_key, &real_pubkey);
    let hash_point = biased_hash_to_point(real_pubkey.compress().to_bytes());

    let mut alpha_bytes = [0u8; 64];
    rng.fill_bytes(&mut alpha_bytes);
    let alpha = Scalar::from_bytes_mod_order_wide(&alpha_bytes);

    let l_0 = &alpha * ED25519_BASEPOINT_TABLE;
    let r_0 = alpha * hash_point;

    let mut responses = vec![Scalar::ZERO; ring_size];
    for i in 0..ring_size {
        if i != real_idx {
            let mut r_bytes = [0u8; 64];
            rng.fill_bytes(&mut r_bytes);
            responses[i] = Scalar::from_bytes_mod_order_wide(&r_bytes);
        }
    }

    let mut hash_prefix = Vec::new();
    hash_prefix.extend_from_slice(&params.message);
    for pubkey in &params.ring_members {
        hash_prefix.extend_from_slice(&pubkey.compress().to_bytes());
    }
    hash_prefix.extend_from_slice(&key_image.compress().to_bytes());
    if let Some(pseudo_comm) = params.pseudo_commitment {
        hash_prefix.extend_from_slice(&pseudo_comm.compress().to_bytes());
    }
    hash_prefix.extend_from_slice(&params.commitment.compress().to_bytes());

    let start_idx = (real_idx + 1) % ring_size;

    let c_next = {
        let mut hasher = Keccak256::new();
        hasher.update(&hash_prefix);
        hasher.update(l_0.compress().to_bytes());
        hasher.update(r_0.compress().to_bytes());
        let hash: [u8; 32] = hasher.finalize().into();
        Scalar::from_bytes_mod_order(hash)
    };

    let mut c_values = vec![Scalar::ZERO; ring_size];
    let mut current_c = c_next;

    for offset in 0..ring_size {
        let idx = (start_idx + offset) % ring_size;

        if idx == real_idx {
            c_values[real_idx] = current_c;
            break;
        }

        c_values[idx] = current_c;

        let pubkey = params.ring_members[idx];
        let hp_idx = biased_hash_to_point(pubkey.compress().to_bytes());

        let l_i = EdwardsPoint::vartime_multiscalar_mul(
            [responses[idx], current_c],
            [ED25519_BASEPOINT_POINT, pubkey],
        );

        let r_i = EdwardsPoint::vartime_multiscalar_mul(
            [responses[idx], current_c],
            [hp_idx, key_image],
        );

        let mut hasher = Keccak256::new();
        hasher.update(&hash_prefix);
        hasher.update(l_i.compress().to_bytes());
        hasher.update(r_i.compress().to_bytes());

        let hash: [u8; 32] = hasher.finalize().into();
        current_c = Scalar::from_bytes_mod_order(hash);
    }

    responses[real_idx] = alpha - (c_values[real_idx] * *params.spend_key);

    Ok(SignedInput {
        signature: ClsagSignature {
            start_index: start_idx,
            c1: c_values[start_idx],
            responses,
            key_image,
        },
        key_image,
        real_index: real_idx,
    })
}

/// Verifies a CLSAG signature
pub fn verify_clsag(
    signature: &ClsagSignature,
    ring_members: &[EdwardsPoint],
    message: &[u8],
    commitment: &EdwardsPoint,
    pseudo_commitment: Option<&EdwardsPoint>,
) -> bool {
    let ring_size = ring_members.len();

    if signature.responses.len() != ring_size {
        return false;
    }

    if ring_size < 2 {
        return false;
    }

    if signature.start_index >= ring_size {
        return false;
    }

    let mut hash_prefix = Vec::new();
    hash_prefix.extend_from_slice(message);
    for pubkey in ring_members {
        hash_prefix.extend_from_slice(&pubkey.compress().to_bytes());
    }
    hash_prefix.extend_from_slice(&signature.key_image.compress().to_bytes());
    if let Some(pseudo_comm) = pseudo_commitment {
        hash_prefix.extend_from_slice(&pseudo_comm.compress().to_bytes());
    }
    hash_prefix.extend_from_slice(&commitment.compress().to_bytes());

    let mut current_c = signature.c1;

    for offset in 0..ring_size {
        let idx = (signature.start_index + offset) % ring_size;

        let pubkey = ring_members[idx];
        let hp_idx = biased_hash_to_point(pubkey.compress().to_bytes());

        let l_i = EdwardsPoint::vartime_multiscalar_mul(
            [signature.responses[idx], current_c],
            [ED25519_BASEPOINT_POINT, pubkey],
        );

        let r_i = EdwardsPoint::vartime_multiscalar_mul(
            [signature.responses[idx], current_c],
            [hp_idx, signature.key_image],
        );

        let mut hasher = Keccak256::new();
        hasher.update(&hash_prefix);
        hasher.update(l_i.compress().to_bytes());
        hasher.update(r_i.compress().to_bytes());

        let hash: [u8; 32] = hasher.finalize().into();
        current_c = Scalar::from_bytes_mod_order(hash);
    }

    current_c == signature.c1
}

/// Verify key image proof
pub fn verify_key_image(
    key_image: &EdwardsPoint,
    public_key: &EdwardsPoint,
    signature: &[u8; 64],
) -> bool {
    crate::crypto::verify_key_image_signature(signature, public_key, key_image)
}

/// Generate proof that key image is correctly formed
pub fn prove_key_image(
    secret_key: &Scalar,
    public_key: &EdwardsPoint,
    key_image: &EdwardsPoint,
) -> [u8; 64] {
    crate::crypto::generate_key_image_signature(secret_key, public_key, key_image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    #[test]
    fn test_key_image_generation() {
        let secret = Scalar::random(&mut OsRng);
        let public = &secret * ED25519_BASEPOINT_TABLE;

        let key_image_1 = generate_key_image(&secret, &public);
        let key_image_2 = generate_key_image(&secret, &public);

        assert_eq!(key_image_1, key_image_2);
    }

    #[test]
    fn test_key_image_proof() {
        let secret = Scalar::random(&mut OsRng);
        let public = &secret * ED25519_BASEPOINT_TABLE;
        let key_image = generate_key_image(&secret, &public);

        let proof = prove_key_image(&secret, &public, &key_image);
        assert!(verify_key_image(&key_image, &public, &proof));

        let wrong_secret = Scalar::random(&mut OsRng);
        let wrong_public = &wrong_secret * ED25519_BASEPOINT_TABLE;
        assert!(!verify_key_image(&key_image, &wrong_public, &proof));
    }

    #[test]
    fn test_clsag_signature_basic() {
        let mut rng = OsRng;
        let ring_size = 4;
        let real_idx = 2;

        let spend_key = Zeroizing::new(Scalar::random(&mut rng));
        let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

        let mut ring_members = Vec::with_capacity(ring_size);
        for i in 0..ring_size {
            if i == real_idx {
                ring_members.push(real_pubkey);
            } else {
                let decoy_key = Scalar::random(&mut rng);
                ring_members.push(&decoy_key * ED25519_BASEPOINT_TABLE);
            }
        }

        let commitment_scalar = Scalar::random(&mut rng);
        let commitment = &commitment_scalar * ED25519_BASEPOINT_TABLE;

        let message = b"test transaction";

        let params = SigningParameters {
            spend_key,
            real_output_index: real_idx,
            ring_members: ring_members.clone(),
            commitment,
            pseudo_commitment: None,
            message: message.to_vec(),
        };

        let signed = sign_clsag(&mut rng, params).expect("signing failed");

        let valid = verify_clsag(
            &signed.signature,
            &ring_members,
            message,
            &commitment,
            None,
        );

        assert!(valid);
    }

    #[test]
    fn test_clsag_signature_wrong_message() {
        let mut rng = OsRng;
        let ring_size = 4;
        let real_idx = 1;

        let spend_key = Zeroizing::new(Scalar::random(&mut rng));
        let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

        let mut ring_members = Vec::with_capacity(ring_size);
        for i in 0..ring_size {
            if i == real_idx {
                ring_members.push(real_pubkey);
            } else {
                let decoy_key = Scalar::random(&mut rng);
                ring_members.push(&decoy_key * ED25519_BASEPOINT_TABLE);
            }
        }

        let commitment_scalar = Scalar::random(&mut rng);
        let commitment = &commitment_scalar * ED25519_BASEPOINT_TABLE;
        let message = b"original message";

        let params = SigningParameters {
            spend_key,
            real_output_index: real_idx,
            ring_members: ring_members.clone(),
            commitment,
            pseudo_commitment: None,
            message: message.to_vec(),
        };

        let signed = sign_clsag(&mut rng, params).expect("signing failed");

        let wrong_message = b"tampered message";
        let valid = verify_clsag(
            &signed.signature,
            &ring_members,
            wrong_message,
            &commitment,
            None,
        );

        assert!(!valid);
    }

    #[test]
    fn test_clsag_invalid_ring_size() {
        let mut rng = OsRng;

        let spend_key = Zeroizing::new(Scalar::random(&mut rng));
        let real_pubkey = &*spend_key * ED25519_BASEPOINT_TABLE;

        let ring_members = vec![real_pubkey];
        let commitment_scalar = Scalar::random(&mut rng);
        let commitment = &commitment_scalar * ED25519_BASEPOINT_TABLE;

        let params = SigningParameters {
            spend_key,
            real_output_index: 0,
            ring_members,
            commitment,
            pseudo_commitment: None,
            message: b"test".to_vec(),
        };

        let result = sign_clsag(&mut rng, params);
        assert!(result.is_err());
    }

    #[test]
    fn test_clsag_invalid_real_index() {
        let mut rng = OsRng;

        let spend_key = Zeroizing::new(Scalar::random(&mut rng));

        let ring_size = 4;
        let mut ring_members = Vec::with_capacity(ring_size);
        for _ in 0..ring_size {
            let key = Scalar::random(&mut rng);
            ring_members.push(&key * ED25519_BASEPOINT_TABLE);
        }

        let commitment_scalar = Scalar::random(&mut rng);
        let commitment = &commitment_scalar * ED25519_BASEPOINT_TABLE;

        let params = SigningParameters {
            spend_key,
            real_output_index: ring_size + 1,
            ring_members,
            commitment,
            pseudo_commitment: None,
            message: b"test".to_vec(),
        };

        let result = sign_clsag(&mut rng, params);
        assert!(result.is_err());
    }
}
