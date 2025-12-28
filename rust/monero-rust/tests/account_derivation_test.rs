//! Test for Monero account and subaddress derivation.
//!
//! This test verifies that the account derivation works correctly for the "honked bagpipe" seed.
//! Specifically, it tests that account 1 (index 1), subaddress 0 produces the expected address.

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_serai::{
    hash_to_scalar,
    wallet::{
        address::{AddressSpec, Network, SubaddressIndex},
        seed::Seed,
        ViewPair,
    },
};
use zeroize::Zeroizing;

const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const EXPECTED_ACCOUNT_0_SUBADDRESS_0: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";
const EXPECTED_ACCOUNT_1_SUBADDRESS_0: &str = "73jsr6CDS38G24SiXVNFZ5YLivkNAMogYGyFVhwSZkue4vM9ntzaHFYX5Nf6HMnR9dLBn1xrHcY2nPoxtZP8Xso6K8qsih7";
const EXPECTED_ACCOUNT_1_SUBADDRESS_1: &str = "78SkHoSxJbRGriwbroDCJhaEoNK5WD8bsScXEQ6aHnyFECfNuBm35v5BBYEGjVYy4bbhWVj2y8gF7SFCQHULgso5BnQPGsQ";

fn spend_key_from_seed(seed: &Seed) -> curve25519_dalek::edwards::EdwardsPoint {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    &spend_scalar * &ED25519_BASEPOINT_TABLE
}

fn view_key_from_seed(seed: &Seed) -> Scalar {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    hash_to_scalar(&spend_bytes)
}

#[test]
fn test_account_0_subaddress_0_is_standard_address() {
    // Account 0, subaddress 0 should be the same as the standard address
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend, Zeroizing::new(view));

    // Standard address
    let standard_address = pair.address(Network::Stagenet, AddressSpec::Standard);
    assert_eq!(
        standard_address.to_string(),
        EXPECTED_ACCOUNT_0_SUBADDRESS_0
    );

    // Note: SubaddressIndex::new(0, 0) returns None, as (0,0) represents the standard address
    // So we can't create a subaddress for account 0, subaddress 0
    assert!(SubaddressIndex::new(0, 0).is_none());
}

#[test]
fn test_account_1_subaddress_0_derivation() {
    // Test that account 1 (index 1), subaddress 0 produces the expected address
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend, Zeroizing::new(view));

    // Create subaddress index for account 1, subaddress 0
    let subaddress_index =
        SubaddressIndex::new(1, 0).expect("SubaddressIndex::new(1, 0) should return Some");

    // Derive the subaddress
    let subaddress = pair.address(Network::Stagenet, AddressSpec::Subaddress(subaddress_index));

    // Verify the derived address matches the expected address
    assert_eq!(
        subaddress.to_string(),
        EXPECTED_ACCOUNT_1_SUBADDRESS_0,
        "Derived address for account 1, subaddress 0 does not match expected address"
    );
}

#[test]
fn test_account_0_subaddress_1_is_different() {
    // Test that account 0, subaddress 1 is different from the standard address
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend, Zeroizing::new(view));

    let standard_address = pair.address(Network::Stagenet, AddressSpec::Standard);

    // Create subaddress index for account 0, subaddress 1
    let subaddress_index =
        SubaddressIndex::new(0, 1).expect("SubaddressIndex::new(0, 1) should return Some");

    let subaddress = pair.address(Network::Stagenet, AddressSpec::Subaddress(subaddress_index));

    // Verify that the subaddress is different from the standard address
    assert_ne!(
        subaddress.to_string(),
        standard_address.to_string(),
        "Subaddress (0, 1) should be different from standard address"
    );
}

#[test]
fn test_account_1_subaddress_1_derivation() {
    // Test that account 1 (index 1), subaddress 1 produces the expected address
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend, Zeroizing::new(view));

    // Create subaddress index for account 1, subaddress 1
    let subaddress_index =
        SubaddressIndex::new(1, 1).expect("SubaddressIndex::new(1, 1) should return Some");

    // Derive the subaddress
    let subaddress = pair.address(Network::Stagenet, AddressSpec::Subaddress(subaddress_index));

    // Verify the derived address matches the expected address
    assert_eq!(
        subaddress.to_string(),
        EXPECTED_ACCOUNT_1_SUBADDRESS_1,
        "Derived address for account 1, subaddress 1 does not match expected address"
    );
}

#[test]
fn test_derive_subaddress_api() {
    // Test the public API function derive_subaddress
    use monero_rust::derive_subaddress;

    let seed = HONKED_BAGPIPE_MNEMONIC;

    // Test account 1, subaddress 0
    let addr_1_0 =
        derive_subaddress(seed, "stagenet", 1, 0, "").expect("derive_subaddress should succeed");
    assert_eq!(addr_1_0, EXPECTED_ACCOUNT_1_SUBADDRESS_0);

    // Test account 1, subaddress 1
    let addr_1_1 =
        derive_subaddress(seed, "stagenet", 1, 1, "").expect("derive_subaddress should succeed");
    assert_eq!(addr_1_1, EXPECTED_ACCOUNT_1_SUBADDRESS_1);

    // Test account 0, subaddress 0 (should return standard address)
    let addr_0_0 = derive_subaddress(seed, "stagenet", 0, 0, "")
        .expect("derive_subaddress should succeed for (0, 0)");
    assert_eq!(addr_0_0, EXPECTED_ACCOUNT_0_SUBADDRESS_0);
}

#[test]
fn test_multiple_accounts_have_different_addresses() {
    // Test that different accounts produce different addresses
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend, Zeroizing::new(view));

    let account_1_addr = pair.address(
        Network::Stagenet,
        AddressSpec::Subaddress(SubaddressIndex::new(1, 0).unwrap()),
    );

    let account_2_addr = pair.address(
        Network::Stagenet,
        AddressSpec::Subaddress(SubaddressIndex::new(2, 0).unwrap()),
    );

    let account_3_addr = pair.address(
        Network::Stagenet,
        AddressSpec::Subaddress(SubaddressIndex::new(3, 0).unwrap()),
    );

    // Verify all accounts have different addresses
    assert_ne!(account_1_addr.to_string(), account_2_addr.to_string());
    assert_ne!(account_2_addr.to_string(), account_3_addr.to_string());
    assert_ne!(account_1_addr.to_string(), account_3_addr.to_string());
}
