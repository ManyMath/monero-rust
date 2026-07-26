//! Reject invalid public spend keys when importing view-only wallets.
//!
//! Torsioned keys can decompress successfully but fail `ViewPair` construction.
//! Reject them before address derivation can panic.

use monero_rust::scanner::{
    decompress_public_spend_key, derive_address_from_view_only, derive_keys_from_view_only,
    derive_subaddress, parse_view_only_keys,
};

const VIEW_KEY: &str = "0a0000000000000000000000000000000000000000000000000000000000000a";

/// A curve point with nonzero torsion.
fn torsioned_spend_key() -> String {
    let point = curve25519_dalek::constants::ED25519_BASEPOINT_POINT
        + curve25519_dalek::constants::EIGHT_TORSION[1];
    assert!(!point.is_torsion_free(), "fixture must actually carry torsion");
    assert!(
        curve25519_dalek::edwards::CompressedEdwardsY(point.compress().to_bytes())
            .decompress()
            .is_some(),
        "and must still decompress, or it would be caught as malformed instead"
    );
    hex::encode(point.compress().to_bytes())
}

fn valid_spend_key() -> String {
    let point = curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    hex::encode(point.compress().to_bytes())
}

#[test]
fn rejects_torsioned_spend_key() {
    let bytes: [u8; 32] = hex::decode(torsioned_spend_key()).unwrap().try_into().unwrap();
    assert!(decompress_public_spend_key(bytes).is_none());
}

#[test]
fn accepts_prime_order_spend_key() {
    let bytes: [u8; 32] = hex::decode(valid_spend_key()).unwrap().try_into().unwrap();
    assert!(decompress_public_spend_key(bytes).is_some());
}

#[test]
fn view_only_import_rejects_torsioned_spend_key() {
    let spend = torsioned_spend_key();
    assert!(
        parse_view_only_keys(&format!("viewonly:{}:{}", VIEW_KEY, spend)).is_none(),
        "import must reject a torsioned spend key"
    );
    assert!(
        parse_view_only_keys(&format!("viewonly:{}:{}", VIEW_KEY, valid_spend_key())).is_some(),
        "and must still accept a good one"
    );
}

/// Check subaddress derivation after importing view-only keys.
#[test]
fn subaddress_derivation_errors_rather_than_panicking() {
    let mnemonic = format!("viewonly:{}:{}", VIEW_KEY, torsioned_spend_key());

    let result = derive_subaddress(&mnemonic, "mainnet", 0, 1, "");
    assert!(result.is_err(), "expected an error, got {:?}", result);
}

#[test]
fn address_derivation_errors_rather_than_panicking() {
    let spend = torsioned_spend_key();

    assert!(derive_address_from_view_only(VIEW_KEY, &spend, "mainnet").is_err());
    assert!(derive_keys_from_view_only(VIEW_KEY, &spend, "mainnet").is_err());
}

/// A well-formed view-only import must keep working, so the check can't be over-broad.
#[test]
fn accepts_a_real_view_only_wallet() {
    let spend = valid_spend_key();

    let address = derive_address_from_view_only(VIEW_KEY, &spend, "mainnet")
        .expect("a prime-order spend key must import");
    assert!(address.starts_with('4'), "mainnet address, got {}", address);

    let sub = derive_subaddress(
        &format!("viewonly:{}:{}", VIEW_KEY, spend),
        "mainnet",
        0,
        1,
        "",
    )
    .expect("subaddress derivation must succeed");
    assert!(sub.starts_with('8'), "mainnet subaddress, got {}", sub);
}
