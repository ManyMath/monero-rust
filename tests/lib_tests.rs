use monero_rust::{MoneroWallet, Network, Language};

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
