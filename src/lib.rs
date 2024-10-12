use monero_serai_mirror::{
    wallet::{
        seed::Seed,
        address::{AddressType, AddressMeta, AddressSpec, MoneroAddress, SubaddressIndex},
        ViewPair,
    },
};

// Re-export for tests.
pub use monero_serai_mirror::wallet::seed::Language;
pub use monero_serai_mirror::wallet::address::Network;

use rand_core::OsRng;
use zeroize::{Zeroizing};
use curve25519_dalek::{
    edwards::EdwardsPoint,
    scalar::Scalar,
    constants::ED25519_BASEPOINT_TABLE,
};
use sha3::{Digest, Keccak256};

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
        let seed = Seed::from_string(Zeroizing::new(mnemonic.to_string())).map_err(|_| "Invalid mnemonic".to_string())?;
        let spend: [u8; 32] = *seed.entropy();
        let spend_scalar: Scalar = Scalar::from_bytes_mod_order(spend);
        let spend_point: EdwardsPoint = &spend_scalar * &ED25519_BASEPOINT_TABLE;
        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar: Scalar = Scalar::from_bytes_mod_order(view);
        let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));

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
        Seed::to_string(&Seed::new(&mut OsRng, language)).to_string()
    }

    /// Returns the mnemonic seed of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the mnemonic seed.
    pub fn get_seed(&self) -> String {
        Seed::to_string(&self.seed).to_string()
    }

    /// Returns the primary address of the wallet.
    ///
    /// # Returns
    ///
    /// A `String` representing the primary address.
    pub fn get_primary_address(&self) -> String {
        let spend_point = &self.view_pair.spend();
        let view_point = &self.view_pair.view();
        let address = MoneroAddress::new(
            AddressMeta::new(self.network, AddressType::Standard),
            *spend_point,
            *view_point,
        );
        address.to_string()
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
        let address = self.view_pair.address(self.network, AddressSpec::Subaddress(subaddress_index));
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
