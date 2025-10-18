pub mod crypto;
pub mod rpc;
pub mod types;
pub mod wallet_state;

use monero_wallet::{
    address::{AddressType, MoneroAddress, SubaddressIndex},
    ViewPair,
};

pub use monero_seed::Language;
use monero_seed::Seed;
pub use wallet_state::WalletState;
pub use monero_wallet::address::Network;
pub use rpc::{ConnectionConfig, ReconnectionPolicy};

use rand_core::OsRng;
use zeroize::{Zeroizing};
use curve25519_dalek::{edwards::EdwardsPoint, scalar::Scalar, constants::ED25519_BASEPOINT_TABLE};
use sha3::{Digest, Keccak256};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::LazyLock;

#[derive(Debug)]
pub enum WalletError {
    IoError(std::io::Error),
    EncryptionError(String),
    InvalidPassword,
    CorruptedFile(String),
    UnsupportedVersion(u32),
    SerializationError(String),
    WalletClosed,
    NotConnected,
    RpcError(monero_rpc::RpcError),
    Other(String),
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalletError::IoError(e) => write!(f, "I/O error: {}", e),
            WalletError::EncryptionError(msg) => write!(f, "Encryption error: {}", msg),
            WalletError::InvalidPassword => write!(f, "Invalid password"),
            WalletError::CorruptedFile(msg) => write!(f, "Corrupted wallet file: {}", msg),
            WalletError::UnsupportedVersion(v) => write!(f, "Unsupported wallet version: {}", v),
            WalletError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            WalletError::WalletClosed => write!(f, "Wallet is closed"),
            WalletError::NotConnected => write!(f, "Not connected to daemon"),
            WalletError::RpcError(e) => write!(f, "RPC error: {}", e),
            WalletError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for WalletError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WalletError::IoError(e) => Some(e),
            WalletError::RpcError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for WalletError {
    fn from(err: std::io::Error) -> Self {
        WalletError::IoError(err)
    }
}

impl From<String> for WalletError {
    fn from(err: String) -> Self {
        WalletError::Other(err)
    }
}

impl From<monero_rpc::RpcError> for WalletError {
    fn from(err: monero_rpc::RpcError) -> Self {
        WalletError::RpcError(err)
    }
}

fn seed_from_string(mnemonic: &str) -> Result<(Language, Seed), String> {
    let languages = [
        Language::English,
        Language::Chinese,
        Language::Dutch,
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
        Language::Portuguese,
        Language::Japanese,
        Language::Russian,
        Language::Esperanto,
        Language::Lojban,
        Language::DeprecatedEnglish,
    ];

    for lang in languages {
        if let Ok(seed) = Seed::from_string(lang, Zeroizing::new(mnemonic.to_string())) {
            return Ok((lang, seed));
        }
    }

    Err("Invalid mnemonic: not valid in any supported language".to_string())
}

pub struct MoneroWallet {
    seed: Seed,
    view_pair: ViewPair,
    network: Network,
}

impl MoneroWallet {
    pub fn new(mnemonic: &str, network: Network) -> Result<Self, String> {
        let (_lang, seed) = seed_from_string(mnemonic)?;
        let spend: [u8; 32] = *seed.entropy();
        let spend_scalar: Scalar = Scalar::from_bytes_mod_order(spend);
        let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar: Scalar = Scalar::from_bytes_mod_order(view);
        let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar))
            .map_err(|e| e.to_string())?;

        Ok(MoneroWallet {
            seed,
            view_pair,
            network,
        })
    }

    pub fn generate_mnemonic(language: Language) -> String {
        Seed::new(&mut OsRng, language).to_string().to_string()
    }

    pub fn get_seed(&self) -> String {
        self.seed.to_string().to_string()
    }

    pub fn get_primary_address(&self) -> String {
        let spend_point = self.view_pair.spend();
        let view_point = self.view_pair.view();
        MoneroAddress::new(self.network, AddressType::Legacy, spend_point, view_point).to_string()
    }

    pub fn get_subaddress(&self, account: u32, index: u32) -> Result<String, String> {
        let subaddress_index = SubaddressIndex::new(account, index).ok_or("Invalid subaddress index".to_string())?;
        let address = self.view_pair.subaddress(self.network, subaddress_index);
        Ok(address.to_string())
    }

    pub fn get_private_spend_key(&self) -> String {
        hex::encode(self.seed.entropy())
    }

    pub fn get_private_view_key(&self) -> String {
        let view: [u8; 32] = Keccak256::digest(self.seed.entropy()).into();
        hex::encode(view)
    }

    pub fn get_public_spend_key(&self) -> String {
        let spend_point = &self.view_pair.spend();
        hex::encode(spend_point.compress().to_bytes())
    }

    pub fn get_public_view_key(&self) -> String {
        let view_point = &self.view_pair.view();
        hex::encode(view_point.compress().to_bytes())
    }
}

fn to_c_string(s: String) -> *mut c_char {
    CString::new(s).unwrap_or_else(|_| CString::new("").unwrap()).into_raw()
}

/// # Safety
/// ptr must be from this library
#[no_mangle]
pub extern "C" fn free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

#[no_mangle]
pub extern "C" fn generate_mnemonic(language: u8) -> *mut c_char {
    let lang = match language {
        0 => Language::German,
        1 => Language::English,
        2 => Language::Spanish,
        3 => Language::French,
        4 => Language::Dutch,
        5 => Language::Italian,
        6 => Language::Portuguese,
        7 => Language::Japanese,
        8 => Language::Russian,
        9 => Language::Esperanto,
        10 => Language::Lojban,
        11 => Language::Chinese,
        12 => Language::DeprecatedEnglish,
        _ => Language::English,
    };
    to_c_string(MoneroWallet::generate_mnemonic(lang))
}

#[no_mangle]
pub extern "C" fn generate_address(
    mnemonic: *const c_char,
    network: u8,
    account: u32,
    index: u32,
) -> *mut c_char {
    if mnemonic.is_null() {
        return to_c_string(String::new());
    }
    let c_str = unsafe { CStr::from_ptr(mnemonic) };
    let mnemonic_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return to_c_string(String::new()),
    };

    let net = match network {
        0 => Network::Mainnet,
        1 => Network::Testnet,
        2 => Network::Stagenet,
        _ => Network::Mainnet,
    };

    let wallet = match MoneroWallet::new(mnemonic_str, net) {
        Ok(w) => w,
        Err(_) => return to_c_string(String::new()),
    };

    if account == 0 && index == 0 {
        return to_c_string(wallet.get_primary_address());
    }

    match wallet.get_subaddress(account, index) {
        Ok(addr) => to_c_string(addr),
        Err(_) => to_c_string(String::new()),
    }
}

/// # Safety
/// Wallet pointer must be valid. Password must be null-terminated UTF-8.
#[no_mangle]
pub extern "C" fn wallet_save(wallet: *const WalletState, password: *const c_char) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        if password.is_null() {
            return -2;
        }

        let password_str = unsafe {
            match CStr::from_ptr(password).to_str() {
                Ok(s) => s,
                Err(_) => return -2,
            }
        };

        let wallet_ref = unsafe { &*wallet };

        match wallet_ref.save(password_str) {
            Ok(()) => 0,
            Err(WalletError::WalletClosed) => -3,
            Err(WalletError::InvalidPassword) => -2,
            Err(_) => -4,
        }
    }));

    result.unwrap_or(-5)
}

/// # Safety
/// Path and password must be null-terminated UTF-8. Caller must free with wallet_free().
#[no_mangle]
pub extern "C" fn wallet_load(path: *const c_char, password: *const c_char) -> *mut WalletState {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if path.is_null() || password.is_null() {
            return std::ptr::null_mut();
        }

        let (path_str, password_str) = unsafe {
            match (CStr::from_ptr(path).to_str(), CStr::from_ptr(password).to_str()) {
                (Ok(p), Ok(pw)) => (p, pw),
                _ => return std::ptr::null_mut(),
            }
        };

        match WalletState::load_from_file(path_str, password_str) {
            Ok(wallet) => Box::into_raw(Box::new(wallet)),
            Err(_) => std::ptr::null_mut(),
        }
    }));

    result.unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// Must only be called once per wallet allocated by wallet_load().
#[no_mangle]
pub extern "C" fn wallet_free(wallet: *mut WalletState) {
    if !wallet.is_null() {
        unsafe {
            let _ = Box::from_raw(wallet);
        }
    }
}

/// # Safety
/// Caller must free with free_string(). Returns null for view-only wallets.
#[no_mangle]
pub extern "C" fn wallet_get_seed(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        let wallet_ref = unsafe { &*wallet };
        match wallet_ref.get_seed() {
            Some(seed) => to_c_string(seed),
            None => std::ptr::null_mut(),
        }
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_get_seed");
        std::ptr::null_mut()
    })
}

/// # Safety
/// Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_seed_language(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        let wallet_ref = unsafe { &*wallet };
        to_c_string(wallet_ref.get_seed_language().to_string())
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_get_seed_language");
        std::ptr::null_mut()
    })
}

/// # Safety
/// Caller must free with free_string(). Returns null for view-only wallets.
#[no_mangle]
pub extern "C" fn wallet_get_private_spend_key(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        let wallet_ref = unsafe { &*wallet };
        match wallet_ref.get_private_spend_key() {
            Some(key) => to_c_string(key),
            None => std::ptr::null_mut(),
        }
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_get_private_spend_key");
        std::ptr::null_mut()
    })
}

/// # Safety
/// Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_private_view_key(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        let wallet_ref = unsafe { &*wallet };
        to_c_string(wallet_ref.get_private_view_key())
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_get_private_view_key");
        std::ptr::null_mut()
    })
}

/// # Safety
/// Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_public_spend_key(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        let wallet_ref = unsafe { &*wallet };
        to_c_string(wallet_ref.get_public_spend_key())
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_get_public_spend_key");
        std::ptr::null_mut()
    })
}

/// # Safety
/// Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_public_view_key(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        let wallet_ref = unsafe { &*wallet };
        to_c_string(wallet_ref.get_public_view_key())
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_get_public_view_key");
        std::ptr::null_mut()
    })
}

/// # Safety
/// Caller must free with free_string(). Returns null if path isn't valid UTF-8.
#[no_mangle]
pub extern "C" fn wallet_get_path(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        let wallet_ref = unsafe { &*wallet };
        match wallet_ref.get_path().to_str() {
            Some(path_str) => to_c_string(path_str.to_string()),
            None => std::ptr::null_mut(),
        }
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_get_path");
        std::ptr::null_mut()
    })
}

/// Returns 1 if view-only, 0 if normal, -1 on null pointer, -5 on panic.
#[no_mangle]
pub extern "C" fn wallet_is_view_only(wallet: *const WalletState) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &*wallet };
        if wallet_ref.is_view_only() { 1 } else { 0 }
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_is_view_only");
        -5
    })
}

/// Returns 1 if closed, 0 if open, -1 on null pointer, -5 on panic.
#[no_mangle]
pub extern "C" fn wallet_is_closed(wallet: *const WalletState) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &*wallet };
        if wallet_ref.is_closed { 1 } else { 0 }
    }))
    .unwrap_or_else(|_| {
        eprintln!("PANIC in wallet_is_closed");
        -5
    })
}

// ==================== SYNC FFI ====================

static GLOBAL_RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Runtime::new().expect("Failed to create tokio runtime")
});

/// Starts syncing. Returns 0 on success, -1 null, -2 not connected, -3 closed, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_start_syncing(wallet: *mut WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &mut *wallet };
        match GLOBAL_RUNTIME.block_on(wallet_ref.start_syncing()) {
            Ok(()) => 0,
            Err(WalletError::NotConnected) => -2,
            Err(WalletError::WalletClosed) => -3,
            Err(_) => -4,
        }
    }));
    result.unwrap_or(-5)
}

/// Stops syncing. Returns 0 on success, -1 null, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_stop_syncing(wallet: *mut WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &mut *wallet };
        GLOBAL_RUNTIME.block_on(wallet_ref.stop_syncing());
        0
    }));
    result.unwrap_or(-5)
}

/// Scans one block. Returns 1 if scanned, 0 if synced, -1 null, -2 not connected, -3 closed, -4 error, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_sync_once(wallet: *mut WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &mut *wallet };
        match GLOBAL_RUNTIME.block_on(wallet_ref.sync_once()) {
            Ok(true) => 1,
            Ok(false) => 0,
            Err(WalletError::NotConnected) => -2,
            Err(WalletError::WalletClosed) => -3,
            Err(_) => -4,
        }
    }));
    result.unwrap_or(-5)
}

/// Returns refresh height, 0 on null/panic.
#[no_mangle]
pub extern "C" fn wallet_get_refresh_from_height(wallet: *const WalletState) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return 0;
        }
        let wallet_ref = unsafe { &*wallet };
        wallet_ref.get_refresh_from_height()
    }));
    result.unwrap_or(0)
}

/// Sets refresh height. Returns 0 on success, -1 null, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_set_refresh_from_height(wallet: *mut WalletState, height: u64) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &mut *wallet };
        wallet_ref.set_refresh_from_height(height);
        0
    }));
    result.unwrap_or(-5)
}

/// Clears outputs/txs and resets to refresh height. Returns 0 on success, -1 null, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_rescan_blockchain(wallet: *mut WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &mut *wallet };
        wallet_ref.rescan_blockchain();
        0
    }));
    result.unwrap_or(-5)
}

/// Returns 1 if syncing, 0 if not, -1 null, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_is_syncing(wallet: *const WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &*wallet };
        if wallet_ref.is_syncing { 1 } else { 0 }
    }));
    result.unwrap_or(-5)
}

/// Returns current scanned height, 0 on null/panic.
#[no_mangle]
pub extern "C" fn wallet_get_current_height(wallet: *const WalletState) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return 0;
        }
        let wallet_ref = unsafe { &*wallet };
        wallet_ref.current_scanned_height
    }));
    result.unwrap_or(0)
}

/// Returns daemon height, 0 on null/panic.
#[no_mangle]
pub extern "C" fn wallet_get_daemon_height(wallet: *const WalletState) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return 0;
        }
        let wallet_ref = unsafe { &*wallet };
        wallet_ref.daemon_height
    }));
    result.unwrap_or(0)
}
