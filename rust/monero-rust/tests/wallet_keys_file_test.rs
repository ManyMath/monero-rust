use std::path::{Path, PathBuf};
use monero_rust::wallet_keys_file::read_keys_file;

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

    let address = monero_rust::derive_address(mnemonic, "stagenet", "")
        .expect("address derivation failed");
    assert_eq!(address, HONKED_STAGENET_ADDRESS);

    let keys = monero_rust::derive_keys(mnemonic, "stagenet", "").unwrap();
    assert_eq!(hex::encode(imported.spend_public_key), keys.public_spend_key);
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

    let address = monero_rust::derive_address(mnemonic, "stagenet", "")
        .expect("address derivation failed");
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
