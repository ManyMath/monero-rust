pub mod crypto;
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

use rand_core::OsRng;
use zeroize::{Zeroizing};
use curve25519_dalek::{edwards::EdwardsPoint, scalar::Scalar, constants::ED25519_BASEPOINT_TABLE};
use sha3::{Digest, Keccak256};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

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
