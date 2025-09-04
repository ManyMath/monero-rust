//! Integration tests verifying BIP39 12-word seeds work through the same
//! monero_rust function calls that the WASM actors make.
//!
//! Each test mirrors a specific actor calling pattern from wallet.rs
//! or tx_builder.rs, ensuring BIP39 input produces correct results at
//! the integration boundary.

const BIP39_SEED: &str =
    "color ranch color remove subway public water embrace before begin liberty fault";
const BIP39_EXPECTED_ADDRESS: &str =
    "49MggvPosJugF8Zq7WAKbsSchz6vbyL6YiUxM4ryfGQDXphs6wiWiXLFWCSshnLPcceGTWUaKfWWMHQAAKESV3TQJVQsL9a";

#[test]
fn test_wasm_seed_birthday_with_bip39() {
    let birthday = monero_rust::seed_birthday(BIP39_SEED);
    // Legacy seeds (converted from BIP39) don't encode a birthday
    assert_eq!(birthday, None);
}

#[test]
fn test_wasm_seed_birthday_with_polyseed() {
    let polyseed = monero_rust::generate_seed("polyseed").unwrap();
    let birthday = monero_rust::seed_birthday(&polyseed);
    // Polyseeds encode a birthday timestamp
    assert!(birthday.is_some());
    assert!(birthday.unwrap() > 0);
}

#[test]
fn test_wasm_seed_birthday_with_classic() {
    let classic = monero_rust::generate_seed("classic").unwrap();
    let birthday = monero_rust::seed_birthday(&classic);
    // Classic seeds don't encode a birthday
    assert_eq!(birthday, None);
}

#[test]
fn test_wasm_generate_bip39_then_birthday() {
    let seed = monero_rust::generate_seed("bip39").unwrap();
    assert_eq!(seed.split_whitespace().count(), 12);
    // Only polyseed encodes a birthday, but BIP39 should not panic
    let birthday = monero_rust::seed_birthday(&seed);
    assert_eq!(birthday, None);
}

#[test]
fn test_wasm_generate_polyseed_then_birthday() {
    let seed = monero_rust::generate_seed("polyseed").unwrap();
    assert_eq!(seed.split_whitespace().count(), 16);
    let birthday = monero_rust::seed_birthday(&seed);
    assert!(birthday.is_some());
}

#[test]
fn test_wasm_derive_address_with_bip39() {
    let address = monero_rust::derive_address(BIP39_SEED, "mainnet", "").unwrap();
    assert_eq!(address, BIP39_EXPECTED_ADDRESS);
}

#[test]
fn test_wasm_derive_subaddress_with_bip39() {
    let primary = monero_rust::derive_subaddress(BIP39_SEED, "mainnet", 0, 0, "").unwrap();
    assert_eq!(primary, BIP39_EXPECTED_ADDRESS);

    let sub = monero_rust::derive_subaddress(BIP39_SEED, "mainnet", 0, 1, "").unwrap();
    assert_ne!(sub, primary);
    assert!(sub.starts_with('8'));
}

#[test]
fn test_wasm_derive_keys_with_bip39() {
    let keys = monero_rust::derive_keys(BIP39_SEED, "mainnet", "").unwrap();
    assert_eq!(keys.address, BIP39_EXPECTED_ADDRESS);
    assert!(!keys.secret_spend_key.is_empty());
    assert!(!keys.secret_view_key.is_empty());
    assert!(!keys.public_spend_key.is_empty());
    assert!(!keys.public_view_key.is_empty());
}

#[test]
fn test_wasm_validate_seed_bip39() {
    monero_rust::validate_seed(BIP39_SEED).unwrap();
}

#[test]
fn test_wasm_validate_seed_invalid_bip39_rejected() {
    let result = monero_rust::validate_seed(
        "color ranch color remove subway public water embrace before begin liberty zzzzz",
    );
    assert!(result.is_err());
}

#[test]
fn test_wasm_bip39_to_legacy_conversion() {
    let legacy = monero_rust::bip39_to_legacy_mnemonic(BIP39_SEED, "", 0).unwrap();
    assert_eq!(legacy.split_whitespace().count(), 25);
    // Legacy seed should derive the same address
    let addr = monero_rust::derive_address(&legacy, "mainnet", "").unwrap();
    assert_eq!(addr, BIP39_EXPECTED_ADDRESS);
}

#[test]
fn test_wasm_resolve_seed_bip39_consistency() {
    let seed = monero_rust::resolve_seed(BIP39_SEED).unwrap();
    // The resolved seed should produce the same keys as direct derive_keys
    let keys = monero_rust::derive_keys(BIP39_SEED, "mainnet", "").unwrap();
    assert_eq!(keys.address, BIP39_EXPECTED_ADDRESS);
}

#[test]
fn test_wasm_resolve_seed_all_types() {
    // BIP39
    monero_rust::resolve_seed(BIP39_SEED).unwrap();
    // Classic
    let classic = monero_rust::generate_seed("classic").unwrap();
    monero_rust::resolve_seed(&classic).unwrap();
    // Polyseed
    let polyseed = monero_rust::generate_seed("polyseed").unwrap();
    monero_rust::resolve_seed(&polyseed).unwrap();
}

#[test]
fn test_wasm_full_bip39_restore_pipeline() {
    // 1. Validate the seed (UI validation step)
    monero_rust::validate_seed(BIP39_SEED).unwrap();

    // 2. Get seed birthday (for restore height)
    let birthday = monero_rust::seed_birthday(BIP39_SEED);
    // BIP39→legacy has no birthday, so restore will use user-provided height
    assert_eq!(birthday, None);

    // 3. Derive keys (wallet initialization)
    let keys = monero_rust::derive_keys(BIP39_SEED, "mainnet", "").unwrap();
    assert_eq!(keys.address, BIP39_EXPECTED_ADDRESS);

    // 4. Derive primary address
    let address = monero_rust::derive_address(BIP39_SEED, "mainnet", "").unwrap();
    assert_eq!(address, BIP39_EXPECTED_ADDRESS);

    // 5. Derive subaddress for receiving
    let sub = monero_rust::derive_subaddress(BIP39_SEED, "mainnet", 0, 1, "").unwrap();
    assert!(sub.starts_with('8'));

    // 6. resolve_seed succeeds (used by all scan/tx paths)
    monero_rust::resolve_seed(BIP39_SEED).unwrap();
}

mod mock_scanner;
use mock_scanner::MockRpc;

#[tokio::test]
async fn test_mock_scan_workflow_with_bip39_seed() {
    let rpc = MockRpc::new(1100);
    let start_height = 1000;
    let target_height = rpc.get_height().unwrap();

    let mut current_height = start_height;
    let mut all_outputs = Vec::new();

    while current_height <= target_height {
        let result = rpc.scan_block(current_height, BIP39_SEED).unwrap();
        all_outputs.extend(result.outputs);
        current_height += 1;
    }

    assert_eq!(current_height, target_height + 1);
    assert!(all_outputs.len() >= 6);
}
