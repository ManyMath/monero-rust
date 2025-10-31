use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_rust::types::SerializableOutput;
use monero_rust::{Network, WalletError, WalletState};
use monero_seed::Seed;
use rand_core::OsRng;
use std::path::PathBuf;

fn create_test_wallet() -> WalletState {
    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap()
}

#[test]
fn test_wallet_state_creation() {
    let wallet = create_test_wallet();

    assert!(!wallet.is_view_only());
    assert!(!wallet.is_closed);
    assert_eq!(wallet.network, Network::Mainnet);
    assert_eq!(wallet.get_balance(), 0);
}

#[test]
fn test_view_only_wallet() {
    let spend_scalar = Scalar::from_bytes_mod_order([1u8; 32]);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let spend_public_key = spend_point.compress().to_bytes();
    let view_private_key = [2u8; 32];

    let wallet = WalletState::new_view_only(
        spend_public_key,
        view_private_key,
        Network::Testnet,
        "test_password",
        PathBuf::from("view_only.bin"),
        100,
    )
    .unwrap();

    assert!(wallet.is_view_only());
    assert_eq!(wallet.network, Network::Testnet);
    assert_eq!(wallet.refresh_from_height, 100);
}

#[test]
fn test_balance_calculation() {
    let mut wallet = create_test_wallet();

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
        payment_id: None,
        key_offset: None,
        output_public_key: None,
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
        payment_id: None,
        key_offset: None,
        output_public_key: None,
    };

    wallet.outputs.insert([1u8; 32], output1);
    wallet.outputs.insert([2u8; 32], output2);

    assert_eq!(wallet.get_balance(), 3000000000000);

    wallet.spent_outputs.insert([1u8; 32]);
    assert_eq!(wallet.get_balance(), 2000000000000);
}

#[test]
fn test_password_hashing() {
    let password = "my_secure_password_123!";
    let salt = WalletState::generate_salt();

    let hash1 = WalletState::hash_password(password, &salt).unwrap();
    let hash2 = WalletState::hash_password(password, &salt).unwrap();
    assert_eq!(hash1, hash2);

    let different_salt = WalletState::generate_salt();
    let hash3 = WalletState::hash_password(password, &different_salt).unwrap();
    assert_ne!(hash1, hash3);

    let hash4 = WalletState::hash_password("different_password", &salt).unwrap();
    assert_ne!(hash1, hash4);
}

#[test]
fn test_password_verification() {
    let password = "correct_password";
    let salt = WalletState::generate_salt();
    let hash = WalletState::hash_password(password, &salt).unwrap();

    assert!(WalletState::verify_password(password, &salt, &hash).is_ok());
    assert!(WalletState::verify_password("wrong_password", &salt, &hash).is_err());

    let wrong_salt = WalletState::generate_salt();
    assert!(WalletState::verify_password(password, &wrong_salt, &hash).is_err());
}

#[test]
fn test_wallet_stores_password_correctly() {
    let password = "wallet_password_456";
    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let wallet = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        PathBuf::from("test.bin"),
        0,
    )
    .unwrap();

    assert!(WalletState::verify_password(password, &wallet.password_salt, &wallet.password_hash).is_ok());
    assert!(WalletState::verify_password("wrong", &wallet.password_salt, &wallet.password_hash).is_err());
}

#[test]
fn test_is_synced() {
    let mut wallet = create_test_wallet();

    assert!(!wallet.is_synced());

    wallet.is_connected = true;
    wallet.daemon_height = 100;
    wallet.current_scanned_height = 50;
    assert!(!wallet.is_synced());

    wallet.current_scanned_height = 100;
    assert!(wallet.is_synced());
}

#[test]
fn test_scanner_initialized() {
    let wallet = create_test_wallet();
    assert_eq!(wallet.get_registered_subaddresses().len(), 0);
}

#[test]
fn test_register_subaddress() {
    let mut wallet = create_test_wallet();

    assert_eq!(wallet.registered_subaddresses.len(), 0);

    wallet.register_subaddress(0, 1).unwrap();
    assert_eq!(wallet.registered_subaddresses.len(), 1);
    assert!(wallet.get_registered_subaddresses().contains(&(0, 1)));

    wallet.register_subaddress(0, 1).unwrap();
    assert_eq!(wallet.registered_subaddresses.len(), 1);
}

#[test]
fn test_register_subaddress_range() {
    let mut wallet = create_test_wallet();

    let count = wallet.register_subaddress_range(0, 1, 5).unwrap();
    assert_eq!(count, 5);
    assert_eq!(wallet.registered_subaddresses.len(), 5);

    let registered = wallet.get_registered_subaddresses();
    for addr in 1..=5 {
        assert!(registered.contains(&(0, addr)));
    }
}

#[test]
fn test_handle_reorganization() {
    let mut wallet = create_test_wallet();

    for (i, height) in [100u64, 105, 110].iter().enumerate() {
        let output = SerializableOutput {
            tx_hash: [i as u8; 32],
            output_index: 0,
            amount: 1000000000000,
            key_image: [i as u8; 32],
            subaddress_indices: (0, 0),
            height: *height,
            unlocked: false,
            spent: false,
            frozen: false,
            payment_id: None,
            key_offset: None,
            output_public_key: None,
        };
        wallet.outputs.insert([i as u8; 32], output);
    }
    wallet.current_scanned_height = 110;
    assert_eq!(wallet.outputs.len(), 3);

    let removed = wallet.handle_reorganization(105);
    assert_eq!(removed, 2);
    assert_eq!(wallet.outputs.len(), 1);
    assert!(wallet.outputs.contains_key(&[0u8; 32]));
    assert_eq!(wallet.current_scanned_height, 104);
}

#[test]
fn test_reorg_cleans_hashsets() {
    let mut wallet = create_test_wallet();

    let output = SerializableOutput {
        tx_hash: [1u8; 32],
        output_index: 0,
        amount: 1000000000000,
        key_image: [1u8; 32],
        subaddress_indices: (0, 0),
        height: 110,
        unlocked: false,
        spent: false,
        frozen: false,
        payment_id: None,
        key_offset: None,
        output_public_key: None,
    };
    wallet.outputs.insert([1u8; 32], output);
    wallet.spent_outputs.insert([1u8; 32]);
    wallet.frozen_outputs.insert([1u8; 32]);

    wallet.handle_reorganization(105);

    assert!(wallet.spent_outputs.is_empty());
    assert!(wallet.frozen_outputs.is_empty());
}

#[test]
fn test_subaddresses_persist() {
    use tempfile::NamedTempFile;

    let temp = NamedTempFile::new().unwrap();
    let path = temp.path().to_path_buf();

    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let mut wallet = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test",
        path.clone(),
        0,
    )
    .unwrap();

    wallet.register_subaddress(0, 1).unwrap();
    wallet.register_subaddress(0, 2).unwrap();
    wallet.register_subaddress(1, 0).unwrap();
    assert_eq!(wallet.registered_subaddresses.len(), 3);

    wallet.save("test").unwrap();

    let loaded = WalletState::load_from_file(&path, "test").unwrap();
    assert_eq!(loaded.registered_subaddresses.len(), 3);

    let registered = loaded.get_registered_subaddresses();
    assert!(registered.contains(&(0, 1)));
    assert!(registered.contains(&(0, 2)));
    assert!(registered.contains(&(1, 0)));
}

// ==================== SYNC TESTS ====================

#[tokio::test]
async fn test_start_syncing_requires_connection() {
    let mut wallet = create_test_wallet();
    let result = wallet.start_syncing().await;
    assert!(matches!(result, Err(WalletError::NotConnected)));
}

#[tokio::test]
async fn test_start_syncing_idempotent() {
    let mut wallet = create_test_wallet();
    wallet.is_connected = true;

    wallet.start_syncing().await.unwrap();
    assert!(wallet.is_syncing);

    wallet.start_syncing().await.unwrap();
    assert!(wallet.is_syncing);
}

#[tokio::test]
async fn test_stop_syncing() {
    let mut wallet = create_test_wallet();
    wallet.is_connected = true;

    wallet.start_syncing().await.unwrap();
    assert!(wallet.is_syncing);

    wallet.stop_syncing().await;
    assert!(!wallet.is_syncing);
}

#[tokio::test]
async fn test_sync_once_requires_connection() {
    let mut wallet = create_test_wallet();
    let result = wallet.sync_once().await;
    assert!(matches!(result, Err(WalletError::NotConnected)));
}

#[tokio::test]
async fn test_sync_once_closed_wallet() {
    let mut wallet = create_test_wallet();
    wallet.is_closed = true;
    let result = wallet.sync_once().await;
    assert!(matches!(result, Err(WalletError::WalletClosed)));
}

#[test]
fn test_sync_interval_default() {
    let wallet = create_test_wallet();
    assert_eq!(wallet.sync_interval, std::time::Duration::from_secs(1));
}

#[test]
fn test_sync_fields_not_serialized() {
    let wallet = create_test_wallet();
    assert!(wallet.sync_handle.is_none());
    assert!(wallet.sync_progress_callback.is_none());
    assert!(!wallet.is_syncing);
}

#[test]
fn test_get_refresh_from_height() {
    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let wallet = WalletState::new(
        seed,
        String::from("English"),
        Network::Stagenet,
        "password",
        PathBuf::from("test.bin"),
        123456,
    )
    .unwrap();

    assert_eq!(wallet.get_refresh_from_height(), 123456);
}

#[test]
fn test_set_refresh_from_height() {
    let mut wallet = create_test_wallet();

    wallet.set_refresh_from_height(200000);

    assert_eq!(wallet.get_refresh_from_height(), 200000);
    assert_eq!(wallet.current_scanned_height, 200000);
}

#[test]
fn test_rescan_blockchain_clears_outputs() {
    let mut wallet = create_test_wallet();
    wallet.current_scanned_height = 100500;

    let output = SerializableOutput {
        tx_hash: [1u8; 32],
        output_index: 0,
        amount: 1000000000000,
        key_image: [3u8; 32],
        subaddress_indices: (0, 0),
        height: 100500,
        unlocked: true,
        spent: false,
        frozen: false,
        payment_id: None,
        key_offset: None,
        output_public_key: None,
    };
    wallet.outputs.insert([3u8; 32], output);

    assert_eq!(wallet.outputs.len(), 1);

    wallet.rescan_blockchain();

    assert_eq!(wallet.outputs.len(), 0);
    assert_eq!(wallet.current_scanned_height, 0);
}

#[test]
fn test_rescan_blockchain_clears_tracking_sets() {
    let mut wallet = create_test_wallet();

    wallet.spent_outputs.insert([1u8; 32]);
    wallet.frozen_outputs.insert([2u8; 32]);

    assert_eq!(wallet.spent_outputs.len(), 1);
    assert_eq!(wallet.frozen_outputs.len(), 1);

    wallet.rescan_blockchain();

    assert!(wallet.spent_outputs.is_empty());
    assert!(wallet.frozen_outputs.is_empty());
}

#[test]
fn test_rescan_preserves_keys() {
    let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
    let seed_string = seed.to_string().to_string();

    let mut wallet = WalletState::new(
        seed,
        String::from("English"),
        Network::Stagenet,
        "password",
        PathBuf::from("test.bin"),
        100000,
    )
    .unwrap();

    wallet.register_subaddress(0, 1).unwrap();
    let subaddresses = wallet.get_registered_subaddresses();

    wallet.rescan_blockchain();

    assert_eq!(*wallet.seed.as_ref().unwrap().to_string(), seed_string);
    assert_eq!(wallet.get_registered_subaddresses(), subaddresses);
    assert_eq!(wallet.network, Network::Stagenet);
}

#[test]
fn test_set_sync_progress_callback() {
    use std::sync::Arc;

    let mut wallet = create_test_wallet();

    let callback = Arc::new(Box::new(|_current: u64, _daemon: u64| {})
        as Box<dyn Fn(u64, u64) + Send + Sync>);

    wallet.set_sync_progress_callback(Some(callback));
    assert!(wallet.sync_progress_callback.is_some());

    wallet.set_sync_progress_callback(None);
    assert!(wallet.sync_progress_callback.is_none());
}
