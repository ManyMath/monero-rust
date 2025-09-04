//! Comprehensive tests for account management functionality

use monero_rust::{derive_address, derive_subaddress};

const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";

#[test]
fn test_standard_address_matches_account_0_subaddress_0() {
    // Standard address should equal derive_subaddress(0, 0)
    let standard = derive_address(HONKED_BAGPIPE_MNEMONIC, "stagenet", "")
        .expect("derive_address should succeed");

    let subaddr_0_0 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 0, 0, "")
        .expect("derive_subaddress(0, 0) should succeed");

    assert_eq!(
        standard, subaddr_0_0,
        "Standard address should equal account 0, subaddress 0"
    );
}

#[test]
fn test_multiple_accounts_different_addresses() {
    // Different accounts should produce different addresses
    let account_0 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 0, 0, "")
        .expect("account 0 should derive");

    let account_1 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 1, 0, "")
        .expect("account 1 should derive");

    let account_2 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 2, 0, "")
        .expect("account 2 should derive");

    assert_ne!(account_0, account_1, "Account 0 and 1 should differ");
    assert_ne!(account_1, account_2, "Account 1 and 2 should differ");
    assert_ne!(account_0, account_2, "Account 0 and 2 should differ");
}

#[test]
fn test_multiple_subaddresses_same_account() {
    // Different subaddresses in the same account should differ
    let addr_0 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 1, 0, "")
        .expect("subaddress 0 should derive");

    let addr_1 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 1, 1, "")
        .expect("subaddress 1 should derive");

    let addr_2 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 1, 2, "")
        .expect("subaddress 2 should derive");

    assert_ne!(addr_0, addr_1, "Subaddress 0 and 1 should differ");
    assert_ne!(addr_1, addr_2, "Subaddress 1 and 2 should differ");
    assert_ne!(addr_0, addr_2, "Subaddress 0 and 2 should differ");
}

#[test]
fn test_derive_many_accounts() {
    // Test deriving up to account 10
    let mut addresses = Vec::new();

    for account in 0..=10 {
        let addr = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", account, 0, "")
            .expect(&format!("Account {} should derive", account));

        // Verify it's a valid stagenet address (starts with 5, 7, or 8)
        let first_char = addr.chars().next().unwrap();
        assert!(
            first_char == '5' || first_char == '7' || first_char == '8',
            "Account {} address should start with 5, 7, or 8 for stagenet, got: {}",
            account,
            first_char
        );

        // Verify it's unique
        assert!(
            !addresses.contains(&addr),
            "Account {} address should be unique",
            account
        );

        addresses.push(addr);
    }

    assert_eq!(addresses.len(), 11, "Should have derived 11 unique addresses");
}

#[test]
fn test_derive_many_subaddresses() {
    // Test deriving up to subaddress 20 for account 0
    let mut addresses = Vec::new();

    for index in 0..=20 {
        let addr = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 0, index, "")
            .expect(&format!("Subaddress {} should derive", index));

        // Verify it's unique
        assert!(
            !addresses.contains(&addr),
            "Subaddress {} should be unique",
            index
        );

        addresses.push(addr);
    }

    assert_eq!(addresses.len(), 21, "Should have derived 21 unique subaddresses");
}

#[test]
fn test_deterministic_derivation() {
    // Deriving the same account/subaddress multiple times should yield the same result
    for _ in 0..5 {
        let addr = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 3, 7, "")
            .expect("Derivation should succeed");

        let addr2 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 3, 7, "")
            .expect("Derivation should succeed");

        assert_eq!(addr, addr2, "Derivation should be deterministic");
    }
}

#[test]
fn test_network_specific_addresses() {
    // Same account/subaddress on different networks should produce different addresses
    let mainnet = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "mainnet", 1, 0, "")
        .expect("mainnet derivation should succeed");

    let testnet = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "testnet", 1, 0, "")
        .expect("testnet derivation should succeed");

    let stagenet = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 1, 0, "")
        .expect("stagenet derivation should succeed");

    assert_ne!(mainnet, testnet, "Mainnet and testnet addresses should differ");
    assert_ne!(testnet, stagenet, "Testnet and stagenet addresses should differ");
    assert_ne!(mainnet, stagenet, "Mainnet and stagenet addresses should differ");

    // Verify network prefixes
    assert!(mainnet.starts_with('4') || mainnet.starts_with('8'), "Mainnet should start with 4 or 8");
    assert!(testnet.starts_with('9') || testnet.starts_with('B') || testnet.starts_with('C'),
            "Testnet should start with 9, B, or C");
    assert!(stagenet.starts_with('5') || stagenet.starts_with('7') || stagenet.starts_with('8'),
            "Stagenet should start with 5, 7, or 8");
}

#[test]
fn test_invalid_network() {
    // Invalid network should return an error
    let result = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "invalidnet", 0, 0, "");
    assert!(result.is_err(), "Invalid network should return error");
}

#[test]
fn test_invalid_seed() {
    // Invalid seed should return an error
    let result = derive_subaddress("invalid seed phrase", "stagenet", 0, 0, "");
    assert!(result.is_err(), "Invalid seed should return error");
}

#[test]
fn test_known_test_vectors() {
    // Test against known addresses
    let account_0_0 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 0, 0, "")
        .expect("should derive");
    assert_eq!(
        account_0_0,
        "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf",
        "Account 0, subaddress 0 should match known value"
    );

    let account_1_0 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 1, 0, "")
        .expect("should derive");
    assert_eq!(
        account_1_0,
        "73jsr6CDS38G24SiXVNFZ5YLivkNAMogYGyFVhwSZkue4vM9ntzaHFYX5Nf6HMnR9dLBn1xrHcY2nPoxtZP8Xso6K8qsih7",
        "Account 1, subaddress 0 should match known value"
    );

    let account_1_1 = derive_subaddress(HONKED_BAGPIPE_MNEMONIC, "stagenet", 1, 1, "")
        .expect("should derive");
    assert_eq!(
        account_1_1,
        "78SkHoSxJbRGriwbroDCJhaEoNK5WD8bsScXEQ6aHnyFECfNuBm35v5BBYEGjVYy4bbhWVj2y8gF7SFCQHULgso5BnQPGsQ",
        "Account 1, subaddress 1 should match known value"
    );
}
