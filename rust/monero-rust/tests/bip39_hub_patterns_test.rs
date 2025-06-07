//! Integration tests verifying BIP39 12-word seeds work through the same
//! monero_rust function calls that the hub actors (wallet.rs, tx_builder.rs)
//! make. Each test mirrors a specific hub actor calling pattern to close the
//! integration gap between unit-level BIP39 tests and the hub layer.
//!
//! The hub crate (WASM-only) cannot compile tests on native targets, so these
//! tests exercise the exact API surface the hub depends on.

use monero_rust::{
    bip39_to_legacy_mnemonic, derive_address, derive_keys, derive_subaddress,
    generate_seed, resolve_seed, seed_birthday, validate_seed,
};

const BIP39_SEED: &str =
    "color ranch color remove subway public water embrace before begin liberty fault";
const BIP39_EXPECTED_ADDRESS: &str =
    "49MggvPosJugF8Zq7WAKbsSchz6vbyL6YiUxM4ryfGQDXphs6wiWiXLFWCSshnLPcceGTWUaKfWWMHQAAKESV3TQJVQsL9a";

#[test]
fn hub_seed_birthday_with_bip39() {
    let birthday = seed_birthday(BIP39_SEED);
    assert_eq!(birthday, None);
}

#[test]
fn hub_seed_birthday_with_polyseed() {
    let polyseed = generate_seed("polyseed").unwrap();
    let birthday = seed_birthday(&polyseed);
    assert!(birthday.is_some());
    assert!(birthday.unwrap() > 0);
}

#[test]
fn hub_seed_birthday_with_classic() {
    let classic = generate_seed("classic").unwrap();
    let birthday = seed_birthday(&classic);
    assert_eq!(birthday, None);
}

#[test]
fn hub_generate_bip39_then_birthday() {
    let seed = generate_seed("bip39").unwrap();
    assert_eq!(seed.split_whitespace().count(), 12);
    let birthday = seed_birthday(&seed);
    assert_eq!(birthday, None);
}

#[test]
fn hub_generate_polyseed_then_birthday() {
    let seed = generate_seed("polyseed").unwrap();
    assert_eq!(seed.split_whitespace().count(), 16);
    let birthday = seed_birthday(&seed);
    assert!(birthday.is_some());
}

#[test]
fn hub_derive_address_with_bip39() {
    let address = derive_address(BIP39_SEED, "mainnet").unwrap();
    assert_eq!(address, BIP39_EXPECTED_ADDRESS);
}

#[test]
fn hub_derive_subaddress_with_bip39() {
    let primary = derive_subaddress(BIP39_SEED, "mainnet", 0, 0).unwrap();
    assert_eq!(primary, BIP39_EXPECTED_ADDRESS);

    let sub = derive_subaddress(BIP39_SEED, "mainnet", 0, 1).unwrap();
    assert_ne!(sub, primary);
    assert!(sub.starts_with('8'));
}

#[test]
fn hub_derive_keys_with_bip39() {
    let keys = derive_keys(BIP39_SEED, "mainnet").unwrap();
    assert_eq!(keys.address, BIP39_EXPECTED_ADDRESS);
    assert!(!keys.secret_spend_key.is_empty());
    assert!(!keys.secret_view_key.is_empty());
    assert!(!keys.public_spend_key.is_empty());
    assert!(!keys.public_view_key.is_empty());
}

#[test]
fn hub_validate_seed_bip39() {
    validate_seed(BIP39_SEED).unwrap();
}

#[test]
fn hub_validate_seed_invalid_bip39_rejected() {
    let result = validate_seed(
        "color ranch color remove subway public water embrace before begin liberty zzzzz",
    );
    assert!(result.is_err());
}

#[test]
fn hub_bip39_to_legacy_conversion() {
    let legacy = bip39_to_legacy_mnemonic(BIP39_SEED, "", 0).unwrap();
    assert_eq!(legacy.split_whitespace().count(), 25);
    let addr = derive_address(&legacy, "mainnet").unwrap();
    assert_eq!(addr, BIP39_EXPECTED_ADDRESS);
}

#[test]
fn hub_resolve_seed_all_types() {
    // BIP39
    resolve_seed(BIP39_SEED).unwrap();
    // Classic
    let classic = generate_seed("classic").unwrap();
    resolve_seed(&classic).unwrap();
    // Polyseed
    let polyseed = generate_seed("polyseed").unwrap();
    resolve_seed(&polyseed).unwrap();
}

#[test]
fn hub_full_bip39_restore_pipeline() {
    // 1. Validate (UI validation before restore)
    validate_seed(BIP39_SEED).unwrap();

    // 2. Seed birthday (for restore height determination)
    let birthday = seed_birthday(BIP39_SEED);
    assert_eq!(birthday, None);

    // 3. Derive keys (wallet initialization)
    let keys = derive_keys(BIP39_SEED, "mainnet").unwrap();
    assert_eq!(keys.address, BIP39_EXPECTED_ADDRESS);

    // 4. Derive primary address
    let address = derive_address(BIP39_SEED, "mainnet").unwrap();
    assert_eq!(address, BIP39_EXPECTED_ADDRESS);

    // 5. Derive subaddress for receiving
    let sub = derive_subaddress(BIP39_SEED, "mainnet", 0, 1).unwrap();
    assert!(sub.starts_with('8'));

    // 6. resolve_seed succeeds (used by all scan/tx paths internally)
    resolve_seed(BIP39_SEED).unwrap();
}

#[test]
fn hub_all_seed_types_through_restore_pipeline() {
    let seeds = vec![
        ("bip39", generate_seed("bip39").unwrap()),
        ("classic", generate_seed("classic").unwrap()),
        ("polyseed", generate_seed("polyseed").unwrap()),
    ];

    for (seed_type, seed) in &seeds {
        validate_seed(seed).unwrap_or_else(|e| panic!("{} validate failed: {}", seed_type, e));
        let _ = seed_birthday(seed); // should not panic for any type
        derive_keys(seed, "mainnet").unwrap_or_else(|e| panic!("{} derive_keys failed: {}", seed_type, e));
        derive_address(seed, "mainnet").unwrap_or_else(|e| panic!("{} derive_address failed: {}", seed_type, e));
        derive_subaddress(seed, "mainnet", 0, 0).unwrap_or_else(|e| panic!("{} derive_subaddress failed: {}", seed_type, e));
        resolve_seed(seed).unwrap_or_else(|e| panic!("{} resolve_seed failed: {}", seed_type, e));
    }
}
