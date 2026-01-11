// Module declarations
pub mod crypto;
pub mod rpc;
pub mod types;
pub mod wallet_state;

use monero_wallet::{
    address::{AddressType, MoneroAddress, SubaddressIndex},
    ViewPair,
};

// Mnemonic support via monero-seed.
pub use monero_seed::Language;
use monero_seed::Seed;

// Re-export WalletState for external use
pub use wallet_state::WalletState;

// Re-export Network for external users/tests.
pub use monero_wallet::address::Network;

// Re-export RPC types for external use
pub use rpc::{ConnectionConfig, ReconnectionPolicy};

use rand_core::OsRng;
use zeroize::{Zeroizing};
use curve25519_dalek::{edwards::EdwardsPoint, scalar::Scalar, constants::ED25519_BASEPOINT_TABLE};
use sha3::{Digest, Keccak256};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Error type for wallet operations.
///
/// This enum covers all possible errors that can occur during wallet
/// operations including file I/O, encryption, serialization, and validation.
#[derive(Debug)]
pub enum WalletError {
    /// I/O error occurred (file read/write)
    IoError(std::io::Error),

    /// Encryption or decryption failed
    EncryptionError(String),

    /// Invalid password provided
    InvalidPassword,

    /// Wallet file is corrupted or invalid
    CorruptedFile(String),

    /// Wallet file version is not supported
    UnsupportedVersion(u32),

    /// Serialization or deserialization failed
    SerializationError(String),

    /// Wallet is closed and cannot be used
    WalletClosed,

    /// RPC error occurred (daemon communication)
    RpcError(monero_wallet::rpc::RpcError),

    /// Daemon is not connected
    NotConnected,

    /// Generic error with message
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
            WalletError::RpcError(e) => write!(f, "RPC error: {}", e),
            WalletError::NotConnected => write!(f, "Daemon is not connected"),
            WalletError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for WalletError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WalletError::IoError(e) => Some(e),
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

impl From<monero_wallet::rpc::RpcError> for WalletError {
    fn from(err: monero_wallet::rpc::RpcError) -> Self {
        WalletError::RpcError(err)
    }
}

/// Helper function to parse a mnemonic seed by trying all supported languages.
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
    /// Creates a new MoneroWallet from a mnemonic and network type.
    ///
    /// # Arguments
    ///
    /// * `mnemonic` - A string slice that holds the mnemonic seed phrase.
    /// * `network` - The Monero network type (Mainnet, Testnet, or Stagenet).
    ///
    /// # Errors
    ///
    /// Returns an error if the mnemonic is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use monero_rust::{MoneroWallet, Language, Network};
    /// let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
    /// let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).unwrap();
    /// ```
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

    /// Generates a new mnemonic seed in the specified language.
    ///
    /// # Arguments
    ///
    /// * `language` - The language for the mnemonic seed.
    ///
    /// # Returns
    ///
    /// A `String` representing the mnemonic seed.
    ///
    /// # Example
    ///
    /// ```
    /// use monero_rust::{MoneroWallet, Language};
    /// let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
    /// ```
    pub fn generate_mnemonic(language: Language) -> String {
        Seed::new(&mut OsRng, language).to_string().to_string()
    }

    /// Returns the mnemonic seed of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the mnemonic seed.
    pub fn get_seed(&self) -> String {
        self.seed.to_string().to_string()
    }

    /// Returns the primary address of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the primary address.
    pub fn get_primary_address(&self) -> String {
        let spend_point = self.view_pair.spend();
        let view_point = self.view_pair.view();
        MoneroAddress::new(self.network, AddressType::Legacy, spend_point, view_point).to_string()
    }

    /// Returns the subaddress of the wallet for the given account and index.
    ///
    /// # Arguments
    ///
    /// * `account` - The account index.
    /// * `index` - The subaddress index.
    ///
    /// # Errors
    ///
    /// Returns an error if the subaddress index is invalid.
    pub fn get_subaddress(&self, account: u32, index: u32) -> Result<String, String> {
        let subaddress_index = SubaddressIndex::new(account, index).ok_or("Invalid subaddress index".to_string())?;
        let address = self.view_pair.subaddress(self.network, subaddress_index);
        Ok(address.to_string())
    }

    /// Returns the private spend key of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the private spend key in hexadecimal format.
    pub fn get_private_spend_key(&self) -> String {
        hex::encode(self.seed.entropy())
    }

    /// Returns the private view key of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the private view key in hexadecimal format.
    pub fn get_private_view_key(&self) -> String {
        let view: [u8; 32] = Keccak256::digest(self.seed.entropy()).into();
        hex::encode(view)
    }

    /// Returns the public spend key of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the public spend key in hexadecimal format.
    pub fn get_public_spend_key(&self) -> String {
        let spend_point = &self.view_pair.spend();
        hex::encode(spend_point.compress().to_bytes())
    }

    /// Returns the public view key of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the public view key in hexadecimal format.
    pub fn get_public_view_key(&self) -> String {
        let view_point = &self.view_pair.view();
        hex::encode(view_point.compress().to_bytes())
    }
}

// C FFI helpers
fn to_c_string(s: String) -> *mut c_char {
    CString::new(s).unwrap_or_else(|_| CString::new("").unwrap()).into_raw()
}

/// Frees a C string allocated by this library.
///
/// # Safety
/// Must only be called on strings allocated by this library's functions.
/// Must not be called more than once on the same pointer.
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
    // Mapping expected by Dart code/tests: 0=German, 1=English, 2=Spanish, ... , 12=Old English.
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
    // Safety: assumes mnemonic is a valid, null-terminated UTF-8 C string.
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

// ========================================================================
// FFI BINDINGS FOR WALLET FILE I/O
// ========================================================================

/// Saves a WalletState to a file with password encryption.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
/// * `password` - C string containing the password
///
/// # Returns
/// * 0 on success
/// * -1 if wallet pointer is null
/// * -2 if password is null or invalid UTF-8
/// * -3 if wallet is closed
/// * -4 if save operation failed
///
/// # Safety
/// The wallet pointer must be valid and point to a WalletState instance.
/// The password must be a valid, null-terminated UTF-8 C string.
#[no_mangle]
pub extern "C" fn wallet_save(
    wallet: *const WalletState,
    password: *const c_char,
) -> i32 {
    // Catch panics to prevent undefined behavior across FFI boundary
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Validate wallet pointer
        if wallet.is_null() {
            return -1;
        }

        // Validate and convert password
        if password.is_null() {
            return -2;
        }

        let password_str = unsafe {
            match CStr::from_ptr(password).to_str() {
                Ok(s) => s,
                Err(_) => return -2,
            }
        };

        // Get wallet reference
        let wallet_ref = unsafe { &*wallet };

        // Attempt to save
        match wallet_ref.save(password_str) {
            Ok(()) => 0,
            Err(WalletError::WalletClosed) => -3,
            Err(WalletError::InvalidPassword) => -2,  // Wrong password
            Err(_) => -4,
        }
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            eprintln!("PANIC in wallet_save - this should never happen");
            -5  // Panic error code
        }
    }
}

/// Loads a WalletState from a file with password decryption.
///
/// # Arguments
/// * `path` - C string containing the file path
/// * `password` - C string containing the password
///
/// # Returns
/// * Pointer to WalletState on success
/// * null pointer on failure
///
/// # Safety
/// The returned pointer must be freed using `wallet_free()`.
/// The path and password must be valid, null-terminated UTF-8 C strings.
#[no_mangle]
pub extern "C" fn wallet_load(
    path: *const c_char,
    password: *const c_char,
) -> *mut WalletState {
    // Catch panics to prevent undefined behavior across FFI boundary
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Validate path
        if path.is_null() {
            return std::ptr::null_mut();
        }

        // Validate password
        if password.is_null() {
            return std::ptr::null_mut();
        }

        // Convert C strings to Rust strings
        let (path_str, password_str) = unsafe {
            let path_result = CStr::from_ptr(path).to_str();
            let password_result = CStr::from_ptr(password).to_str();

            match (path_result, password_result) {
                (Ok(p), Ok(pw)) => (p, pw),
                _ => return std::ptr::null_mut(),
            }
        };

        // Attempt to load wallet
        match WalletState::load_from_file(path_str, password_str) {
            Ok(wallet) => Box::into_raw(Box::new(wallet)),
            Err(_) => std::ptr::null_mut(),
        }
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            eprintln!("PANIC in wallet_load - this should never happen");
            std::ptr::null_mut()
        }
    }
}

/// Frees a WalletState allocated by this library.
///
/// # Safety
/// Must only be called on wallets allocated by this library's functions (e.g., wallet_load).
/// Must not be called more than once on the same pointer.
#[no_mangle]
pub extern "C" fn wallet_free(wallet: *mut WalletState) {
    if !wallet.is_null() {
        unsafe {
            let _ = Box::from_raw(wallet);
        }
    }
}

// ==================== WALLET GETTERS FFI ====================

/// Gets the wallet's mnemonic seed.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * C string containing the mnemonic seed for normal wallets
/// * null pointer for view-only wallets or on error
///
/// # Safety
/// The returned string must be freed using `free_string()`.
#[no_mangle]
pub extern "C" fn wallet_get_seed(wallet: *const WalletState) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };

        match wallet_ref.get_seed() {
            Some(seed) => to_c_string(seed),
            None => std::ptr::null_mut(),
        }
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            eprintln!("PANIC in wallet_get_seed");
            std::ptr::null_mut()
        }
    }
}

/// Gets the language of the wallet's mnemonic seed.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * C string containing the seed language (e.g., "English")
/// * null pointer on error
///
/// # Safety
/// The returned string must be freed using `free_string()`.
#[no_mangle]
pub extern "C" fn wallet_get_seed_language(wallet: *const WalletState) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };
        to_c_string(wallet_ref.get_seed_language().to_string())
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            eprintln!("PANIC in wallet_get_seed_language");
            std::ptr::null_mut()
        }
    }
}

/// Gets the wallet's private spend key.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * C string containing the hex-encoded private spend key for normal wallets
/// * null pointer for view-only wallets or on error
///
/// # Safety
/// The returned string must be freed using `free_string()`.
#[no_mangle]
pub extern "C" fn wallet_get_private_spend_key(wallet: *const WalletState) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };

        match wallet_ref.get_private_spend_key() {
            Some(key) => to_c_string(key),
            None => std::ptr::null_mut(),
        }
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            eprintln!("PANIC in wallet_get_private_spend_key");
            std::ptr::null_mut()
        }
    }
}

/// Gets the wallet's private view key.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * C string containing the hex-encoded private view key
/// * null pointer on error
///
/// # Safety
/// The returned string must be freed using `free_string()`.
#[no_mangle]
pub extern "C" fn wallet_get_private_view_key(wallet: *const WalletState) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };
        to_c_string(wallet_ref.get_private_view_key())
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            eprintln!("PANIC in wallet_get_private_view_key");
            std::ptr::null_mut()
        }
    }
}

/// Gets the wallet's public spend key.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * C string containing the hex-encoded public spend key
/// * null pointer on error
///
/// # Safety
/// The returned string must be freed using `free_string()`.
#[no_mangle]
pub extern "C" fn wallet_get_public_spend_key(wallet: *const WalletState) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };
        to_c_string(wallet_ref.get_public_spend_key())
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            eprintln!("PANIC in wallet_get_public_spend_key");
            std::ptr::null_mut()
        }
    }
}

/// Gets the wallet's public view key.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * C string containing the hex-encoded public view key
/// * null pointer on error
///
/// # Safety
/// The returned string must be freed using `free_string()`.
#[no_mangle]
pub extern "C" fn wallet_get_public_view_key(wallet: *const WalletState) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };
        to_c_string(wallet_ref.get_public_view_key())
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            eprintln!("PANIC in wallet_get_public_view_key");
            std::ptr::null_mut()
        }
    }
}

/// Gets the filesystem path where the wallet is stored.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * C string containing the wallet file path (UTF-8 encoded)
/// * null pointer on error or if the path contains invalid UTF-8
///
/// # Safety
/// The returned string must be freed using `free_string()`.
///
/// # Note
/// Returns null if the path is not valid UTF-8. On Unix systems, paths can
/// contain arbitrary bytes, so callers should handle this case appropriately.
#[no_mangle]
pub extern "C" fn wallet_get_path(wallet: *const WalletState) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return std::ptr::null_mut();
        }

        let wallet_ref = unsafe { &*wallet };

        // Try to convert path to UTF-8 string
        // Return null if the path contains invalid UTF-8 (rather than silently corrupting it)
        match wallet_ref.get_path().to_str() {
            Some(path_str) => to_c_string(path_str.to_string()),
            None => {
                eprintln!("WARN: wallet_get_path called on wallet with non-UTF-8 path");
                std::ptr::null_mut()
            }
        }
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_) => {
            eprintln!("PANIC in wallet_get_path");
            std::ptr::null_mut()
        }
    }
}

/// Checks if the wallet is view-only (no spend key).
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * 1 if view-only wallet
/// * 0 if normal wallet (has spend key)
/// * -1 on error (null pointer)
/// * -5 on panic
#[no_mangle]
pub extern "C" fn wallet_is_view_only(wallet: *const WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }

        let wallet_ref = unsafe { &*wallet };
        if wallet_ref.is_view_only() { 1 } else { 0 }
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            eprintln!("PANIC in wallet_is_view_only");
            -5
        }
    }
}

/// Checks if the wallet is closed.
///
/// # Arguments
/// * `wallet` - Pointer to WalletState
///
/// # Returns
/// * 1 if wallet is closed
/// * 0 if wallet is open
/// * -1 on error (null pointer)
/// * -5 on panic
#[no_mangle]
pub extern "C" fn wallet_is_closed(wallet: *const WalletState) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if wallet.is_null() {
            return -1;
        }

        let wallet_ref = unsafe { &*wallet };
        if wallet_ref.is_closed { 1 } else { 0 }
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            eprintln!("PANIC in wallet_is_closed");
            -5
        }
    }
}

// ==================== END WALLET GETTERS FFI ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_creation() {
        let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
        let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).expect("Failed to create wallet");
        assert_eq!(wallet.get_seed(), mnemonic);
        println!("Primary Address: {}", wallet.get_primary_address());
    }

    #[test]
    fn test_get_primary_address() {
        let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
        let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).expect("Failed to create wallet");
        let address = wallet.get_primary_address();
        assert!(!address.is_empty());
    }

    #[test]
    fn test_get_subaddress() {
        let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
        let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).expect("Failed to create wallet");
        let subaddress = wallet.get_subaddress(0, 1).expect("Failed to get subaddress");
        assert!(!subaddress.is_empty());
    }

    #[test]
    fn test_keys() {
        let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
        let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).expect("Failed to create wallet");
        assert!(!wallet.get_private_spend_key().is_empty());
        assert!(!wallet.get_private_view_key().is_empty());
        assert!(!wallet.get_public_spend_key().is_empty());
        assert!(!wallet.get_public_view_key().is_empty());
    }
}
