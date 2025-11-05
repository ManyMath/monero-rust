pub mod crypto;
pub mod decoy_selection;
pub mod fee_calculation;
pub mod input_selection;
pub mod rpc;
pub mod transaction_builder;
pub mod types;
pub mod wallet_state;

use monero_wallet::{
    address::{AddressType, MoneroAddress, SubaddressIndex},
    ViewPair,
};

pub use monero_seed::Language;
use monero_seed::Seed;
pub use wallet_state::{WalletState, ScanMode};
pub use monero_wallet::address::Network;
pub use rpc::{ConnectionConfig, ReconnectionPolicy};
pub use input_selection::{InputSelectionConfig, InputSelectionError, SelectedInputs};
pub use decoy_selection::{DecoySelectionConfig, select_decoys_for_output, select_decoys_for_outputs};
pub use transaction_builder::{PendingTransaction, TransactionConfig, TransactionPriority};
pub use fee_calculation::{WeightEstimator, estimate_fee, estimate_sweep_fee};

use rand_core::OsRng;
use zeroize::{Zeroizing};
use curve25519_dalek::{edwards::EdwardsPoint, scalar::Scalar, constants::ED25519_BASEPOINT_TABLE};
use sha3::{Digest, Keccak256};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{LazyLock, Mutex};
use std::collections::{HashSet, VecDeque};

const MAX_TXS_BATCH: u64 = 10000;
const STRING_REGISTRY_MAX_SIZE: usize = 10000;

#[derive(serde::Serialize)]
struct OutputJson {
    tx_hash: String,
    output_index: u64,
    amount: u64,
    key_image: String,
    subaddress_indices: [u32; 2],
    height: u64,
    unlocked: bool,
    spent: bool,
    frozen: bool,
    payment_id: Option<String>,
}

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
    InvalidResponse(String),
    TxKeyLimitExceeded,
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
            WalletError::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            WalletError::TxKeyLimitExceeded => write!(f, "Transaction key storage limit exceeded"),
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

// Track allocated FFI strings with LRU eviction to prevent unbounded memory growth
struct StringRegistry {
    tracked: HashSet<usize>,
    lru_queue: VecDeque<usize>,
}

impl StringRegistry {
    fn new() -> Self {
        Self {
            tracked: HashSet::new(),
            lru_queue: VecDeque::new(),
        }
    }

    fn insert(&mut self, ptr: usize) {
        if self.tracked.len() >= STRING_REGISTRY_MAX_SIZE {
            if let Some(oldest) = self.lru_queue.pop_front() {
                self.tracked.remove(&oldest);
                // oldest pointer is intentionally leaked - can't safely free
                // since C code may still hold it
            }
        }
        if self.tracked.insert(ptr) {
            self.lru_queue.push_back(ptr);
        }
    }

    fn remove(&mut self, ptr: usize) -> bool {
        if self.tracked.remove(&ptr) {
            if let Some(pos) = self.lru_queue.iter().position(|&p| p == ptr) {
                self.lru_queue.remove(pos);
            }
            true
        } else {
            false
        }
    }
}

static STRING_REGISTRY: LazyLock<Mutex<StringRegistry>> = LazyLock::new(|| {
    Mutex::new(StringRegistry::new())
});

fn to_c_string(s: String) -> *mut c_char {
    let c_string = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
    let ptr = c_string.into_raw();
    if let Ok(mut reg) = STRING_REGISTRY.lock() {
        reg.insert(ptr as usize);
    }
    ptr
}

/// # Safety
/// ptr must be from this library
#[no_mangle]
pub extern "C" fn free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    let mut should_free = false;
    if let Ok(mut reg) = STRING_REGISTRY.lock() {
        if reg.remove(ptr as usize) {
            should_free = true;
        } else {
            eprintln!("free_string: untracked pointer {:p}", ptr);
            return;
        }
    }
    if should_free {
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
        if !unsafe { WalletState::validate_ptr(wallet) } {
            eprintln!("wallet_save: invalid wallet pointer");
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
        if wallet.is_null() || !unsafe { WalletState::validate_ptr(wallet) } {
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
        if wallet.is_null() || !unsafe { WalletState::validate_ptr(wallet) } {
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
        wallet_ref.get_current_syncing_height()
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
        wallet_ref.get_daemon_height()
    }));
    result.unwrap_or(0)
}

/// Returns total balance in piconeros, 0 on null/panic.
#[no_mangle]
pub extern "C" fn wallet_get_balance(wallet: *const WalletState) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return 0;
        }
        let wallet_ref = unsafe { &*wallet };
        wallet_ref.get_balance()
    }));
    result.unwrap_or(0)
}

/// Returns unlocked (spendable) balance in piconeros, 0 on null/panic.
#[no_mangle]
pub extern "C" fn wallet_get_unlocked_balance(wallet: *const WalletState) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return 0;
        }
        let wallet_ref = unsafe { &*wallet };
        wallet_ref.get_unlocked_balance()
    }));
    result.unwrap_or(0)
}

/// Refreshes output unlock status from daemon. Returns 0 on success,
/// -1 null, -2 not connected, -3 closed, -4 error, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_refresh_outputs(wallet: *mut WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &mut *wallet };
        match GLOBAL_RUNTIME.block_on(wallet_ref.refresh_outputs()) {
            Ok(()) => 0,
            Err(WalletError::NotConnected) => -2,
            Err(WalletError::WalletClosed) => -3,
            Err(_) => -4,
        }
    }));
    result.unwrap_or(-5)
}

/// Refreshes transaction confirmation counts from daemon. Returns 0 on success,
/// -1 null, -2 not connected, -3 closed, -4 error, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_refresh_transactions(wallet: *mut WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        let wallet_ref = unsafe { &mut *wallet };
        match GLOBAL_RUNTIME.block_on(wallet_ref.refresh_transactions()) {
            Ok(()) => 0,
            Err(WalletError::NotConnected) => -2,
            Err(WalletError::WalletClosed) => -3,
            Err(_) => -4,
        }
    }));
    result.unwrap_or(-5)
}

/// Returns total output count, 0 on null/panic.
#[no_mangle]
pub extern "C" fn wallet_get_outputs_count(wallet: *const WalletState) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return 0;
        }
        let wallet_ref = unsafe { &*wallet };
        wallet_ref.get_outputs_count() as u64
    }));
    result.unwrap_or(0)
}

/// Returns spent output count, 0 on null/panic.
#[no_mangle]
pub extern "C" fn wallet_get_spent_outputs_count(wallet: *const WalletState) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return 0;
        }
        let wallet_ref = unsafe { &*wallet };
        wallet_ref.get_spent_outputs_count() as u64
    }));
    result.unwrap_or(0)
}

/// Returns transaction count, 0 on null/panic.
#[no_mangle]
pub extern "C" fn wallet_get_transaction_count(wallet: *const WalletState) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return 0;
        }
        let wallet_ref = unsafe { &*wallet };
        wallet_ref.get_transaction_count() as u64
    }));
    result.unwrap_or(0)
}

/// Returns JSON for a single transaction, or null if not found.
/// Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_tx(wallet: *const WalletState, txid: *const u8) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() || txid.is_null() {
            return std::ptr::null_mut();
        }
        if !unsafe { WalletState::validate_ptr(wallet) } {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };
        let txid_arr: [u8; 32] = unsafe {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(std::slice::from_raw_parts(txid, 32));
            arr
        };

        match wallet_ref.get_tx(&txid_arr) {
            Some(tx) => match serde_json::to_string(tx) {
                Ok(json) => to_c_string(json),
                Err(_) => std::ptr::null_mut(),
            },
            None => std::ptr::null_mut(),
        }
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// Returns JSON array of transactions. Null entries for missing txids.
/// Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_txs(
    wallet: *const WalletState,
    txids: *const u8,
    count: u64,
) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() || txids.is_null() || count == 0 {
            return std::ptr::null_mut();
        }
        if !unsafe { WalletState::validate_ptr(wallet) } {
            return std::ptr::null_mut();
        }

        if count > MAX_TXS_BATCH {
            eprintln!("[ERROR] wallet_get_txs - count {} exceeds max {}", count, MAX_TXS_BATCH);
            return std::ptr::null_mut();
        }

        let count_usize = match usize::try_from(count) {
            Ok(c) => c,
            Err(_) => return std::ptr::null_mut(),
        };
        let total_bytes = match count_usize.checked_mul(32) {
            Some(b) => b,
            None => return std::ptr::null_mut(),
        };

        let wallet_ref = unsafe { &*wallet };
        let txid_vec: Vec<[u8; 32]> = unsafe {
            std::slice::from_raw_parts(txids, total_bytes)
                .chunks_exact(32)
                .map(|chunk| {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(chunk);
                    arr
                })
                .collect()
        };

        let results = wallet_ref.get_txs(&txid_vec);
        let json_values: Vec<serde_json::Value> = results
            .iter()
            .map(|tx_opt| match tx_opt {
                Some(tx) => serde_json::to_value(tx).unwrap_or(serde_json::Value::Null),
                None => serde_json::Value::Null,
            })
            .collect();

        match serde_json::to_string(&json_values) {
            Ok(json) => to_c_string(json),
            Err(_) => std::ptr::null_mut(),
        }
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// Returns JSON array of all transactions. Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_all_txs(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        if !unsafe { WalletState::validate_ptr(wallet) } {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };
        let all_txs = wallet_ref.get_all_txs();

        match serde_json::to_string(&all_txs) {
            Ok(json) => to_c_string(json),
            Err(_) => std::ptr::null_mut(),
        }
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// Returns JSON array of all txids as hex strings. Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_all_txids(wallet: *const WalletState) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }
        if !unsafe { WalletState::validate_ptr(wallet) } {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };
        let txids: Vec<String> = wallet_ref.get_all_txids().iter().map(hex::encode).collect();

        match serde_json::to_string(&txids) {
            Ok(json) => to_c_string(json),
            Err(_) => std::ptr::null_mut(),
        }
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// Returns tx private key as hex string for an outgoing transaction, or null if not found.
/// Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_tx_key(wallet: *const WalletState, txid: *const u8) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() || txid.is_null() {
            return std::ptr::null_mut();
        }
        if !unsafe { WalletState::validate_ptr(wallet) } {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };
        let txid_arr: [u8; 32] = unsafe {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(std::slice::from_raw_parts(txid, 32));
            arr
        };

        match wallet_ref.get_tx_key(&txid_arr) {
            Some(tx_key) => to_c_string(hex::encode(*tx_key.tx_private_key)),
            None => std::ptr::null_mut(),
        }
    }))
    .unwrap_or(std::ptr::null_mut())
}

/// Stores a tx key for an outgoing transaction.
/// Returns 0 on success, -1 null wallet, -2 null txid, -3 null key,
/// -4 invalid additional_keys, -5 closed, -6 limit exceeded, -7 panic.
#[no_mangle]
pub extern "C" fn wallet_store_tx_key(
    wallet: *mut WalletState,
    txid: *const u8,
    tx_private_key: *const u8,
    additional_keys: *const u8,
    additional_keys_count: u64,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        if !unsafe { WalletState::validate_ptr(wallet) } {
            return -1;
        }
        if txid.is_null() {
            return -2;
        }
        if tx_private_key.is_null() {
            return -3;
        }
        if additional_keys_count > 0 && additional_keys.is_null() {
            return -4;
        }

        let wallet_ref = unsafe { &mut *wallet };
        if wallet_ref.is_closed {
            return -5;
        }

        let txid_arr: [u8; 32] = unsafe {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(std::slice::from_raw_parts(txid, 32));
            arr
        };

        let key_arr: [u8; 32] = unsafe {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(std::slice::from_raw_parts(tx_private_key, 32));
            arr
        };

        let mut tx_key = crate::types::TxKey::new(txid_arr, key_arr);

        // Parse additional keys if present
        if additional_keys_count > 0 {
            let count = match usize::try_from(additional_keys_count) {
                Ok(c) => c,
                Err(_) => return -4,
            };
            let total_bytes = match count.checked_mul(32) {
                Some(b) => b,
                None => return -4,
            };

            unsafe {
                let slice = std::slice::from_raw_parts(additional_keys, total_bytes);
                for chunk in slice.chunks_exact(32) {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(chunk);
                    tx_key.add_additional_key(key);
                }
            }
        }

        match wallet_ref.store_tx_key(txid_arr, tx_key) {
            Ok(()) => 0,
            Err(WalletError::WalletClosed) => -5,
            Err(WalletError::TxKeyLimitExceeded) => -6,
            Err(_) => -6,
        }
    }));

    result.unwrap_or(-7)
}

/// Returns outputs as JSON. Set include_spent=1 to include spent outputs.
/// Set refresh=1 to refresh from daemon first.
/// Returns null on error. Caller must free with free_string().
#[no_mangle]
pub extern "C" fn wallet_get_outputs(
    wallet: *mut WalletState,
    include_spent: i32,
    refresh: i32,
) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() || !unsafe { WalletState::validate_ptr(wallet) } {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &mut *wallet };
        if wallet_ref.is_closed {
            return std::ptr::null_mut();
        }

        if refresh != 0 {
            if let Err(e) = GLOBAL_RUNTIME.block_on(wallet_ref.refresh_outputs()) {
                eprintln!("[ERROR] wallet_get_outputs - refresh failed: {}", e);
                return std::ptr::null_mut();
            }
        }

        let outputs = match wallet_ref.get_outputs(include_spent != 0) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("[ERROR] wallet_get_outputs - {}", e);
                return std::ptr::null_mut();
            }
        };

        let json_outputs: Vec<OutputJson> = outputs
            .iter()
            .map(|o| OutputJson {
                tx_hash: hex::encode(o.tx_hash),
                output_index: o.output_index,
                amount: o.amount,
                key_image: hex::encode(o.key_image),
                subaddress_indices: [o.subaddress_indices.0, o.subaddress_indices.1],
                height: o.height,
                unlocked: o.unlocked,
                spent: o.spent,
                frozen: o.frozen,
                payment_id: o.payment_id.as_ref().map(hex::encode),
            })
            .collect();

        match serde_json::to_string(&json_outputs) {
            Ok(json) => to_c_string(json),
            Err(_) => std::ptr::null_mut(),
        }
    }));

    result.unwrap_or(std::ptr::null_mut())
}

/// Freeze an output by key image. Returns 0 on success,
/// -1 bad wallet, -2 bad key_image, -3 closed, -4 not found, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_freeze_output(wallet: *mut WalletState, key_image: *const u8) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() || !unsafe { WalletState::validate_ptr(wallet) } {
            return -1;
        }
        if key_image.is_null() {
            return -2;
        }

        let wallet_ref = unsafe { &mut *wallet };
        if wallet_ref.is_closed {
            return -3;
        }

        let ki: [u8; 32] = unsafe {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(std::slice::from_raw_parts(key_image, 32));
            arr
        };

        match wallet_ref.freeze_output(&ki) {
            Ok(()) => 0,
            Err(WalletError::WalletClosed) => -3,
            Err(WalletError::Other(msg)) if msg.contains("not found") => -4,
            Err(_) => -4,
        }
    }));

    result.unwrap_or(-5)
}

/// Thaw (unfreeze) an output by key image. Returns 0 on success,
/// -1 bad wallet, -2 bad key_image, -3 closed, -4 not found, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_thaw_output(wallet: *mut WalletState, key_image: *const u8) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() || !unsafe { WalletState::validate_ptr(wallet) } {
            return -1;
        }
        if key_image.is_null() {
            return -2;
        }

        let wallet_ref = unsafe { &mut *wallet };
        if wallet_ref.is_closed {
            return -3;
        }

        let ki: [u8; 32] = unsafe {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(std::slice::from_raw_parts(key_image, 32));
            arr
        };

        match wallet_ref.thaw_output(&ki) {
            Ok(()) => 0,
            Err(WalletError::WalletClosed) => -3,
            Err(WalletError::Other(msg)) if msg.contains("not found") => -4,
            Err(_) => -4,
        }
    }));

    result.unwrap_or(-5)
}

/// Export key images to file. Returns count on success,
/// -1 bad wallet, -2 bad filename, -3 closed, -4 export failed, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_export_key_images(
    wallet: *const WalletState,
    filename: *const c_char,
    all: i32,
) -> i64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() || !unsafe { WalletState::validate_ptr(wallet) } {
            return -1;
        }
        if filename.is_null() {
            return -2;
        }

        let filename_str = unsafe {
            match CStr::from_ptr(filename).to_str() {
                Ok(s) => s,
                Err(_) => return -2,
            }
        };

        if filename_str.contains("..") {
            return -2;
        }

        let wallet_ref = unsafe { &*wallet };
        if wallet_ref.is_closed {
            return -3;
        }

        match wallet_ref.export_key_images(filename_str, all != 0) {
            Ok(count) => count as i64,
            Err(WalletError::WalletClosed) => -3,
            Err(_) => -4,
        }
    }));

    result.unwrap_or(-5)
}

/// Import key images from file. Returns (newly_spent << 32) | already_spent on success,
/// -1 bad wallet, -2 bad filename, -3 closed, -4 import failed, -5 panic.
#[no_mangle]
pub extern "C" fn wallet_import_key_images(wallet: *mut WalletState, filename: *const c_char) -> i64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() || !unsafe { WalletState::validate_ptr(wallet) } {
            return -1;
        }
        if filename.is_null() {
            return -2;
        }

        let filename_str = unsafe {
            match CStr::from_ptr(filename).to_str() {
                Ok(s) => s,
                Err(_) => return -2,
            }
        };

        if filename_str.contains("..") {
            return -2;
        }

        let wallet_ref = unsafe { &mut *wallet };
        if wallet_ref.is_closed {
            return -3;
        }

        match wallet_ref.import_key_images(filename_str) {
            Ok((spent, unspent)) => {
                let spent_u32 = spent.min(u32::MAX as usize) as u32;
                let unspent_u32 = unspent.min(u32::MAX as usize) as u32;
                ((spent_u32 as i64) << 32) | (unspent_u32 as i64)
            }
            Err(WalletError::WalletClosed) => -3,
            Err(_) => -4,
        }
    }));

    result.unwrap_or(-5)
}

/// Estimate tx fee for given priority and amount.
/// Returns fee in piconeros or negative error code.
#[no_mangle]
pub extern "C" fn wallet_estimate_fee(
    wallet: *const WalletState,
    priority: u8,
    amount: u64,
) -> i64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }
        if !unsafe { WalletState::validate_ptr(wallet) } {
            return -1;
        }

        let wallet_ref = unsafe { &*wallet };

        if wallet_ref.is_closed {
            return -2;
        }
        if !wallet_ref.is_connected {
            return -3;
        }

        let tx_priority = match TransactionPriority::from_u8(priority) {
            Ok(p) => p,
            Err(_) => return -4,
        };

        match GLOBAL_RUNTIME.block_on(wallet_ref.estimate_fee(tx_priority, amount)) {
            Ok(fee) => i64::try_from(fee).unwrap_or(i64::MAX),
            Err(WalletError::WalletClosed) => -2,
            Err(WalletError::NotConnected) => -3,
            Err(_) => -5,
        }
    }));

    result.unwrap_or(-6)
}
