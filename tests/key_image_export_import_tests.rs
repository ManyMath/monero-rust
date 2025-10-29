use monero_rust::{WalletState, WalletError};
use monero_rust::types::{SerializableOutput, KeyImage};
use monero_seed::{Seed, Language};
use monero_wallet::address::Network;
use tempfile::TempDir;
use std::ffi::CString;
use std::os::raw::c_char;

const TEST_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";

fn create_test_wallet_with_outputs() -> (WalletState, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("test_wallet.bin");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(TEST_SEED.to_string())
    ).unwrap();

    let mut wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        wallet_path,
        0,
    ).unwrap();

    for i in 0..5 {
        let mut key_image: KeyImage = [0u8; 32];
        key_image[0] = i as u8;

        let output = SerializableOutput {
            tx_hash: [i; 32],
            output_index: i as u64,
            amount: 1_000_000_000 * (i as u64 + 1),
            key_image,
            subaddress_indices: (0, i as u32),
            height: 1000 + i as u64,
            unlocked: true,
            spent: i % 2 == 0,
            frozen: false,
            payment_id: None,
        };

        wallet.outputs.insert(key_image, output);
        if i % 2 == 0 {
            wallet.spent_outputs.insert(key_image);
        }
    }

    (wallet, temp_dir)
}

#[test]
fn test_export_key_images_all_outputs() {
    let (wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_all.bin");

    let count = wallet.export_key_images(&export_path, true).unwrap();

    assert_eq!(count, 5);
    assert!(export_path.exists());
    assert!(std::fs::metadata(&export_path).unwrap().len() > 26);
}

#[test]
fn test_export_key_images_spent_only() {
    let (wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_spent.bin");

    let count = wallet.export_key_images(&export_path, false).unwrap();

    assert_eq!(count, 3);
    assert!(export_path.exists());
}

#[test]
fn test_export_key_images_empty_wallet() {
    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("empty_wallet.bin");

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new("hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower".to_string())
    ).unwrap();

    let wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        wallet_path,
        0,
    ).unwrap();

    let export_path = temp_dir.path().join("keyimages_empty.bin");
    let count = wallet.export_key_images(&export_path, true).unwrap();

    assert_eq!(count, 0);
    assert!(export_path.exists());
}

#[test]
fn test_import_key_images_round_trip() {
    let (mut wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_roundtrip.bin");

    wallet.spent_outputs.clear();
    for output in wallet.outputs.values_mut() {
        output.spent = false;
    }

    let export_count = wallet.export_key_images(&export_path, true).unwrap();
    assert_eq!(export_count, 5);

    let (spent, unspent) = wallet.import_key_images(&export_path).unwrap();

    assert_eq!(spent, 5);
    assert_eq!(unspent, 0);
    assert_eq!(wallet.spent_outputs.len(), 5);
}

#[test]
fn test_import_key_images_idempotent() {
    let (mut wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_idempotent.bin");

    wallet.spent_outputs.clear();
    for output in wallet.outputs.values_mut() {
        output.spent = false;
    }

    wallet.export_key_images(&export_path, true).unwrap();

    let (spent1, _) = wallet.import_key_images(&export_path).unwrap();
    let (spent2, unspent2) = wallet.import_key_images(&export_path).unwrap();

    assert_eq!(spent1, 5);
    assert_eq!(spent2, 0);
    assert_eq!(unspent2, 5);
}

#[test]
fn test_export_import_preserves_data() {
    let (mut wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_preserve.bin");

    let original_outputs: Vec<_> = wallet.outputs.keys().copied().collect();
    assert_eq!(original_outputs.len(), 5);

    wallet.export_key_images(&export_path, true).unwrap();
    wallet.import_key_images(&export_path).unwrap();

    for key_image in &original_outputs {
        assert!(wallet.outputs.contains_key(key_image));
        assert!(wallet.spent_outputs.contains(key_image));
    }
}

#[test]
fn test_import_key_images_wrong_wallet() {
    let (wallet1, temp_dir1) = create_test_wallet_with_outputs();
    let export_path = temp_dir1.path().join("keyimages_wallet1.bin");

    wallet1.export_key_images(&export_path, true).unwrap();

    let temp_dir2 = TempDir::new().unwrap();
    let wallet_path2 = temp_dir2.path().join("wallet2.bin");

    let seed2 = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new("sequence atlas unveil summon pebbles tuesday beer rudely snake rockets different fuselage woven tagged bested dented vegan hover rapid fawns obvious muppet randomly seasons randomly".to_string())
    ).unwrap();

    let mut wallet2 = WalletState::new(
        seed2,
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        wallet_path2,
        0,
    ).unwrap();

    let result = wallet2.import_key_images(&export_path);
    assert!(result.is_err());

    match result {
        Err(WalletError::InvalidResponse(_)) | Err(WalletError::EncryptionError(_)) => {}
        other => panic!("unexpected: {:?}", other),
    }
}

#[test]
fn test_export_creates_valid_magic_header() {
    let (wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_magic.bin");

    wallet.export_key_images(&export_path, true).unwrap();

    let file_contents = std::fs::read(&export_path).unwrap();

    assert!(file_contents.len() >= 24);
    assert_eq!(&file_contents[0..24], b"Monero key image export\x03");
}

#[test]
fn test_import_detects_invalid_magic() {
    let temp_dir = TempDir::new().unwrap();
    let invalid_file = temp_dir.path().join("invalid.bin");

    std::fs::write(&invalid_file, b"Not a key image file").unwrap();

    let (mut wallet, _) = create_test_wallet_with_outputs();
    let result = wallet.import_key_images(&invalid_file);

    assert!(result.is_err());
    match result {
        Err(WalletError::CorruptedFile(msg)) => assert!(msg.contains("magic")),
        other => panic!("expected CorruptedFile, got {:?}", other),
    }
}

#[test]
fn test_import_detects_truncated_file() {
    let temp_dir = TempDir::new().unwrap();
    let truncated_file = temp_dir.path().join("truncated.bin");

    std::fs::write(&truncated_file, &[1, 2, 3, 4, 5]).unwrap();

    let (mut wallet, _) = create_test_wallet_with_outputs();
    let result = wallet.import_key_images(&truncated_file);

    assert!(matches!(result, Err(WalletError::CorruptedFile(_))));
}

#[test]
fn test_export_with_closed_wallet() {
    let (mut wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_closed.bin");

    wallet.is_closed = true;

    let result = wallet.export_key_images(&export_path, true);
    assert!(matches!(result, Err(WalletError::WalletClosed)));
}

#[test]
fn test_import_with_closed_wallet() {
    let (wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_for_closed.bin");

    wallet.export_key_images(&export_path, true).unwrap();

    let mut wallet = wallet;
    wallet.is_closed = true;

    let result = wallet.import_key_images(&export_path);
    assert!(matches!(result, Err(WalletError::WalletClosed)));
}

#[test]
fn test_export_file_format_structure() {
    let (wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("keyimages_structure.bin");

    let count = wallet.export_key_images(&export_path, true).unwrap();
    assert_eq!(count, 5);

    let file_contents = std::fs::read(&export_path).unwrap();

    assert!(file_contents.len() >= 32);

    let encrypted_data_len = file_contents.len() - 24 - 8;
    // plaintext: offset(4) + spend_pub(32) + view_pub(32) + 5*(key_image(32) + sig(64)) = 548
    assert_eq!(encrypted_data_len, 4 + 32 + 32 + (96 * 5));
}

#[test]
fn test_export_different_wallets_different_files() {
    let temp_dir = TempDir::new().unwrap();

    let (wallet1, _temp1) = create_test_wallet_with_outputs();

    let seed2 = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new("sequence atlas unveil summon pebbles tuesday beer rudely snake rockets different fuselage woven tagged bested dented vegan hover rapid fawns obvious muppet randomly seasons randomly".to_string())
    ).unwrap();

    let wallet_path2 = temp_dir.path().join("wallet2.bin");
    let wallet2 = WalletState::new(
        seed2,
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        wallet_path2,
        0,
    ).unwrap();

    let export1 = temp_dir.path().join("export1.bin");
    let export2 = temp_dir.path().join("export2.bin");

    wallet1.export_key_images(&export1, true).unwrap();
    wallet2.export_key_images(&export2, true).unwrap();

    let content1 = std::fs::read(&export1).unwrap();
    let content2 = std::fs::read(&export2).unwrap();

    assert_ne!(content1, content2);
}

extern "C" {
    fn wallet_export_key_images(wallet: *const WalletState, filename: *const c_char, all: i32) -> i64;
    fn wallet_import_key_images(wallet: *mut WalletState, filename: *const c_char) -> i64;
}

#[test]
fn test_ffi_export_key_images_success() {
    let (wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("ffi_export.bin");
    let c_path = CString::new(export_path.to_str().unwrap()).unwrap();

    let result = unsafe { wallet_export_key_images(&wallet, c_path.as_ptr(), 1) };

    assert_eq!(result, 5);
    assert!(export_path.exists());
}

#[test]
fn test_ffi_export_key_images_null_wallet() {
    let temp_dir = TempDir::new().unwrap();
    let export_path = temp_dir.path().join("null_wallet.bin");
    let c_path = CString::new(export_path.to_str().unwrap()).unwrap();

    let result = unsafe { wallet_export_key_images(std::ptr::null(), c_path.as_ptr(), 1) };

    assert_eq!(result, -1);
}

#[test]
fn test_ffi_export_key_images_null_filename() {
    let (wallet, _) = create_test_wallet_with_outputs();

    let result = unsafe { wallet_export_key_images(&wallet, std::ptr::null(), 1) };

    assert_eq!(result, -2);
}

#[test]
fn test_ffi_import_key_images_success() {
    let (mut wallet, temp_dir) = create_test_wallet_with_outputs();
    let export_path = temp_dir.path().join("ffi_import.bin");
    let c_path = CString::new(export_path.to_str().unwrap()).unwrap();

    wallet.spent_outputs.clear();

    unsafe { wallet_export_key_images(&wallet, c_path.as_ptr(), 1) };

    let import_result = unsafe { wallet_import_key_images(&mut wallet, c_path.as_ptr()) };

    assert!(import_result >= 0);

    let spent = ((import_result >> 32) & 0xFFFFFFFF) as u32;
    let unspent = (import_result & 0xFFFFFFFF) as u32;

    assert_eq!(spent, 5);
    assert_eq!(unspent, 0);
}

#[test]
fn test_ffi_import_key_images_null_wallet() {
    let temp_dir = TempDir::new().unwrap();
    let import_path = temp_dir.path().join("null_import.bin");
    let c_path = CString::new(import_path.to_str().unwrap()).unwrap();

    let result = unsafe { wallet_import_key_images(std::ptr::null_mut(), c_path.as_ptr()) };

    assert_eq!(result, -1);
}

#[test]
fn test_ffi_import_key_images_null_filename() {
    let (mut wallet, _) = create_test_wallet_with_outputs();

    let result = unsafe { wallet_import_key_images(&mut wallet, std::ptr::null()) };

    assert_eq!(result, -2);
}

#[test]
fn test_ffi_round_trip() {
    let (mut wallet, temp_dir) = create_test_wallet_with_outputs();
    let path = temp_dir.path().join("ffi_roundtrip.bin");
    let c_path = CString::new(path.to_str().unwrap()).unwrap();

    wallet.spent_outputs.clear();

    let export_count = unsafe { wallet_export_key_images(&wallet, c_path.as_ptr(), 1) };
    assert_eq!(export_count, 5);

    let import_result = unsafe { wallet_import_key_images(&mut wallet, c_path.as_ptr()) };

    let spent = ((import_result >> 32) & 0xFFFFFFFF) as u32;
    assert_eq!(spent, 5);
    assert_eq!(wallet.spent_outputs.len(), 5);
}
