/// Integration tests for WalletState serialization and deserialization.
///
/// These tests verify that WalletState can be correctly serialized to both
/// JSON and binary formats, and then deserialized back without data loss.

use monero_rust::{types::SerializableOutput, wallet_state::WalletState, Language, Network, WalletError};
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
            output_index: i as u64,
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

// ========================================================================
// ENCRYPTED FILE I/O TESTS
// ========================================================================

#[test]
fn test_wallet_save_and_load_roundtrip() {
    use std::fs;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_wallet.bin");

    let password = "my_secure_password_123!";

    // Create a wallet
    let seed = Seed::new(&mut OsRng, Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        wallet_path.clone(),
        100,
    )
    .expect("Failed to create WalletState");

    // Add some test data
    wallet_state.daemon_address = Some(String::from("http://node.example.com:18081"));
    wallet_state.is_connected = true;
    wallet_state.daemon_height = 1000;
    wallet_state.current_scanned_height = 500;

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

    let balance_before = wallet_state.get_balance();
    let seed_before = wallet_state.seed.as_ref().expect("Seed should be Some").to_string();

    // Save the wallet
    wallet_state.save(password).expect("Failed to save wallet");

    // Verify file was created
    assert!(wallet_path.exists());

    // Load the wallet
    let loaded_wallet = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load wallet");

    // Verify all data matches
    assert_eq!(
        loaded_wallet.seed.as_ref().expect("Seed should be Some").to_string(),
        seed_before
    );
    assert_eq!(loaded_wallet.network, wallet_state.network);
    assert_eq!(loaded_wallet.seed_language, wallet_state.seed_language);
    assert_eq!(loaded_wallet.daemon_address, wallet_state.daemon_address);
    assert_eq!(loaded_wallet.daemon_height, wallet_state.daemon_height);
    assert_eq!(loaded_wallet.current_scanned_height, wallet_state.current_scanned_height);
    assert_eq!(loaded_wallet.get_balance(), balance_before);
    assert_eq!(loaded_wallet.outputs.len(), wallet_state.outputs.len());
    assert_eq!(loaded_wallet.frozen_outputs.len(), wallet_state.frozen_outputs.len());

    // Verify ViewPair was reconstructed correctly
    assert_eq!(
        loaded_wallet.view_pair.spend().compress().to_bytes(),
        wallet_state.view_pair.spend().compress().to_bytes()
    );
    assert_eq!(
        loaded_wallet.view_pair.view().compress().to_bytes(),
        wallet_state.view_pair.view().compress().to_bytes()
    );
}

#[test]
fn test_wallet_load_wrong_password() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_wallet.bin");

    let correct_password = "correct_password";
    let wrong_password = "wrong_password";

    // Create and save a wallet
    let seed = Seed::new(&mut OsRng, Language::English);
    let wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        correct_password,
        wallet_path.clone(),
        0,
    )
    .expect("Failed to create WalletState");

    wallet_state.save(correct_password).expect("Failed to save wallet");

    // Try to load with wrong password
    let result = WalletState::load_from_file(&wallet_path, wrong_password);

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            WalletError::InvalidPassword => {}, // Expected
            e => panic!("Expected InvalidPassword, got {:?}", e),
        }
    }
}

#[test]
fn test_wallet_save_to_custom_path() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let default_path = temp_dir.path().join("default.bin");
    let custom_path = temp_dir.path().join("custom_location.bin");

    let password = "secure_password_123";

    // Create a wallet with default path
    let seed = Seed::new(&mut OsRng, Language::English);
    let wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        default_path.clone(),
        0,
    )
    .expect("Failed to create WalletState");

    // Save to custom path
    wallet_state.save_to_file(&custom_path, password)
        .expect("Failed to save to custom path");

    // Verify custom path exists
    assert!(custom_path.exists());

    // Default path should NOT exist
    assert!(!default_path.exists());

    // Load from custom path
    let loaded = WalletState::load_from_file(&custom_path, password)
        .expect("Failed to load from custom path");

    assert_eq!(
        loaded.seed.as_ref().expect("Seed should be Some").to_string(),
        wallet_state.seed.as_ref().expect("Seed should be Some").to_string()
    );
}

#[test]
fn test_wallet_corrupted_file_wrong_magic() {
    use std::fs;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("corrupted.bin");

    // Write a file with wrong magic bytes
    let mut fake_data = b"FAKE".to_vec(); // Wrong magic
    fake_data.extend_from_slice(&[1u8; 100]); // Random data

    fs::write(&wallet_path, &fake_data).expect("Failed to write fake file");

    // Try to load
    let result = WalletState::load_from_file(&wallet_path, "password");

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            WalletError::CorruptedFile(msg) => {
                assert!(msg.contains("Invalid magic bytes"));
            }
            e => panic!("Expected CorruptedFile, got {:?}", e),
        }
    }
}

#[test]
fn test_wallet_corrupted_file_truncated() {
    use std::fs;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("truncated.bin");

    // Write a truncated file (less than header size)
    fs::write(&wallet_path, &[1u8; 20]).expect("Failed to write truncated file");

    // Try to load
    let result = WalletState::load_from_file(&wallet_path, "password");

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            WalletError::CorruptedFile(msg) => {
                assert!(msg.contains("File too small"));
            }
            e => panic!("Expected CorruptedFile, got {:?}", e),
        }
    }
}

#[test]
fn test_wallet_unsupported_version() {
    use std::fs;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("future_version.bin");

    // Create a file with future version
    let mut fake_data = b"MNRS".to_vec(); // Correct magic
    fake_data.extend_from_slice(&999u32.to_le_bytes()); // Future version
    fake_data.extend_from_slice(&[1u8; 100]); // Rest of data

    fs::write(&wallet_path, &fake_data).expect("Failed to write fake file");

    // Try to load
    let result = WalletState::load_from_file(&wallet_path, "password");

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            WalletError::UnsupportedVersion(v) => {
                assert_eq!(v, 999);
            }
            e => panic!("Expected UnsupportedVersion, got {:?}", e),
        }
    }
}

#[test]
fn test_wallet_closed_cannot_save() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("closed.bin");

    let password = "secure_password_123";

    // Create a wallet
    let seed = Seed::new(&mut OsRng, Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        wallet_path.clone(),
        0,
    )
    .expect("Failed to create WalletState");

    // Close the wallet
    wallet_state.is_closed = true;

    // Try to save - should fail
    let result = wallet_state.save(password);

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            WalletError::WalletClosed => {}, // Expected
            e => panic!("Expected WalletClosed, got {:?}", e),
        }
    }
}

#[test]
fn test_view_only_wallet_file_io() {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("view_only.bin");

    let password = "view_only_password";

    // Generate test keys
    let spend_scalar = Scalar::from_bytes_mod_order([42u8; 32]);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let spend_public_key = spend_point.compress().to_bytes();
    let view_private_key = [99u8; 32];

    let wallet_state = WalletState::new_view_only(
        spend_public_key,
        view_private_key,
        Network::Testnet,
        password,
        wallet_path.clone(),
        500,
    )
    .expect("Failed to create view-only wallet");

    assert!(wallet_state.is_view_only());

    // Save the wallet
    wallet_state.save(password).expect("Failed to save view-only wallet");

    // Load the wallet
    let loaded = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load view-only wallet");

    // Verify it's still view-only
    assert!(loaded.is_view_only());
    assert_eq!(loaded.spend_key, None);
    assert_eq!(loaded.network, Network::Testnet);
    assert_eq!(loaded.refresh_from_height, 500);

    // Verify ViewPair was reconstructed correctly
    assert_eq!(
        loaded.view_pair.spend().compress().to_bytes(),
        wallet_state.view_pair.spend().compress().to_bytes()
    );
    assert_eq!(
        loaded.view_pair.view().compress().to_bytes(),
        wallet_state.view_pair.view().compress().to_bytes()
    );
}

#[test]
fn test_wallet_file_io_different_networks() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let password = "secure_password_123";

    for network in [Network::Mainnet, Network::Testnet, Network::Stagenet] {
        let wallet_path = temp_dir.path().join(format!("{:?}.bin", network));

        let seed = Seed::new(&mut OsRng, Language::English);
        let wallet_state = WalletState::new(
            seed,
            String::from("English"),
            network,
            password,
            wallet_path.clone(),
            0,
        )
        .expect("Failed to create WalletState");

        // Save
        wallet_state.save(password).expect("Failed to save wallet");

        // Load
        let loaded = WalletState::load_from_file(&wallet_path, password)
            .expect("Failed to load wallet");

        assert_eq!(loaded.network, network);
    }
}

#[test]
fn test_wallet_file_io_with_outputs_and_transactions() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("full_wallet.bin");
    let password = "secure_password_123";

    // Create a wallet with lots of data
    let seed = Seed::new(&mut OsRng, Language::English);
    let mut wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        wallet_path.clone(),
        0,
    )
    .expect("Failed to create WalletState");

    // Add multiple outputs
    for i in 0..10 {
        let output = SerializableOutput {
            tx_hash: [i; 32],
            output_index: i as u64,
            amount: (i as u64 + 1) * 1000000000000,
            key_image: [i + 10; 32],
            subaddress_indices: (0, i as u32),
            height: 100 + i as u64,
            unlocked: true,
            spent: i % 3 == 0,
            frozen: i % 4 == 0,
        };
        wallet_state.outputs.insert([i + 10; 32], output);

        if i % 3 == 0 {
            wallet_state.spent_outputs.insert([i + 10; 32]);
        }
        if i % 4 == 0 {
            wallet_state.frozen_outputs.insert([i + 10; 32]);
        }
    }

    let balance_before = wallet_state.get_balance();
    let unlocked_balance_before = wallet_state.get_unlocked_balance();

    // Save
    wallet_state.save(password).expect("Failed to save wallet");

    // Load
    let loaded = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load wallet");

    // Verify all data
    assert_eq!(loaded.outputs.len(), 10);
    assert_eq!(loaded.spent_outputs.len(), wallet_state.spent_outputs.len());
    assert_eq!(loaded.frozen_outputs.len(), wallet_state.frozen_outputs.len());
    assert_eq!(loaded.get_balance(), balance_before);
    assert_eq!(loaded.get_unlocked_balance(), unlocked_balance_before);
}

#[test]
fn test_wallet_atomic_write_integrity() {
    use std::fs;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("atomic.bin");
    let password = "secure_password_123";

    // Create and save initial wallet
    let seed = Seed::new(&mut OsRng, Language::English);
    let wallet_state = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        wallet_path.clone(),
        0,
    )
    .expect("Failed to create WalletState");

    wallet_state.save(password).expect("Failed to save wallet");

    // Verify temp file doesn't exist after successful save
    let temp_path = wallet_path.with_extension("tmp");
    assert!(!temp_path.exists(), "Temp file should be cleaned up");

    // Verify wallet file exists
    assert!(wallet_path.exists());

    // Load and verify
    let loaded = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load wallet");

    assert_eq!(
        loaded.seed.as_ref().expect("Seed should be Some").to_string(),
        wallet_state.seed.as_ref().expect("Seed should be Some").to_string()
    );
}
// ==================== GETTER TESTS ====================

#[test]
fn test_getters_on_normal_wallet() {
    use monero_seed::Seed;
    use rand_core::OsRng;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_getter_wallet.bin");
    let password = "test_password_123";

    // Create a normal wallet (with spend key)
    let seed = Seed::new(&mut OsRng, Language::English);
    let wallet = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        wallet_path.clone(),
        0,
    )
    .expect("Failed to create wallet");

    // Test get_seed() - should return Some for normal wallet
    let seed_result = wallet.get_seed();
    assert!(seed_result.is_some(), "get_seed() should return Some for normal wallet");
    let seed_string = seed_result.unwrap();
    assert!(!seed_string.is_empty(), "Seed should not be empty");
    // Monero seeds are 25 words
    assert_eq!(seed_string.split_whitespace().count(), 25, "Seed should have 25 words");

    // Test get_seed_language()
    let language = wallet.get_seed_language();
    assert_eq!(language, "English", "Seed language should be English");

    // Test get_private_spend_key() - should return Some for normal wallet
    let spend_key = wallet.get_private_spend_key();
    assert!(spend_key.is_some(), "get_private_spend_key() should return Some for normal wallet");
    let spend_key_hex = spend_key.unwrap();
    assert_eq!(spend_key_hex.len(), 64, "Private spend key should be 64 hex chars (32 bytes)");
    assert!(spend_key_hex.chars().all(|c| c.is_ascii_hexdigit()), "Spend key should be valid hex");

    // Test get_private_view_key()
    let view_key = wallet.get_private_view_key();
    assert_eq!(view_key.len(), 64, "Private view key should be 64 hex chars (32 bytes)");
    assert!(view_key.chars().all(|c| c.is_ascii_hexdigit()), "View key should be valid hex");

    // Test get_public_spend_key()
    let pub_spend = wallet.get_public_spend_key();
    assert_eq!(pub_spend.len(), 64, "Public spend key should be 64 hex chars (32 bytes)");
    assert!(pub_spend.chars().all(|c| c.is_ascii_hexdigit()), "Public spend key should be valid hex");

    // Test get_public_view_key()
    let pub_view = wallet.get_public_view_key();
    assert_eq!(pub_view.len(), 64, "Public view key should be 64 hex chars (32 bytes)");
    assert!(pub_view.chars().all(|c| c.is_ascii_hexdigit()), "Public view key should be valid hex");

    // Test get_path()
    let path = wallet.get_path();
    assert_eq!(path, wallet_path.as_path(), "get_path() should return the correct wallet path");

    // Test is_view_only() - should be false for normal wallet
    assert!(!wallet.is_view_only(), "Normal wallet should not be view-only");

    // Test is_closed flag - should be false initially
    assert!(!wallet.is_closed, "Wallet should not be closed initially");
}

#[test]
fn test_getters_on_view_only_wallet() {
    use monero_seed::Seed;
    use rand_core::OsRng;
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar, edwards::EdwardsPoint};
    use sha3::{Digest, Keccak256};
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_view_only_getter_wallet.bin");
    let password = "view_only_password";

    // Create keys from a seed (but won't store the spend key)
    let seed = Seed::new(&mut OsRng, Language::Spanish);
    let spend: [u8; 32] = *seed.entropy();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let view: [u8; 32] = Keccak256::digest(&spend).into();

    // Create a view-only wallet
    let wallet = WalletState::new_view_only(
        spend_point.compress().to_bytes(),
        view,
        Network::Testnet,
        password,
        wallet_path.clone(),
        0,
    )
    .expect("Failed to create view-only wallet");

    // Test get_seed() - should return None for view-only wallet
    assert!(wallet.get_seed().is_none(), "get_seed() should return None for view-only wallet");

    // Test get_private_spend_key() - should return None for view-only wallet
    assert!(
        wallet.get_private_spend_key().is_none(),
        "get_private_spend_key() should return None for view-only wallet"
    );

    // Test get_private_view_key() - should work for view-only wallet
    let view_key = wallet.get_private_view_key();
    assert_eq!(view_key.len(), 64, "Private view key should be 64 hex chars");
    assert!(view_key.chars().all(|c| c.is_ascii_hexdigit()), "View key should be valid hex");

    // Test get_public_spend_key() - should work for view-only wallet
    let pub_spend = wallet.get_public_spend_key();
    assert_eq!(pub_spend.len(), 64, "Public spend key should be 64 hex chars");
    assert!(pub_spend.chars().all(|c| c.is_ascii_hexdigit()), "Public spend key should be valid hex");

    // Test get_public_view_key() - should work for view-only wallet
    let pub_view = wallet.get_public_view_key();
    assert_eq!(pub_view.len(), 64, "Public view key should be 64 hex chars");
    assert!(pub_view.chars().all(|c| c.is_ascii_hexdigit()), "Public view key should be valid hex");

    // Test is_view_only() - should be true
    assert!(wallet.is_view_only(), "View-only wallet should report as view-only");

    // Test get_path()
    assert_eq!(wallet.get_path(), wallet_path.as_path());
}

#[test]
fn test_getter_key_consistency() {
    use monero_seed::Seed;
    use rand_core::OsRng;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_consistency_wallet.bin");
    let password = "consistency_test";

    let seed = Seed::new(&mut OsRng, Language::English);
    let wallet = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        wallet_path,
        0,
    )
    .expect("Failed to create wallet");

    // Get keys multiple times to ensure consistency
    let seed1 = wallet.get_seed();
    let seed2 = wallet.get_seed();
    assert_eq!(seed1, seed2, "get_seed() should return consistent results");

    let priv_spend1 = wallet.get_private_spend_key();
    let priv_spend2 = wallet.get_private_spend_key();
    assert_eq!(priv_spend1, priv_spend2, "get_private_spend_key() should return consistent results");

    let priv_view1 = wallet.get_private_view_key();
    let priv_view2 = wallet.get_private_view_key();
    assert_eq!(priv_view1, priv_view2, "get_private_view_key() should return consistent results");

    let pub_spend1 = wallet.get_public_spend_key();
    let pub_spend2 = wallet.get_public_spend_key();
    assert_eq!(pub_spend1, pub_spend2, "get_public_spend_key() should return consistent results");

    let pub_view1 = wallet.get_public_view_key();
    let pub_view2 = wallet.get_public_view_key();
    assert_eq!(pub_view1, pub_view2, "get_public_view_key() should return consistent results");
}

#[test]
fn test_getter_after_save_and_load() {
    use monero_seed::Seed;
    use rand_core::OsRng;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_persistence_getter_wallet.bin");
    let password = "test_password_456";

    // Create and save wallet
    let seed = Seed::new(&mut OsRng, Language::German);
    let original_wallet = WalletState::new(
        seed,
        String::from("German"),
        Network::Stagenet,
        password,
        wallet_path.clone(),
        0,
    )
    .expect("Failed to create wallet");

    // Capture original values
    let original_seed = original_wallet.get_seed().unwrap();
    let original_language = original_wallet.get_seed_language().to_string();
    let original_priv_spend = original_wallet.get_private_spend_key().unwrap();
    let original_priv_view = original_wallet.get_private_view_key();
    let original_pub_spend = original_wallet.get_public_spend_key();
    let original_pub_view = original_wallet.get_public_view_key();

    // Save wallet
    original_wallet.save(password).expect("Failed to save wallet");

    // Load wallet
    let loaded_wallet = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load wallet");

    // Verify all getters return the same values
    assert_eq!(loaded_wallet.get_seed().unwrap(), original_seed, "Seed should match after load");
    assert_eq!(loaded_wallet.get_seed_language(), original_language, "Language should match after load");
    assert_eq!(loaded_wallet.get_private_spend_key().unwrap(), original_priv_spend, "Private spend key should match after load");
    assert_eq!(loaded_wallet.get_private_view_key(), original_priv_view, "Private view key should match after load");
    assert_eq!(loaded_wallet.get_public_spend_key(), original_pub_spend, "Public spend key should match after load");
    assert_eq!(loaded_wallet.get_public_view_key(), original_pub_view, "Public view key should match after load");
    assert_eq!(loaded_wallet.get_path(), wallet_path.as_path(), "Path should match after load");
    assert!(!loaded_wallet.is_view_only(), "Wallet should still be normal (not view-only) after load");
}

#[test]
fn test_getter_is_closed_flag() {
    use monero_seed::Seed;
    use rand_core::OsRng;
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_closed_wallet.bin");
    let password = "closed_test";

    let seed = Seed::new(&mut OsRng, Language::English);
    let mut wallet = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        wallet_path,
        0,
    )
    .expect("Failed to create wallet");

    // Initially should not be closed
    assert!(!wallet.is_closed, "Wallet should not be closed initially");

    // Manually set closed flag (simulating close operation)
    wallet.is_closed = true;

    // Now should be closed
    assert!(wallet.is_closed, "Wallet should be closed after setting flag");
}

// ==================== END GETTER TESTS ====================
