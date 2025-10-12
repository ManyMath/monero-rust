use monero_rust::{
    free_string, wallet_get_path, wallet_get_private_spend_key, wallet_get_private_view_key,
    wallet_get_public_spend_key, wallet_get_public_view_key, wallet_get_seed,
    wallet_get_seed_language, wallet_is_closed, wallet_is_view_only, wallet_state::WalletState,
    Language, Network,
};
use monero_seed::Seed;
use rand_core::OsRng;
use std::ffi::CStr;
use std::path::PathBuf;
use tempfile::tempdir;

unsafe fn c_str_to_string(ptr: *mut i8) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string())
    }
}

fn create_test_wallet(path: PathBuf) -> WalletState {
    let seed = Seed::new(&mut OsRng, Language::English);
    WalletState::new(
        seed,
        String::from("English"),
        Network::Mainnet,
        "test_password",
        path,
        0,
    )
    .expect("wallet creation failed")
}

fn create_view_only_wallet(path: PathBuf) -> WalletState {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
    use sha3::{Digest, Keccak256};

    let seed = Seed::new(&mut OsRng, Language::English);
    let spend: [u8; 32] = *seed.entropy();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let view: [u8; 32] = Keccak256::digest(&spend).into();

    WalletState::new_view_only(
        spend_point.compress().to_bytes(),
        view,
        Network::Mainnet,
        "test_password",
        path,
        0,
    )
    .expect("view-only wallet creation failed")
}

#[test]
fn test_wallet_get_seed() {
    let temp_dir = tempdir().unwrap();
    let wallet = create_test_wallet(temp_dir.path().join("seed.bin"));
    let wallet_ptr = &wallet as *const WalletState;

    let seed_ptr = wallet_get_seed(wallet_ptr);
    assert!(!seed_ptr.is_null());

    let seed_str = unsafe { c_str_to_string(seed_ptr) }.unwrap();
    assert_eq!(seed_str.split_whitespace().count(), 25);

    free_string(seed_ptr);
}

#[test]
fn test_wallet_get_seed_returns_null_for_view_only() {
    let temp_dir = tempdir().unwrap();
    let wallet = create_view_only_wallet(temp_dir.path().join("view_only.bin"));
    let wallet_ptr = &wallet as *const WalletState;

    let seed_ptr = wallet_get_seed(wallet_ptr);
    assert!(seed_ptr.is_null());
}

#[test]
fn test_null_pointer_handling() {
    let null_wallet: *const WalletState = std::ptr::null();

    assert!(wallet_get_seed(null_wallet).is_null());
    assert!(wallet_get_seed_language(null_wallet).is_null());
    assert!(wallet_get_private_spend_key(null_wallet).is_null());
    assert!(wallet_get_private_view_key(null_wallet).is_null());
    assert!(wallet_get_public_spend_key(null_wallet).is_null());
    assert!(wallet_get_public_view_key(null_wallet).is_null());
    assert!(wallet_get_path(null_wallet).is_null());
    assert_eq!(wallet_is_view_only(null_wallet), -1);
    assert_eq!(wallet_is_closed(null_wallet), -1);
}

#[test]
fn test_wallet_get_seed_language() {
    let temp_dir = tempdir().unwrap();
    let seed = Seed::new(&mut OsRng, Language::Spanish);
    let wallet = WalletState::new(
        seed,
        String::from("Spanish"),
        Network::Mainnet,
        "test_password",
        temp_dir.path().join("lang.bin"),
        0,
    )
    .unwrap();

    let wallet_ptr = &wallet as *const WalletState;
    let lang_ptr = wallet_get_seed_language(wallet_ptr);
    assert!(!lang_ptr.is_null());

    let lang_str = unsafe { c_str_to_string(lang_ptr) }.unwrap();
    assert_eq!(lang_str, "Spanish");

    free_string(lang_ptr);
}

#[test]
fn test_wallet_get_private_keys() {
    let temp_dir = tempdir().unwrap();
    let wallet = create_test_wallet(temp_dir.path().join("keys.bin"));
    let wallet_ptr = &wallet as *const WalletState;

    let spend_ptr = wallet_get_private_spend_key(wallet_ptr);
    assert!(!spend_ptr.is_null());
    let spend_key = unsafe { c_str_to_string(spend_ptr) }.unwrap();
    assert_eq!(spend_key.len(), 64);
    assert!(spend_key.chars().all(|c| c.is_ascii_hexdigit()));
    free_string(spend_ptr);

    let view_ptr = wallet_get_private_view_key(wallet_ptr);
    assert!(!view_ptr.is_null());
    let view_key = unsafe { c_str_to_string(view_ptr) }.unwrap();
    assert_eq!(view_key.len(), 64);
    assert!(view_key.chars().all(|c| c.is_ascii_hexdigit()));
    free_string(view_ptr);
}

#[test]
fn test_wallet_get_public_keys() {
    let temp_dir = tempdir().unwrap();
    let wallet = create_test_wallet(temp_dir.path().join("pub_keys.bin"));
    let wallet_ptr = &wallet as *const WalletState;

    let pub_spend_ptr = wallet_get_public_spend_key(wallet_ptr);
    assert!(!pub_spend_ptr.is_null());
    let pub_spend = unsafe { c_str_to_string(pub_spend_ptr) }.unwrap();
    assert_eq!(pub_spend.len(), 64);
    assert!(pub_spend.chars().all(|c| c.is_ascii_hexdigit()));
    free_string(pub_spend_ptr);

    let pub_view_ptr = wallet_get_public_view_key(wallet_ptr);
    assert!(!pub_view_ptr.is_null());
    let pub_view = unsafe { c_str_to_string(pub_view_ptr) }.unwrap();
    assert_eq!(pub_view.len(), 64);
    assert!(pub_view.chars().all(|c| c.is_ascii_hexdigit()));
    free_string(pub_view_ptr);
}

#[test]
fn test_wallet_get_path() {
    let temp_dir = tempdir().unwrap();
    let wallet_path = temp_dir.path().join("path_test.bin");
    let wallet = create_test_wallet(wallet_path.clone());
    let wallet_ptr = &wallet as *const WalletState;

    let path_ptr = wallet_get_path(wallet_ptr);
    assert!(!path_ptr.is_null());

    let path_str = unsafe { c_str_to_string(path_ptr) }.unwrap();
    assert_eq!(PathBuf::from(&path_str), wallet_path);

    free_string(path_ptr);
}

#[test]
fn test_wallet_is_view_only() {
    let temp_dir = tempdir().unwrap();

    let normal = create_test_wallet(temp_dir.path().join("normal.bin"));
    let normal_ptr = &normal as *const WalletState;
    assert_eq!(wallet_is_view_only(normal_ptr), 0);

    let view_only = create_view_only_wallet(temp_dir.path().join("view_only.bin"));
    let view_only_ptr = &view_only as *const WalletState;
    assert_eq!(wallet_is_view_only(view_only_ptr), 1);
}

#[test]
fn test_wallet_is_closed() {
    let temp_dir = tempdir().unwrap();
    let mut wallet = create_test_wallet(temp_dir.path().join("closed.bin"));
    let wallet_ptr = &wallet as *const WalletState;

    assert_eq!(wallet_is_closed(wallet_ptr), 0);

    wallet.is_closed = true;
    assert_eq!(wallet_is_closed(wallet_ptr), 1);
}

#[test]
fn test_free_string_handles_null() {
    free_string(std::ptr::null_mut());
}

#[test]
fn test_multiple_calls_return_consistent_results() {
    let temp_dir = tempdir().unwrap();
    let wallet = create_test_wallet(temp_dir.path().join("multi.bin"));
    let wallet_ptr = &wallet as *const WalletState;

    let seed1 = wallet_get_seed(wallet_ptr);
    let seed2 = wallet_get_seed(wallet_ptr);

    let str1 = unsafe { c_str_to_string(seed1) }.unwrap();
    let str2 = unsafe { c_str_to_string(seed2) }.unwrap();
    assert_eq!(str1, str2);

    free_string(seed1);
    free_string(seed2);
}
