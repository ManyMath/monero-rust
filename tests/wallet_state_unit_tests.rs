use monero_rust::wallet_state::WalletState;
use monero_rust::types::{SerializableOutput, Transaction};
use monero_rust::Network;
use monero_seed::Seed;
use curve25519_dalek::scalar::Scalar;
use std::path::PathBuf;

#[test]
fn test_wallet_state_creation() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test_wallet.bin"),
        0,
    )
    .unwrap();

    assert!(!wallet_state.is_view_only());
    assert!(!wallet_state.is_closed);
    assert_eq!(wallet_state.network, Network::Mainnet);
    assert_eq!(wallet_state.get_balance(), 0);
}

#[test]
fn test_view_only_wallet() {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint};

    let spend_scalar = Scalar::from_bytes_mod_order([1u8; 32]);
    let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let spend_public_key = spend_point.compress().to_bytes();
    let view_private_key = [2u8; 32];

    let wallet_state = WalletState::new_view_only(
        spend_public_key,
        view_private_key,
        Network::Testnet,
        "test_password",
        PathBuf::from("view_only.bin"),
        100,
    )
    .expect("Failed to create view-only wallet");

    assert!(wallet_state.is_view_only());
    assert_eq!(wallet_state.network, Network::Testnet);
    assert_eq!(wallet_state.refresh_from_height, 100);
}

#[test]
fn test_balance_calculation() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Add some outputs
    let output1 = SerializableOutput {
        tx_hash: [1u8; 32],
        output_index: 0,
        amount: 1000000000000,
        key_image: [1u8; 32],
        subaddress_indices: (0, 0),
        height: 100,
        unlocked: true,
        spent: false,
        frozen: false,
    };

    let output2 = SerializableOutput {
        tx_hash: [2u8; 32],
        output_index: 0,
        amount: 2000000000000,
        key_image: [2u8; 32],
        subaddress_indices: (0, 1),
        height: 110,
        unlocked: true,
        spent: false,
        frozen: false,
    };

    wallet_state.outputs.insert([1u8; 32], output1);
    wallet_state.outputs.insert([2u8; 32], output2);

    assert_eq!(wallet_state.get_balance(), 3000000000000);

    // Mark one as spent
    wallet_state.spent_outputs.insert([1u8; 32]);
    assert_eq!(wallet_state.get_balance(), 2000000000000);
}

#[test]
fn test_password_hashing() {
    let password = "my_secure_password_123!";
    let salt = WalletState::generate_salt();

    // Hash the password
    let hash1 = WalletState::hash_password(password, &salt).expect("Failed to hash password");
    let hash2 = WalletState::hash_password(password, &salt).expect("Failed to hash password");

    // Same password with same salt should produce same hash
    assert_eq!(hash1, hash2);

    // Different salt should produce different hash
    let different_salt = WalletState::generate_salt();
    let hash3 = WalletState::hash_password(password, &different_salt)
        .expect("Failed to hash password");
    assert_ne!(hash1, hash3);

    // Different password should produce different hash
    let hash4 = WalletState::hash_password("different_password", &salt)
        .expect("Failed to hash password");
    assert_ne!(hash1, hash4);
}

#[test]
fn test_password_verification() {
    let password = "correct_password";
    let wrong_password = "wrong_password";
    let salt = WalletState::generate_salt();
    let hash = WalletState::hash_password(password, &salt).expect("Failed to hash password");

    // Correct password should verify
    assert!(WalletState::verify_password(password, &salt, &hash).is_ok());

    // Wrong password should fail
    assert!(WalletState::verify_password(wrong_password, &salt, &hash).is_err());

    // Wrong salt should fail
    let wrong_salt = WalletState::generate_salt();
    assert!(WalletState::verify_password(password, &wrong_salt, &hash).is_err());
}

#[test]
fn test_wallet_stores_password_correctly() {
    use rand_core::OsRng;

    let password = "wallet_password_456";
    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Verify the stored password hash
    assert!(
        WalletState::verify_password(password, &wallet_state.password_salt, &wallet_state.password_hash).is_ok()
    );

    // Wrong password should fail
    assert!(
        WalletState::verify_password("wrong", &wallet_state.password_salt, &wallet_state.password_hash).is_err()
    );
}

#[test]
fn test_is_synced() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Not synced if not connected
    assert!(!wallet_state.is_synced());

    wallet_state.is_connected = true;
    wallet_state.daemon_height = 100;
    wallet_state.current_scanned_height = 50;

    // Not synced if behind
    assert!(!wallet_state.is_synced());

    wallet_state.current_scanned_height = 100;

    // Synced when caught up
    assert!(wallet_state.is_synced());
}

// ========================================================================
// SCANNING TESTS - Output detection and blockchain scanning
// ========================================================================

#[test]
fn test_scanner_initialized_with_primary_address() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Scanner should be initialized (we can't directly test it but check side effects)
    // Primary address (0, 0) is handled automatically by Scanner, not stored in registered_subaddresses
    let registered = wallet_state.get_registered_subaddresses();
    assert_eq!(registered.len(), 0); // No subaddresses registered yet (primary is automatic)
}

#[test]
fn test_register_subaddress() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Initially no subaddresses (primary is handled automatically)
    assert_eq!(wallet_state.registered_subaddresses.len(), 0);

    // Register a new subaddress
    wallet_state.register_subaddress(0, 1).expect("Failed to register subaddress");

    // Should now have 1 registered subaddress
    assert_eq!(wallet_state.registered_subaddresses.len(), 1);

    let registered = wallet_state.get_registered_subaddresses();
    assert!(registered.contains(&(0, 1)));

    // Registering the same address again should be idempotent (no duplicates)
    wallet_state.register_subaddress(0, 1).expect("Failed to register subaddress again");
    assert_eq!(wallet_state.registered_subaddresses.len(), 1);
}

#[test]
fn test_register_subaddress_range() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Register range (0, 1) through (0, 5)
    let count = wallet_state.register_subaddress_range(0, 1, 5)
        .expect("Failed to register range");

    assert_eq!(count, 5);

    // Should now have 5 subaddresses: (0,1) through (0,5)
    // Primary (0, 0) is automatic, not in this list
    assert_eq!(wallet_state.registered_subaddresses.len(), 5);

    let registered = wallet_state.get_registered_subaddresses();
    assert!(registered.contains(&(0, 1)));
    assert!(registered.contains(&(0, 2)));
    assert!(registered.contains(&(0, 3)));
    assert!(registered.contains(&(0, 4)));
    assert!(registered.contains(&(0, 5)));
}

#[test]
fn test_handle_reorganization_removes_outputs() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Add outputs at different heights
    let output_at_100 = SerializableOutput {
        tx_hash: [1u8; 32],
        output_index: 0,
        amount: 1000000000000,
        key_image: [1u8; 32],
        subaddress_indices: (0, 0),
        height: 100,
        unlocked: false,
        spent: false,
        frozen: false,
    };

    let output_at_105 = SerializableOutput {
        tx_hash: [2u8; 32],
        output_index: 0,
        amount: 2000000000000,
        key_image: [2u8; 32],
        subaddress_indices: (0, 0),
        height: 105,
        unlocked: false,
        spent: false,
        frozen: false,
    };

    let output_at_110 = SerializableOutput {
        tx_hash: [3u8; 32],
        output_index: 0,
        amount: 3000000000000,
        key_image: [3u8; 32],
        subaddress_indices: (0, 0),
        height: 110,
        unlocked: false,
        spent: false,
        frozen: false,
    };

    wallet_state.outputs.insert([1u8; 32], output_at_100);
    wallet_state.outputs.insert([2u8; 32], output_at_105);
    wallet_state.outputs.insert([3u8; 32], output_at_110);
    wallet_state.current_scanned_height = 110;

    assert_eq!(wallet_state.outputs.len(), 3);

    // Reorganization at height 105 should remove outputs at 105 and above
    let removed_count = wallet_state.handle_reorganization(105);

    assert_eq!(removed_count, 2); // Outputs at 105 and 110 removed
    assert_eq!(wallet_state.outputs.len(), 1); // Only output at 100 remains
    assert!(wallet_state.outputs.contains_key(&[1u8; 32]));
    assert!(!wallet_state.outputs.contains_key(&[2u8; 32]));
    assert!(!wallet_state.outputs.contains_key(&[3u8; 32]));

    // Scanned height should be rewound to fork_height - 1
    assert_eq!(wallet_state.current_scanned_height, 104);
}

#[test]
fn test_handle_reorganization_cleans_hashsets() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Add outputs and mark some as spent/frozen
    let output1 = SerializableOutput {
        tx_hash: [1u8; 32],
        output_index: 0,
        amount: 1000000000000,
        key_image: [1u8; 32],
        subaddress_indices: (0, 0),
        height: 100,
        unlocked: false,
        spent: false,
        frozen: false,
    };

    let output2 = SerializableOutput {
        tx_hash: [2u8; 32],
        output_index: 0,
        amount: 2000000000000,
        key_image: [2u8; 32],
        subaddress_indices: (0, 0),
        height: 110,
        unlocked: false,
        spent: false,
        frozen: false,
    };

    wallet_state.outputs.insert([1u8; 32], output1);
    wallet_state.outputs.insert([2u8; 32], output2);

    // Mark output2 as spent and frozen
    wallet_state.spent_outputs.insert([2u8; 32]);
    wallet_state.frozen_outputs.insert([2u8; 32]);

    assert_eq!(wallet_state.spent_outputs.len(), 1);
    assert_eq!(wallet_state.frozen_outputs.len(), 1);

    // Reorganization at height 105 removes output2
    wallet_state.handle_reorganization(105);

    // Key image [2] should be removed from HashSets
    assert_eq!(wallet_state.spent_outputs.len(), 0);
    assert_eq!(wallet_state.frozen_outputs.len(), 0);
    assert!(!wallet_state.spent_outputs.contains(&[2u8; 32]));
    assert!(!wallet_state.frozen_outputs.contains(&[2u8; 32]));
}

#[test]
fn test_handle_reorganization_cleans_transactions() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // Add transactions at different heights
    let tx1 = Transaction::new_incoming([1u8; 32], Some(100), 1234567890, 1000000000000);
    let tx2 = Transaction::new_incoming([2u8; 32], Some(105), 1234567895, 2000000000000);
    let tx3 = Transaction::new_incoming([3u8; 32], Some(110), 1234567900, 3000000000000);

    wallet_state.transactions.insert([1u8; 32], tx1);
    wallet_state.transactions.insert([2u8; 32], tx2);
    wallet_state.transactions.insert([3u8; 32], tx3);

    assert_eq!(wallet_state.transactions.len(), 3);

    // Reorganization at height 105
    wallet_state.handle_reorganization(105);

    // Only tx1 (height 100) should remain
    assert_eq!(wallet_state.transactions.len(), 1);
    assert!(wallet_state.transactions.contains_key(&[1u8; 32]));
    assert!(!wallet_state.transactions.contains_key(&[2u8; 32]));
    assert!(!wallet_state.transactions.contains_key(&[3u8; 32]));
}

#[test]
fn test_subaddresses_persist_across_save_load() {
    use rand_core::OsRng;
    use tempfile::NamedTempFile;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let wallet_path = temp_file.path().to_path_buf();

    // Create wallet and register subaddresses
    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);

    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        wallet_path.clone(),
        0,
    )
    .unwrap();

    // Register some subaddresses
    wallet_state.register_subaddress(0, 1).unwrap();
    wallet_state.register_subaddress(0, 2).unwrap();
    wallet_state.register_subaddress(1, 0).unwrap();

    assert_eq!(wallet_state.registered_subaddresses.len(), 3);

    // Save wallet
    wallet_state.save("test_password").expect("Failed to save wallet");

    // Load wallet
    let loaded_wallet = WalletState::load_from_file(
        wallet_path.to_str().unwrap(),
        "test_password"
    ).expect("Failed to load wallet");

    // Verify subaddresses were restored
    assert_eq!(loaded_wallet.registered_subaddresses.len(), 3);

    let registered = loaded_wallet.get_registered_subaddresses();
    assert!(registered.contains(&(0, 1)));
    assert!(registered.contains(&(0, 2)));
    assert!(registered.contains(&(1, 0)));
}

#[test]
fn test_invalid_subaddress_index() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    // SubaddressIndex::new() returns None for invalid indices
    // In practice, this would be very large numbers that overflow internal limits
    // For now, all reasonable values should work, so we just verify the API
    // returns Result and handles the Option correctly

    // This should succeed for reasonable values
    let result = wallet_state.register_subaddress(0, 100);
    assert!(result.is_ok());

    let result = wallet_state.register_subaddress(10, 10);
    assert!(result.is_ok());
}

#[test]
fn test_get_registered_subaddresses() {
    use rand_core::OsRng;

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    wallet_state.register_subaddress(0, 1).unwrap();
    wallet_state.register_subaddress(0, 5).unwrap();
    wallet_state.register_subaddress(1, 0).unwrap();

    let registered = wallet_state.get_registered_subaddresses();

    // Should have 3 registered subaddresses (primary is automatic)
    assert_eq!(registered.len(), 3);

    // Verify all are present
    assert!(registered.contains(&(0, 1)));
    assert!(registered.contains(&(0, 5)));
    assert!(registered.contains(&(1, 0)));
}
