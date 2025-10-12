use monero_rust::{types::SerializableOutput, wallet_state::WalletState, Language, Network, WalletError};
use monero_seed::Seed;
use rand_core::OsRng;
use std::path::PathBuf;

#[test]
fn test_wallet_state_json_serialization_roundtrip() {
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

    wallet_state.daemon_address = Some(String::from("http://node.example.com:18081"));
    wallet_state.is_connected = true;
    wallet_state.daemon_height = 1000;
    wallet_state.current_scanned_height = 500;

    let json = serde_json::to_string(&wallet_state).expect("Failed to serialize to JSON");

    let deserialized: WalletState =
        serde_json::from_str(&json).expect("Failed to deserialize from JSON");

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

    let binary = bincode::serialize(&wallet_state).expect("Failed to serialize to bincode");

    let deserialized: WalletState =
        bincode::deserialize(&binary).expect("Failed to deserialize from bincode");

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

        let binary = bincode::serialize(&wallet_state).expect("Failed to serialize");
        let deserialized: WalletState =
            bincode::deserialize(&binary).expect("Failed to deserialize");

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

    let binary = bincode::serialize(&wallet_state).expect("Failed to serialize");
    let deserialized: WalletState =
        bincode::deserialize(&binary).expect("Failed to deserialize");

    let balance_after = deserialized.get_balance();

    assert_eq!(balance_before, balance_after);
    assert_eq!(deserialized.outputs.len(), 5);
    assert_eq!(deserialized.spent_outputs.len(), 3);
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

    assert!(!wallet_state.is_synced());

    wallet_state.is_connected = true;
    wallet_state.daemon_height = 1000;
    wallet_state.current_scanned_height = 500;
    assert!(!wallet_state.is_synced());

    wallet_state.current_scanned_height = 1000;
    assert!(wallet_state.is_synced());

    let json = serde_json::to_string(&wallet_state).expect("Failed to serialize");
    let deserialized: WalletState =
        serde_json::from_str(&json).expect("Failed to deserialize");

    assert!(deserialized.is_synced());
}

#[test]
fn test_wallet_save_and_load_roundtrip() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_wallet.bin");

    let password = "my_secure_password_123!";

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

    wallet_state.save(password).expect("Failed to save wallet");

    assert!(wallet_path.exists());

    let loaded_wallet = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load wallet");

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

    let result = WalletState::load_from_file(&wallet_path, wrong_password);

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            WalletError::InvalidPassword => {},
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

    wallet_state.save_to_file(&custom_path, password)
        .expect("Failed to save to custom path");

    assert!(custom_path.exists());
    assert!(!default_path.exists());

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

    let mut fake_data = b"FAKE".to_vec();
    fake_data.extend_from_slice(&[1u8; 100]);

    fs::write(&wallet_path, &fake_data).expect("Failed to write fake file");

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

    fs::write(&wallet_path, &[1u8; 20]).expect("Failed to write truncated file");

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

    wallet_state.is_closed = true;

    let result = wallet_state.save(password);

    assert!(result.is_err());
    if let Err(e) = result {
        match e {
            WalletError::WalletClosed => {},
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

    wallet_state.save(password).expect("Failed to save view-only wallet");

    let loaded = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load view-only wallet");

    assert!(loaded.is_view_only());
    assert_eq!(loaded.spend_key, None);
    assert_eq!(loaded.network, Network::Testnet);
    assert_eq!(loaded.refresh_from_height, 500);
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

        wallet_state.save(password).expect("Failed to save wallet");

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

    wallet_state.save(password).expect("Failed to save wallet");

    let loaded = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load wallet");

    assert_eq!(loaded.outputs.len(), 10);
    assert_eq!(loaded.spent_outputs.len(), wallet_state.spent_outputs.len());
    assert_eq!(loaded.frozen_outputs.len(), wallet_state.frozen_outputs.len());
    assert_eq!(loaded.get_balance(), balance_before);
    assert_eq!(loaded.get_unlocked_balance(), unlocked_balance_before);
}

#[test]
fn test_wallet_atomic_write_integrity() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("atomic.bin");
    let password = "secure_password_123";

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

    let temp_path = wallet_path.with_extension("tmp");
    assert!(!temp_path.exists(), "Temp file should be cleaned up");
    assert!(wallet_path.exists());

    let loaded = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load wallet");

    assert_eq!(
        loaded.seed.as_ref().expect("Seed should be Some").to_string(),
        wallet_state.seed.as_ref().expect("Seed should be Some").to_string()
    );
}

#[test]
fn test_getters_on_normal_wallet() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_getter_wallet.bin");
    let password = "test_password_123";

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

    let seed_result = wallet.get_seed();
    assert!(seed_result.is_some());
    let seed_string = seed_result.unwrap();
    assert!(!seed_string.is_empty());
    assert_eq!(seed_string.split_whitespace().count(), 25);

    assert_eq!(wallet.get_seed_language(), "English");

    let spend_key = wallet.get_private_spend_key();
    assert!(spend_key.is_some());
    let spend_key_hex = spend_key.unwrap();
    assert_eq!(spend_key_hex.len(), 64);
    assert!(spend_key_hex.chars().all(|c| c.is_ascii_hexdigit()));

    let view_key = wallet.get_private_view_key();
    assert_eq!(view_key.len(), 64);
    assert!(view_key.chars().all(|c| c.is_ascii_hexdigit()));

    let pub_spend = wallet.get_public_spend_key();
    assert_eq!(pub_spend.len(), 64);
    assert!(pub_spend.chars().all(|c| c.is_ascii_hexdigit()));

    let pub_view = wallet.get_public_view_key();
    assert_eq!(pub_view.len(), 64);
    assert!(pub_view.chars().all(|c| c.is_ascii_hexdigit()));

    assert_eq!(wallet.get_path(), wallet_path.as_path());
    assert!(!wallet.is_view_only());
    assert!(!wallet.is_closed);
}

#[test]
fn test_getters_on_view_only_wallet() {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar, edwards::EdwardsPoint};
    use sha3::{Digest, Keccak256};
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_view_only_getter_wallet.bin");
    let password = "view_only_password";

    let seed = Seed::new(&mut OsRng, Language::Spanish);
    let spend: [u8; 32] = *seed.entropy();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let view: [u8; 32] = Keccak256::digest(&spend).into();

    let wallet = WalletState::new_view_only(
        spend_point.compress().to_bytes(),
        view,
        Network::Testnet,
        password,
        wallet_path.clone(),
        0,
    )
    .expect("Failed to create view-only wallet");

    assert!(wallet.get_seed().is_none());
    assert!(wallet.get_private_spend_key().is_none());

    let view_key = wallet.get_private_view_key();
    assert_eq!(view_key.len(), 64);
    assert!(view_key.chars().all(|c| c.is_ascii_hexdigit()));

    let pub_spend = wallet.get_public_spend_key();
    assert_eq!(pub_spend.len(), 64);
    assert!(pub_spend.chars().all(|c| c.is_ascii_hexdigit()));

    let pub_view = wallet.get_public_view_key();
    assert_eq!(pub_view.len(), 64);
    assert!(pub_view.chars().all(|c| c.is_ascii_hexdigit()));

    assert!(wallet.is_view_only());
    assert_eq!(wallet.get_path(), wallet_path.as_path());
}

#[test]
fn test_getter_key_consistency() {
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

    assert_eq!(wallet.get_seed(), wallet.get_seed());
    assert_eq!(wallet.get_private_spend_key(), wallet.get_private_spend_key());
    assert_eq!(wallet.get_private_view_key(), wallet.get_private_view_key());
    assert_eq!(wallet.get_public_spend_key(), wallet.get_public_spend_key());
    assert_eq!(wallet.get_public_view_key(), wallet.get_public_view_key());
}

#[test]
fn test_getter_after_save_and_load() {
    use tempfile::tempdir;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_persistence_getter_wallet.bin");
    let password = "test_password_456";

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

    let original_seed = original_wallet.get_seed().unwrap();
    let original_language = original_wallet.get_seed_language().to_string();
    let original_priv_spend = original_wallet.get_private_spend_key().unwrap();
    let original_priv_view = original_wallet.get_private_view_key();
    let original_pub_spend = original_wallet.get_public_spend_key();
    let original_pub_view = original_wallet.get_public_view_key();

    original_wallet.save(password).expect("Failed to save wallet");

    let loaded_wallet = WalletState::load_from_file(&wallet_path, password)
        .expect("Failed to load wallet");

    assert_eq!(loaded_wallet.get_seed().unwrap(), original_seed);
    assert_eq!(loaded_wallet.get_seed_language(), original_language);
    assert_eq!(loaded_wallet.get_private_spend_key().unwrap(), original_priv_spend);
    assert_eq!(loaded_wallet.get_private_view_key(), original_priv_view);
    assert_eq!(loaded_wallet.get_public_spend_key(), original_pub_spend);
    assert_eq!(loaded_wallet.get_public_view_key(), original_pub_view);
    assert_eq!(loaded_wallet.get_path(), wallet_path.as_path());
    assert!(!loaded_wallet.is_view_only());
}

#[test]
fn test_getter_is_closed_flag() {
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

    assert!(!wallet.is_closed);
    wallet.is_closed = true;
    assert!(wallet.is_closed);
}
