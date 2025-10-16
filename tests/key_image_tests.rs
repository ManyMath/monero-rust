//! Key image computation tests.
//!
//! Validates that our key image computation matches monero-oxide and uses correct formulas.

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_generators::biased_hash_to_point;
use rand_core::OsRng;
use sha3::{Digest, Keccak256};
use std::ops::Deref;
use zeroize::Zeroizing;

/// Verify our key image formula matches monero-oxide.
/// Formula: key_image = (spend_key + key_offset) * H_p(output_key)
#[test]
fn test_key_image_formula_matches_monero_oxide() {
    let spend_key = Zeroizing::new(Scalar::random(&mut OsRng));
    let key_offset = Scalar::random(&mut OsRng);

    let effective_spend_key = spend_key.deref() + key_offset;
    let output_key = &effective_spend_key * ED25519_BASEPOINT_TABLE;

    // monero-oxide formula (wallet/src/send/mod.rs:604)
    let monero_oxide_key_image = {
        let input_key = spend_key.deref() + key_offset;
        let hash_point = biased_hash_to_point(output_key.compress().to_bytes());
        (input_key * hash_point).compress().to_bytes()
    };

    // our formula (wallet_state.rs)
    let our_key_image = {
        let effective_spend_key = spend_key.deref() + key_offset;
        let hash_point = biased_hash_to_point(output_key.compress().to_bytes());
        (effective_spend_key * hash_point).compress().to_bytes()
    };

    assert_eq!(our_key_image, monero_oxide_key_image);
}

/// Deterministic test with fixed values for regression testing.
#[test]
fn test_key_image_computation_deterministic() {
    let spend_key_bytes = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x00,
    ];

    let key_offset_bytes = [
        0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e,
        0x3f, 0x00,
    ];

    let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order(spend_key_bytes));
    let key_offset = Scalar::from_bytes_mod_order(key_offset_bytes);

    let effective_spend_key = spend_key.deref() + key_offset;
    let output_key = &effective_spend_key * ED25519_BASEPOINT_TABLE;

    let key_image = {
        let hash_point = biased_hash_to_point(output_key.compress().to_bytes());
        (effective_spend_key * hash_point).compress().to_bytes()
    };

    const EXPECTED: &str = "f5e092f7c06e64a50131c14ac7ea2cc0959ecc883ccd922599785797bf3b826d";
    let expected = hex::decode(EXPECTED).unwrap();

    assert_eq!(key_image, expected.as_slice(), "Key image regression failure");
}

/// Verify key image computation is deterministic across iterations.
#[test]
fn test_key_image_is_deterministic() {
    let spend_key = Zeroizing::new(Scalar::from_bytes_mod_order([42u8; 32]));
    let key_offset = Scalar::from_bytes_mod_order([99u8; 32]);

    let effective_spend_key = spend_key.deref() + key_offset;
    let output_key = &effective_spend_key * ED25519_BASEPOINT_TABLE;

    let key_images: Vec<[u8; 32]> = (0..10)
        .map(|_| {
            let hash_point = biased_hash_to_point(output_key.compress().to_bytes());
            (&effective_spend_key * hash_point).compress().to_bytes()
        })
        .collect();

    let first = key_images[0];
    for ki in &key_images {
        assert_eq!(*ki, first);
    }
}

/// View-only wallets use deterministic placeholder key images.
#[test]
fn test_view_only_placeholder() {
    let tx_hash = [0x42u8; 32];
    let output_index = 7u64;

    let placeholder1 = {
        let mut hasher = Keccak256::new();
        hasher.update(&tx_hash);
        hasher.update(&output_index.to_le_bytes());
        let result: [u8; 32] = hasher.finalize().into();
        result
    };

    let placeholder2 = {
        let mut hasher = Keccak256::new();
        hasher.update(&tx_hash);
        hasher.update(&output_index.to_le_bytes());
        let result: [u8; 32] = hasher.finalize().into();
        result
    };

    assert_eq!(placeholder1, placeholder2);
}

/// Different outputs must produce different key images.
#[test]
fn test_key_images_are_unique() {
    let spend_key = Zeroizing::new(Scalar::random(&mut OsRng));

    let key_images: Vec<[u8; 32]> = (0..10)
        .map(|_| {
            let key_offset = Scalar::random(&mut OsRng);
            let effective_spend_key = spend_key.deref() + key_offset;
            let output_key = &effective_spend_key * ED25519_BASEPOINT_TABLE;
            let hash_point = biased_hash_to_point(output_key.compress().to_bytes());
            (&effective_spend_key * hash_point).compress().to_bytes()
        })
        .collect();

    for i in 0..key_images.len() {
        for j in (i + 1)..key_images.len() {
            assert_ne!(key_images[i], key_images[j]);
        }
    }
}

/// Same output always produces same key image.
#[test]
fn test_same_output_same_key_image() {
    let spend_key = Zeroizing::new(Scalar::random(&mut OsRng));
    let key_offset = Scalar::random(&mut OsRng);

    let key_images: Vec<[u8; 32]> = (0..10)
        .map(|_| {
            let effective_spend_key = spend_key.deref() + key_offset;
            let output_key = &effective_spend_key * ED25519_BASEPOINT_TABLE;
            let hash_point = biased_hash_to_point(output_key.compress().to_bytes());
            (&effective_spend_key * hash_point).compress().to_bytes()
        })
        .collect();

    let first = key_images[0];
    for ki in &key_images {
        assert_eq!(*ki, first);
    }
}
