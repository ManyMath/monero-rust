/// Integration tests for WalletState serialization and deserialization.
///
/// These tests verify that WalletState can be correctly serialized to both
/// JSON and binary formats, and then deserialized back without data loss.

use monero_rust::{types::SerializableOutput, wallet_state::WalletState, Language, Network};
use monero_seed::Seed;
use rand_core::OsRng;
use std::path::PathBuf;

#[test]
fn test_wallet_state_json_serialization_roundtrip() {
    // Create a wallet state
    let seed = Seed::new(&mut OsRng, Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test_wallet.bin"),
        100,
    )
    .expect("Failed to create WalletState");

    // Add some test data
    wallet_state.daemon_address = Some(String::from("http://node.example.com:18081"));
    wallet_state.is_connected = true;
    wallet_state.daemon_height = 1000;
    wallet_state.current_scanned_height = 500;

    // Serialize to JSON
    let json = serde_json::to_string(&wallet_state).expect("Failed to serialize to JSON");

    // Deserialize from JSON (ViewPair is automatically reconstructed)
    let deserialized: WalletState =
        serde_json::from_str(&json).expect("Failed to deserialize from JSON");

    // Verify key fields match
    assert_eq!(
        deserialized
            .seed
            .as_ref()
            .expect("Seed should be Some")
            .to_string(),
        wallet_state
            .seed
            .as_ref()
            .expect("Seed should be Some")
            .to_string()
    );
    assert_eq!(deserialized.network, wallet_state.network);
    assert_eq!(deserialized.seed_language, wallet_state.seed_language);
    assert_eq!(deserialized.password_hash, wallet_state.password_hash);
    assert_eq!(deserialized.wallet_path, wallet_state.wallet_path);
    assert_eq!(
        deserialized.refresh_from_height,
        wallet_state.refresh_from_height
    );
    assert_eq!(deserialized.daemon_address, wallet_state.daemon_address);
    assert_eq!(deserialized.daemon_height, wallet_state.daemon_height);
    assert_eq!(
        deserialized.current_scanned_height,
        wallet_state.current_scanned_height
    );

    // Verify ViewPair was reconstructed correctly
    assert_eq!(
        deserialized.view_pair.spend().compress().to_bytes(),
        wallet_state.view_pair.spend().compress().to_bytes()
    );
    assert_eq!(
        deserialized.view_pair.view().compress().to_bytes(),
        wallet_state.view_pair.view().compress().to_bytes()
    );
}

#[test]
fn test_wallet_state_bincode_serialization_roundtrip() {
    // Create a wallet state
    let seed = Seed::new(&mut OsRng, Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Testnet,
        "test_password",
        PathBuf::from("test_wallet2.bin"),
        200,
    )
    .expect("Failed to create WalletState");

    // Add test outputs
    let output = SerializableOutput {
        tx_hash: [1u8; 32],
        output_index: 0,
        amount: 5000000000000,
        key_image: [2u8; 32],
        subaddress_indices: (0, 1),
        height: 250,
        unlocked: true,
        spent: false,
        frozen: false,
    };
    wallet_state.outputs.insert([2u8; 32], output);
    wallet_state.frozen_outputs.insert([3u8; 32]);

    // Serialize to binary
    let binary = bincode::serialize(&wallet_state).expect("Failed to serialize to bincode");

    // Deserialize from binary (ViewPair is automatically reconstructed)
    let deserialized: WalletState =
        bincode::deserialize(&binary).expect("Failed to deserialize from bincode");

    // Verify data integrity
    assert_eq!(
        deserialized
            .seed
            .as_ref()
            .expect("Seed should be Some")
            .to_string(),
        wallet_state
            .seed
            .as_ref()
            .expect("Seed should be Some")
            .to_string()
    );
    assert_eq!(deserialized.network, wallet_state.network);
    assert_eq!(deserialized.outputs.len(), wallet_state.outputs.len());
    assert_eq!(
        deserialized.frozen_outputs.len(),
        wallet_state.frozen_outputs.len()
    );
}

#[test]
fn test_wallet_state_with_different_networks() {
    for network in [Network::Mainnet, Network::Testnet, Network::Stagenet] {
        let seed = Seed::new(&mut OsRng, Language::English);
        let wallet_state = WalletState::new(
            seed,
            String::from("English"),
            network,
            "test_password",
            PathBuf::from("test.bin"),
            0,
        )
        .expect("Failed to create WalletState");

        // Serialize and deserialize
        let json = serde_json::to_string(&wallet_state).expect("Failed to serialize");
        let deserialized: WalletState =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.network, network);
    }
}

#[test]
fn test_wallet_state_with_different_languages() {
    for language in [
        Language::English,
        Language::Spanish,
        Language::French,
        Language::German,
        Language::Italian,
        Language::Portuguese,
        Language::Japanese,
        Language::Russian,
    ] {
        let seed = Seed::new(&mut OsRng, language);
        let wallet_state = WalletState::new(
            seed,
            format!("{:?}", language),
            Network::Mainnet,
            "test_password",
            PathBuf::from("test.bin"),
            0,
        )
        .expect("Failed to create WalletState");

        // Serialize and deserialize
        let binary = bincode::serialize(&wallet_state).expect("Failed to serialize");
        let deserialized: WalletState =
            bincode::deserialize(&binary).expect("Failed to deserialize");

        // Verify seed can be recovered
        assert_eq!(
            deserialized
                .seed
                .as_ref()
                .expect("Seed should be Some")
                .to_string(),
            wallet_state
                .seed
                .as_ref()
                .expect("Seed should be Some")
                .to_string()
        );
    }
}

#[test]
fn test_view_only_wallet_serialization() {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};

    // Generate test keys
    let spend_scalar = Scalar::from_bytes_mod_order([42u8; 32]);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let spend_public_key = spend_point.compress().to_bytes();
    let view_private_key = [99u8; 32];

    let wallet_state = WalletState::new_view_only(
        spend_public_key,
        view_private_key,
        Network::Stagenet,
        "test_password",
        PathBuf::from("view_only.bin"),
        500,
    )
    .expect("Failed to create view-only wallet");

    assert!(wallet_state.is_view_only());

    // Serialize and deserialize
    let json = serde_json::to_string(&wallet_state).expect("Failed to serialize");
    let deserialized: WalletState =
        serde_json::from_str(&json).expect("Failed to deserialize");

    assert!(deserialized.is_view_only());
    assert_eq!(deserialized.spend_key, None);
}

#[test]
fn test_wallet_state_balance_calculations_persist() {
    let seed = Seed::new(&mut OsRng, Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .expect("Failed to create WalletState");

    // Add outputs
    for i in 0..5 {
        let output = SerializableOutput {
            tx_hash: [i; 32],
            output_index: i,
            amount: (i as u64 + 1) * 1000000000000,
            key_image: [i; 32],
            subaddress_indices: (0, i as u32),
            height: 100 + i as u64,
            unlocked: true,
            spent: i % 2 == 0, // Mark even indices as spent
            frozen: false,
        };
        wallet_state.outputs.insert([i; 32], output);

        if i % 2 == 0 {
            wallet_state.spent_outputs.insert([i; 32]);
        }
    }

    let balance_before = wallet_state.get_balance();

    // Serialize and deserialize
    let binary = bincode::serialize(&wallet_state).expect("Failed to serialize");
    let deserialized: WalletState =
        bincode::deserialize(&binary).expect("Failed to deserialize");

    let balance_after = deserialized.get_balance();

    assert_eq!(balance_before, balance_after);
    assert_eq!(deserialized.outputs.len(), 5);
    assert_eq!(deserialized.spent_outputs.len(), 3); // 0, 2, 4
}

#[test]
fn test_wallet_state_default() {
    let wallet_state = WalletState::default();

    assert!(!wallet_state.is_view_only());
    assert!(!wallet_state.is_closed);
    assert_eq!(wallet_state.network, Network::Mainnet);
    assert_eq!(wallet_state.get_balance(), 0);
    assert_eq!(wallet_state.outputs.len(), 0);
}

#[test]
fn test_wallet_state_is_synced() {
    let seed = Seed::new(&mut OsRng, Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .expect("Failed to create WalletState");

    // Not synced initially
    assert!(!wallet_state.is_synced());

    // Connect but not caught up
    wallet_state.is_connected = true;
    wallet_state.daemon_height = 1000;
    wallet_state.current_scanned_height = 500;
    assert!(!wallet_state.is_synced());

    // Fully synced
    wallet_state.current_scanned_height = 1000;
    assert!(wallet_state.is_synced());

    // Serialize and verify state persists
    let json = serde_json::to_string(&wallet_state).expect("Failed to serialize");
    let deserialized: WalletState =
        serde_json::from_str(&json).expect("Failed to deserialize");

    assert!(deserialized.is_synced());
}
