use monero_rust::{Language, MoneroWallet, Network};

#[test]
fn test_wallet_creation() {
    let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
    let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).unwrap();
    assert_eq!(wallet.get_seed(), mnemonic);
}

#[test]
fn test_get_primary_address() {
    let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
    let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).unwrap();
    assert!(!wallet.get_primary_address().is_empty());
}

#[test]
fn test_get_subaddress() {
    let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
    let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).unwrap();
    let subaddress = wallet.get_subaddress(0, 1).unwrap();
    assert!(!subaddress.is_empty());
}

#[test]
fn test_keys() {
    let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
    let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).unwrap();
    assert!(!wallet.get_private_spend_key().is_empty());
    assert!(!wallet.get_private_view_key().is_empty());
    assert!(!wallet.get_public_spend_key().is_empty());
    assert!(!wallet.get_public_view_key().is_empty());
}
