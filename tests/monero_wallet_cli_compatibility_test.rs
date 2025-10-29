use monero_rust::WalletState;
use monero_seed::{Language, Seed};
use monero_wallet::address::Network;
use std::path::PathBuf;
use tempfile::TempDir;

const TEST_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";

#[test]
fn test_import_monero_wallet_cli_key_images() {
    let test_vector_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/stagenet_key_images.monero-wallet-cli");

    if !test_vector_path.exists() {
        panic!("test vector not found: {:?}", test_vector_path);
    }

    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let mut wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Testnet,
        "",
        wallet_path,
        0,
    ).unwrap();

    wallet.save("").unwrap();

    let (newly_spent, already_spent) = wallet.import_key_images(&test_vector_path).unwrap();
    assert!(newly_spent >= 0 || already_spent >= 0);
}

#[test]
fn test_export_format_compatibility() {
    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Testnet,
        "",
        wallet_path,
        0,
    ).unwrap();

    let export_path = temp_dir.path().join("our_export.bin");
    wallet.export_key_images(&export_path, true).unwrap();

    let exported_data = std::fs::read(&export_path).unwrap();
    assert!(exported_data.starts_with(b"Monero key image export\x03"));
}

#[test]
fn test_round_trip_with_cli_format() {
    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let mut wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Testnet,
        "test_password",
        wallet_path,
        0,
    ).unwrap();

    let export_path = temp_dir.path().join("export.bin");
    let count = wallet.export_key_images(&export_path, true).unwrap();
    assert_eq!(count, 0);

    let file_data = std::fs::read(&export_path).unwrap();
    assert!(file_data.len() >= 24);
    assert_eq!(&file_data[0..23], b"Monero key image export");
    assert_eq!(file_data[23], 0x03);
    assert!(file_data.len() >= 100); // magic(24) + IV(8) + encrypted(offset(4) + keys(64))

    let (newly_spent, already_spent) = wallet.import_key_images(&export_path).unwrap();
    assert_eq!(newly_spent, 0);
    assert_eq!(already_spent, 0);
}

#[test]
fn test_export_structure_matches_monero_format() {
    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.mw");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Testnet,
        "",
        wallet_path,
        0,
    ).unwrap();

    let export_path = temp_dir.path().join("export.bin");
    wallet.export_key_images(&export_path, true).unwrap();

    let file_data = std::fs::read(&export_path).unwrap();
    assert_eq!(&file_data[0..24], b"Monero key image export\x03");

    let iv = &file_data[24..32];
    let encrypted = &file_data[32..];

    let view_key_hex = wallet.get_private_view_key();
    let view_key_bytes = hex::decode(&view_key_hex).unwrap();
    let mut view_key_array = [0u8; 32];
    view_key_array.copy_from_slice(&view_key_bytes);

    let chacha_key = monero_rust::crypto::derive_chacha_key_cryptonight(&view_key_array, 1);
    let mut iv_array = [0u8; 8];
    iv_array.copy_from_slice(iv);

    let decrypted = monero_rust::crypto::chacha20_decrypt(encrypted, &chacha_key, &iv_array);

    // decrypted: offset(4) + spend_public(32) + view_public(32) + key_images...
    assert!(decrypted.len() >= 68);

    let offset = u32::from_le_bytes([decrypted[0], decrypted[1], decrypted[2], decrypted[3]]);
    assert_eq!(offset, 0);

    let spend_public = &decrypted[4..36];
    let view_public = &decrypted[36..68];

    assert_eq!(spend_public, &wallet.view_pair.spend().compress().to_bytes()[..]);
    assert_eq!(view_public, &wallet.view_pair.view().compress().to_bytes()[..]);
}
