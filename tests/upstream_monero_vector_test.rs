use std::path::{Path, PathBuf};

use monero_rust::{epee_compat, wallet_keys_file::read_keys_file};

fn vector_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/upstream_monero")
        .join(name)
}

fn assert_not_zero_key(key: &[u8; 32], name: &str) {
    assert_ne!(key, &[0u8; 32], "{name} should not be all zeroes");
}

#[test]
fn upstream_wallet_9svhk1_keys_loads_with_known_password() {
    let imported = read_keys_file(&vector_path("wallet_9svHk1.keys"), "test")
        .expect("upstream wallet_9svHk1.keys should decrypt and parse");

    assert!(!imported.watch_only);
    assert_eq!(imported.nettype, 1, "fixture should be a testnet wallet");
    assert!(imported.creation_timestamp > 0);
    assert!(imported.mnemonic.is_some());
    assert_not_zero_key(&imported.spend_secret_key, "spend secret key");
    assert_not_zero_key(&imported.view_secret_key, "view secret key");
    assert_not_zero_key(&imported.spend_public_key, "spend public key");
    assert_not_zero_key(&imported.view_public_key, "view public key");
}

#[test]
fn upstream_wallet_00fd416a_keys_loads_with_known_password() {
    let imported = read_keys_file(&vector_path("wallet_00fd416a.keys"), "beepbeep")
        .expect("upstream wallet_00fd416a.keys should decrypt and parse");

    assert!(!imported.watch_only);
    assert_eq!(imported.nettype, 0, "fixture should be a mainnet wallet");
    assert!(imported.creation_timestamp > 0);
    assert!(imported.mnemonic.is_some());
    assert_not_zero_key(&imported.spend_secret_key, "spend secret key");
    assert_not_zero_key(&imported.view_secret_key, "view secret key");
    assert_not_zero_key(&imported.spend_public_key, "spend public key");
    assert_not_zero_key(&imported.view_public_key, "view public key");
}

#[test]
fn upstream_static_outputs_fixture_pins_legacy_output_export_magic() {
    let data = std::fs::read(vector_path("outputs")).expect("outputs fixture should be readable");

    assert_eq!(data.len(), 1035);
    assert!(
        data.starts_with(b"Monero output export\x03"),
        "fixture should preserve Monero's checked-in legacy output-export marker"
    );
}

#[test]
fn current_offline_signing_magic_constants_match_monero_wallet2() {
    assert_eq!(
        epee_compat::UNSIGNED_TX_MAGIC,
        b"Monero unsigned tx set\x05"
    );
    assert_eq!(epee_compat::SIGNED_TX_MAGIC, b"Monero signed tx set\x05");
    assert_eq!(
        epee_compat::KEY_IMAGES_MAGIC,
        b"Monero key image export\x03"
    );
    assert_eq!(
        epee_compat::OUTPUT_EXPORT_MAGIC,
        b"Monero output export\x04"
    );

    let multisig_unsigned_tx_magic = b"Monero multisig unsigned tx set\x01";
    assert_eq!(multisig_unsigned_tx_magic.len(), 32);
}
