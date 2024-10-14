use monero_rust::{MoneroWallet, Language, Network};

#[test]
fn test_integration_wallet_creation() {
    let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
    let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).expect("Failed to create wallet");
    assert_eq!(wallet.get_seed(), mnemonic);
}

#[test]
fn test_integration_get_primary_address() {
    let mnemonic = MoneroWallet::generate_mnemonic(Language::English);
    let wallet = MoneroWallet::new(&mnemonic, Network::Mainnet).expect("Failed to create wallet");
    let address = wallet.get_primary_address();
    assert!(!address.is_empty());
}