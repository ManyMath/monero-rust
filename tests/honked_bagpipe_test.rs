mod test_helpers;

use test_helpers::{test_vector_path, MockWalletHelper};
use monero_seed::{Language, Seed};
use monero_wallet::address::Network;
use tempfile::TempDir;

const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const EXPECTED_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";
const BLOCK_HEIGHT: u64 = 1384526;
const TX_ID: &str = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";
const EXPECTED_AMOUNT: u64 = 10_000_000_000_000;

fn txid_from_hex(hex: &str) -> [u8; 32] {
    let bytes = hex::decode(hex).expect("valid hex");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    arr
}

#[test]
fn test_address_derivation() {
    use monero_rust::WalletState;

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()),
    ).expect("valid mnemonic");

    let temp_dir = TempDir::new().unwrap();
    let wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        temp_dir.path().join("test.mw"),
        BLOCK_HEIGHT.saturating_sub(10),
    ).expect("wallet creation failed");

    assert_eq!(wallet.get_address(), EXPECTED_ADDRESS);
}

#[tokio::test]
async fn test_scan_output_deterministic() {
    let recording_path = test_vector_path("honked_bagpipe_rpc.json");
    assert!(recording_path.exists(), "Run: STAGENET_NODE=127.0.0.1:38081 cargo test --test honked_bagpipe_test test_scan_output_live -- --ignored");

    let mut helper = MockWalletHelper::from_mnemonic_and_recording(
        HONKED_BAGPIPE_MNEMONIC,
        Network::Stagenet,
        recording_path,
        BLOCK_HEIGHT.saturating_sub(10),
    ).expect("wallet setup failed");

    assert_eq!(helper.wallet.get_address(), EXPECTED_ADDRESS);

    let outputs_found = helper.scan_block_deterministic(BLOCK_HEIGHT).await.expect("scan failed");
    assert_eq!(outputs_found, 1);

    let expected_txid = txid_from_hex(TX_ID);
    let output = helper.wallet.outputs.values()
        .find(|o| o.tx_hash == expected_txid)
        .expect("output not found");

    assert_eq!(output.amount, EXPECTED_AMOUNT);
    assert_eq!(output.subaddress_indices, (0, 0));
    assert_eq!(output.height, BLOCK_HEIGHT);
    assert!(!output.spent);
    assert!(!output.frozen);
    assert_eq!(helper.wallet.get_balance(), EXPECTED_AMOUNT);
}

#[tokio::test]
#[ignore]
async fn test_scan_output_live() {
    use monero_rust::{rpc::ConnectionConfig, WalletState};
    use std::time::Duration;

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()),
    ).expect("valid mnemonic");

    let temp_dir = TempDir::new().unwrap();
    let mut wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        temp_dir.path().join("live.mw"),
        BLOCK_HEIGHT.saturating_sub(10),
    ).expect("wallet creation failed");

    let node = std::env::var("STAGENET_NODE").unwrap_or_else(|_| "127.0.0.1:38081".to_string());
    let config = ConnectionConfig::new(format!("http://{}", node))
        .with_trusted(true)
        .with_timeout(Duration::from_secs(30));

    wallet.connect(config).await.expect("connection failed");

    let outputs_found = wallet.scan_block_by_height(BLOCK_HEIGHT).await.expect("scan failed");
    assert_eq!(outputs_found, 1);

    let expected_txid = txid_from_hex(TX_ID);
    let output = wallet.outputs.values()
        .find(|o| o.tx_hash == expected_txid)
        .expect("output not found");

    assert_eq!(output.amount, EXPECTED_AMOUNT);
    assert_eq!(output.subaddress_indices, (0, 0));
    assert_eq!(output.height, BLOCK_HEIGHT);
    assert!(!output.spent);
    assert!(!output.frozen);
    assert_eq!(wallet.get_balance(), EXPECTED_AMOUNT);

    wallet.disconnect().await;
}
