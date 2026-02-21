use monero_rust::wallet_keys_file::{read_keys_file, write_keys_file};
use std::path::{Path, PathBuf};

const HONKED_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const HONKED_STAGENET_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";

const VOCAL_MNEMONIC: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";
const VOCAL_SPEND_KEY: &str = "722bbfcf99a9b2c9e700ce857850dd8c4c94c73dca8d914c603f5fee0e365803";
const VOCAL_VIEW_KEY: &str = "0a1a38f6d246e894600a3e27238a064bf5e8d91801df47a17107596b1378e501";
const VOCAL_STAGENET_ADDRESS: &str = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

fn vector_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors")
        .join(name)
}

#[test]
fn import_honked_keys() {
    let path = vector_path("honked.keys");
    let imported = read_keys_file(&path, "")
        .or_else(|_| read_keys_file(&path, "1"))
        .expect("failed to decrypt honked.keys");

    assert!(!imported.watch_only);

    let mnemonic = imported.mnemonic.as_deref().expect("missing mnemonic");
    assert_eq!(mnemonic, HONKED_MNEMONIC);

    let address =
        monero_rust::derive_address(mnemonic, "stagenet", "").expect("address derivation failed");
    assert_eq!(address, HONKED_STAGENET_ADDRESS);

    let keys = monero_rust::derive_keys(mnemonic, "stagenet", "").unwrap();
    assert_eq!(
        hex::encode(imported.spend_public_key),
        keys.public_spend_key
    );
    assert_eq!(hex::encode(imported.view_public_key), keys.public_view_key);
}

#[test]
fn import_vocal_keys() {
    let path = vector_path("vocal.keys");
    let imported = read_keys_file(&path, "")
        .or_else(|_| read_keys_file(&path, "1"))
        .expect("failed to decrypt vocal.keys");

    assert!(!imported.watch_only);

    assert_eq!(hex::encode(imported.spend_secret_key), VOCAL_SPEND_KEY);
    assert_eq!(hex::encode(imported.view_secret_key), VOCAL_VIEW_KEY);

    let mnemonic = imported.mnemonic.as_deref().expect("missing mnemonic");
    assert_eq!(mnemonic, VOCAL_MNEMONIC);

    let address =
        monero_rust::derive_address(mnemonic, "stagenet", "").expect("address derivation failed");
    assert_eq!(address, VOCAL_STAGENET_ADDRESS);
}

#[test]
fn import_view_only_wallet() {
    let path = vector_path("hemlock_view_only.keys");
    let imported = read_keys_file(&path, "")
        .or_else(|_| read_keys_file(&path, "1"))
        .expect("failed to decrypt hemlock_view_only.keys");

    assert!(imported.watch_only);
    assert!(imported.mnemonic.is_none());
    assert_eq!(imported.spend_secret_key, [0u8; 32]);
    assert_ne!(imported.view_secret_key, [0u8; 32]);
}

fn assert_roundtrip(name: &str) {
    let original = read_keys_file(&vector_path(name), "").unwrap();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_keys_file(tmp.path(), "", &original).unwrap();

    let reimported = read_keys_file(tmp.path(), "").unwrap();

    assert_eq!(original.spend_secret_key, reimported.spend_secret_key);
    assert_eq!(original.view_secret_key, reimported.view_secret_key);
    assert_eq!(original.spend_public_key, reimported.spend_public_key);
    assert_eq!(original.view_public_key, reimported.view_public_key);
    assert_eq!(original.creation_timestamp, reimported.creation_timestamp);
    assert_eq!(original.watch_only, reimported.watch_only);
    assert_eq!(original.seed_language, reimported.seed_language);
    assert_eq!(original.mnemonic, reimported.mnemonic);
    assert_eq!(original.nettype, reimported.nettype);
}

#[test]
fn roundtrip_honked() {
    assert_roundtrip("honked.keys");
}

#[test]
fn roundtrip_vocal() {
    assert_roundtrip("vocal.keys");
}

#[test]
fn roundtrip_view_only() {
    assert_roundtrip("hemlock_view_only.keys");
}

fn assert_bit_exact(name: &str) {
    let original_path = vector_path(name);
    let original_bytes = std::fs::read(&original_path).unwrap();

    let imported = read_keys_file(&original_path, "").unwrap();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_keys_file(tmp.path(), "", &imported).unwrap();

    let written_bytes = std::fs::read(tmp.path()).unwrap();
    assert_eq!(
        original_bytes, written_bytes,
        "{name}: written file differs from original"
    );
}

#[test]
fn bit_exact_honked() {
    assert_bit_exact("honked.keys");
}

#[test]
fn bit_exact_vocal() {
    assert_bit_exact("vocal.keys");
}

#[test]
fn bit_exact_hemlock_view_only() {
    assert_bit_exact("hemlock_view_only.keys");
}

#[test]
fn independent_view_key_does_not_offer_incomplete_mnemonic_backup() {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};

    let mut wallet = read_keys_file(&vector_path("vocal.keys"), "").unwrap();
    let independent_view = Scalar::from(42u64);
    wallet.view_secret_key = independent_view.to_bytes();
    wallet.view_public_key = (&independent_view * ED25519_BASEPOINT_TABLE).compress().to_bytes();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_keys_file(tmp.path(), "", &wallet).unwrap();
    let imported = read_keys_file(tmp.path(), "").unwrap();
    assert_eq!(imported.spend_secret_key, wallet.spend_secret_key);
    assert_eq!(imported.view_secret_key, wallet.view_secret_key);
    assert_eq!(imported.spend_public_key, wallet.spend_public_key);
    assert_eq!(imported.view_public_key, wallet.view_public_key);
    assert!(!imported.watch_only);
    assert!(imported.mnemonic.is_none());
}
