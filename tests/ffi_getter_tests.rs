/// FFI Integration tests for wallet getter functions.
///
/// These tests verify that the C FFI bindings work correctly, including:
/// - Proper null pointer handling
/// - Memory management (string allocation/deallocation)
/// - Correct data marshaling between Rust and C
/// - Error handling across FFI boundary

use monero_rust::{
    wallet_get_seed, wallet_get_seed_language, wallet_get_private_spend_key,
    wallet_get_private_view_key, wallet_get_public_spend_key, wallet_get_public_view_key,
    wallet_get_path, wallet_is_view_only, wallet_is_closed, free_string,
    wallet_state::WalletState, Language, Network,
};
use monero_seed::Seed;
use rand_core::OsRng;
use std::ffi::CStr;
use std::path::PathBuf;
use tempfile::tempdir;

/// Helper to safely convert C string pointer to Rust String
unsafe fn c_str_to_string(ptr: *mut i8) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        let c_str = CStr::from_ptr(ptr);
        c_str.to_str().ok().map(|s| s.to_string())
    }
}

#[test]
fn test_ffi_wallet_get_seed() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_ffi_seed.bin");
    let password = "test_password";

    // Create a normal wallet
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

    let wallet_ptr = &wallet as *const WalletState;

    // Call FFI function
    let seed_ptr = wallet_get_seed(wallet_ptr);
    assert!(!seed_ptr.is_null(), "seed_ptr should not be null for normal wallet");

    // Verify we can read the string
    let seed_str = unsafe { c_str_to_string(seed_ptr) }
        .expect("Failed to convert seed to string");

    // Monero seeds have 25 words
    assert_eq!(seed_str.split_whitespace().count(), 25, "Seed should have 25 words");

    // Verify it's valid English words (just check first word exists)
    let first_word = seed_str.split_whitespace().next().unwrap();
    assert!(!first_word.is_empty(), "First word should not be empty");

    // Free the string
    unsafe { free_string(seed_ptr) };
}

#[test]
fn test_ffi_wallet_get_seed_view_only() {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar, edwards::EdwardsPoint};
    use sha3::{Digest, Keccak256};

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_ffi_view_only.bin");
    let password = "test_password";

    // Create view-only wallet
    let seed = Seed::new(&mut OsRng, Language::English);
    let spend: [u8; 32] = *seed.entropy();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let view: [u8; 32] = Keccak256::digest(&spend).into();

    let wallet = WalletState::new_view_only(
        spend_point.compress().to_bytes(),
        view,
        Network::Mainnet,
        password,
        wallet_path,
        0,
    )
    .expect("Failed to create view-only wallet");

    let wallet_ptr = &wallet as *const WalletState;

    // Call FFI function - should return null for view-only wallet
    let seed_ptr = wallet_get_seed(wallet_ptr);
    assert!(seed_ptr.is_null(), "seed_ptr should be null for view-only wallet");
}

#[test]
fn test_ffi_null_pointer_handling() {
    // All string-returning functions should return null for null wallet pointer
    let null_wallet: *const WalletState = std::ptr::null();

    assert!(wallet_get_seed(null_wallet).is_null());
    assert!(wallet_get_seed_language(null_wallet).is_null());
    assert!(wallet_get_private_spend_key(null_wallet).is_null());
    assert!(wallet_get_private_view_key(null_wallet).is_null());
    assert!(wallet_get_public_spend_key(null_wallet).is_null());
    assert!(wallet_get_public_view_key(null_wallet).is_null());
    assert!(wallet_get_path(null_wallet).is_null());

    // Boolean-returning functions should return -1 for null pointer
    assert_eq!(wallet_is_view_only(null_wallet), -1);
    assert_eq!(wallet_is_closed(null_wallet), -1);
}

#[test]
fn test_ffi_wallet_get_seed_language() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_ffi_lang.bin");
    let password = "test_password";

    let seed = Seed::new(&mut OsRng, Language::Spanish);
    let wallet = WalletState::new(
        seed,
        String::from("Spanish"),
        Network::Mainnet,
        password,
        wallet_path,
        0,
    )
    .expect("Failed to create wallet");

    let wallet_ptr = &wallet as *const WalletState;
    let lang_ptr = wallet_get_seed_language(wallet_ptr);
    assert!(!lang_ptr.is_null());

    let lang_str = unsafe { c_str_to_string(lang_ptr) }
        .expect("Failed to convert language to string");

    assert_eq!(lang_str, "Spanish");

    unsafe { free_string(lang_ptr) };
}

#[test]
fn test_ffi_wallet_get_private_keys() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_ffi_keys.bin");
    let password = "test_password";

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

    let wallet_ptr = &wallet as *const WalletState;

    // Test private spend key
    let spend_key_ptr = wallet_get_private_spend_key(wallet_ptr);
    assert!(!spend_key_ptr.is_null());
    let spend_key = unsafe { c_str_to_string(spend_key_ptr) }.unwrap();
    assert_eq!(spend_key.len(), 64, "Private spend key should be 64 hex chars");
    assert!(spend_key.chars().all(|c| c.is_ascii_hexdigit()), "Should be valid hex");
    unsafe { free_string(spend_key_ptr) };

    // Test private view key
    let view_key_ptr = wallet_get_private_view_key(wallet_ptr);
    assert!(!view_key_ptr.is_null());
    let view_key = unsafe { c_str_to_string(view_key_ptr) }.unwrap();
    assert_eq!(view_key.len(), 64, "Private view key should be 64 hex chars");
    assert!(view_key.chars().all(|c| c.is_ascii_hexdigit()), "Should be valid hex");
    unsafe { free_string(view_key_ptr) };
}

#[test]
fn test_ffi_wallet_get_public_keys() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_ffi_pub_keys.bin");
    let password = "test_password";

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

    let wallet_ptr = &wallet as *const WalletState;

    // Test public spend key
    let pub_spend_ptr = wallet_get_public_spend_key(wallet_ptr);
    assert!(!pub_spend_ptr.is_null());
    let pub_spend = unsafe { c_str_to_string(pub_spend_ptr) }.unwrap();
    assert_eq!(pub_spend.len(), 64, "Public spend key should be 64 hex chars");
    assert!(pub_spend.chars().all(|c| c.is_ascii_hexdigit()));
    unsafe { free_string(pub_spend_ptr) };

    // Test public view key
    let pub_view_ptr = wallet_get_public_view_key(wallet_ptr);
    assert!(!pub_view_ptr.is_null());
    let pub_view = unsafe { c_str_to_string(pub_view_ptr) }.unwrap();
    assert_eq!(pub_view.len(), 64, "Public view key should be 64 hex chars");
    assert!(pub_view.chars().all(|c| c.is_ascii_hexdigit()));
    unsafe { free_string(pub_view_ptr) };
}

#[test]
fn test_ffi_wallet_get_path() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_ffi_path.bin");
    let password = "test_password";

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

    let wallet_ptr = &wallet as *const WalletState;

    let path_ptr = wallet_get_path(wallet_ptr);
    assert!(!path_ptr.is_null());

    let path_str = unsafe { c_str_to_string(path_ptr) }.unwrap();
    assert_eq!(PathBuf::from(&path_str), wallet_path);

    unsafe { free_string(path_ptr) };
}

#[test]
fn test_ffi_wallet_is_view_only() {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar, edwards::EdwardsPoint};
    use sha3::{Digest, Keccak256};

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let password = "test_password";

    // Create normal wallet
    let wallet_path1 = temp_dir.path().join("normal.bin");
    let seed = Seed::new(&mut OsRng, Language::English);
    let normal_wallet = WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        password,
        wallet_path1,
        0,
    )
    .expect("Failed to create wallet");

    let normal_ptr = &normal_wallet as *const WalletState;
    assert_eq!(wallet_is_view_only(normal_ptr), 0, "Normal wallet should return 0 (false)");

    // Create view-only wallet
    let wallet_path2 = temp_dir.path().join("view_only.bin");
    let seed2 = Seed::new(&mut OsRng, Language::English);
    let spend: [u8; 32] = *seed2.entropy();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let view: [u8; 32] = Keccak256::digest(&spend).into();

    let view_only_wallet = WalletState::new_view_only(
        spend_point.compress().to_bytes(),
        view,
        Network::Mainnet,
        password,
        wallet_path2,
        0,
    )
    .expect("Failed to create view-only wallet");

    let view_only_ptr = &view_only_wallet as *const WalletState;
    assert_eq!(wallet_is_view_only(view_only_ptr), 1, "View-only wallet should return 1 (true)");
}

#[test]
fn test_ffi_wallet_is_closed() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_closed.bin");
    let password = "test_password";

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

    let wallet_ptr = &wallet as *const WalletState;

    // Initially should not be closed
    assert_eq!(wallet_is_closed(wallet_ptr), 0, "Wallet should not be closed initially");

    // Set closed flag
    wallet.is_closed = true;
    assert_eq!(wallet_is_closed(wallet_ptr), 1, "Wallet should be closed after setting flag");
}

#[test]
fn test_ffi_memory_leak_prevention() {
    // This test verifies that calling free_string on a null pointer doesn't crash
    unsafe {
        free_string(std::ptr::null_mut());
    }
    // If we get here without crashing, the test passes
}

#[test]
fn test_ffi_multiple_calls_same_wallet() {
    // Verify that calling getters multiple times on the same wallet works correctly
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let wallet_path = temp_dir.path().join("test_multi.bin");
    let password = "test_password";

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

    let wallet_ptr = &wallet as *const WalletState;

    // Call get_seed multiple times
    let seed1 = wallet_get_seed(wallet_ptr);
    let seed2 = wallet_get_seed(wallet_ptr);

    assert!(!seed1.is_null());
    assert!(!seed2.is_null());

    let str1 = unsafe { c_str_to_string(seed1) }.unwrap();
    let str2 = unsafe { c_str_to_string(seed2) }.unwrap();

    // Should return the same seed
    assert_eq!(str1, str2, "Multiple calls should return same seed");

    unsafe {
        free_string(seed1);
        free_string(seed2);
    }
}
