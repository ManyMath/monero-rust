use monero_wallet::{
    address::{AddressType, MoneroAddress, SubaddressIndex},
    ViewPair,
};

// Transitional mnemonic support via serai (feature-gated).
#[cfg(feature = "serai-seed")]
pub use monero_serai_mirror::wallet::seed::Language;
#[cfg(feature = "serai-seed")]
use monero_serai_mirror::wallet::seed::Seed;

// Re-export Network for external users/tests.
pub use monero_wallet::address::Network;

use rand_core::OsRng;
use zeroize::{Zeroizing};
use curve25519_dalek::{edwards::EdwardsPoint, scalar::Scalar, constants::ED25519_BASEPOINT_TABLE};
use sha3::{Digest, Keccak256};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

pub struct MoneroWallet {
    #[cfg(feature = "serai-seed")]
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
        // Derive spend/view from mnemonic via serai's Seed (temporary).
        //
        // TODO: Remove all monero-serai and monero-serai-mirror usages.
        #[cfg(feature = "serai-seed")]
        let seed = Seed::from_string(Zeroizing::new(mnemonic.to_string()))
            .map_err(|_| "Invalid mnemonic".to_string())?;
        #[cfg(feature = "serai-seed")]
        let spend: [u8; 32] = *seed.entropy();

        #[cfg(not(feature = "serai-seed"))]
        return Err("Mnemonic support disabled (no seed backend)".into());

        let spend_scalar: Scalar = Scalar::from_bytes_mod_order(spend);
        let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar: Scalar = Scalar::from_bytes_mod_order(view);
        let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar))
            .map_err(|e| e.to_string())?;

        Ok(MoneroWallet {
            #[cfg(feature = "serai-seed")]
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
    #[cfg(feature = "serai-seed")]
    pub fn generate_mnemonic(language: Language) -> String {
        Seed::to_string(&Seed::new(&mut OsRng, language)).to_string()
    }

    /// Returns the mnemonic seed of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the mnemonic seed.
    #[cfg(feature = "serai-seed")]
    pub fn get_seed(&self) -> String {
        Seed::to_string(&self.seed).to_string()
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
        #[cfg(feature = "serai-seed")]
        { hex::encode(self.seed.entropy()) }
        #[cfg(not(feature = "serai-seed"))]
        { String::from("") }
    }

    /// Returns the private view key of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the private view key in hexadecimal format.
    pub fn get_private_view_key(&self) -> String {
        #[cfg(feature = "serai-seed")]
        {
            let view: [u8; 32] = Keccak256::digest(self.seed.entropy()).into();
            hex::encode(view)
        }
        #[cfg(not(feature = "serai-seed"))]
        { String::from("") }
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

#[no_mangle]
pub extern "C" fn generate_mnemonic(language: u8) -> *mut c_char {
    #[cfg(feature = "serai-seed")]
    {
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
            12 => Language::EnglishOld,
            _ => Language::English,
        };
        to_c_string(MoneroWallet::generate_mnemonic(lang))
    }
    #[cfg(not(feature = "serai-seed"))]
    {
        to_c_string(String::new())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use monero_serai_mirror::wallet::seed::Language;

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
