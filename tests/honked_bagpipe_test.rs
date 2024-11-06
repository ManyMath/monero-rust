//! Test for the "honked bagpipe" mnemonic with deterministic RPC responses.
//!
//! This test verifies scanning of a known output on stagenet:
//! - Mnemonic: honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime
//! - Primary address: 58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf
//! - Block: 1384526 (stagenet)
//! - TX: 07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5
//! - Amount: 10 XMR

mod test_helpers;

use test_helpers::{test_vector_path, MockWalletHelper};
use monero_seed::{Language, Seed};
use monero_wallet::address::Network;
use tempfile::TempDir;

const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const EXPECTED_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";
const BLOCK_HEIGHT: u64 = 1384526;
const TX_ID: &str = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";
const EXPECTED_AMOUNT: u64 = 10_000_000_000_000; // 10 XMR in piconeros

fn txid_from_hex(hex: &str) -> [u8; 32] {
    let bytes = hex::decode(hex).expect("valid hex");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    arr
}

#[test]
fn test_honked_bagpipe_address_derivation() {
    // Verify that the mnemonic produces the expected address
    use monero_rust::WalletState;

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()),
    )
    .expect("valid mnemonic");

    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("honked_bagpipe.mw");

    let wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        wallet_path,
        BLOCK_HEIGHT.saturating_sub(10),
    )
    .expect("Failed to create wallet");

    let address = wallet.get_address();
    assert_eq!(
        address, EXPECTED_ADDRESS,
        "Mnemonic should derive the expected address"
    );
}

#[tokio::test]
async fn test_honked_bagpipe_deterministic() {
    // This test uses pre-recorded RPC responses for deterministic behavior
    let recording_path = test_vector_path("honked_bagpipe_rpc.json");

    if !recording_path.exists() {
        panic!(
            "Recording file not found: {:?}\nRun: STAGENET_NODE=127.0.0.1:38081 cargo test --test honked_bagpipe_test test_honked_bagpipe_live -- --ignored",
            recording_path
        );
    }

    let mut helper = MockWalletHelper::from_mnemonic_and_recording(
        HONKED_BAGPIPE_MNEMONIC,
        Network::Stagenet,
        recording_path,
        BLOCK_HEIGHT.saturating_sub(10),
    )
    .expect("Failed to create wallet helper");

    let address = helper.wallet.get_address();
    assert_eq!(
        address, EXPECTED_ADDRESS,
        "Mnemonic should derive the expected address"
    );

    // Scan the block containing the transaction
    let outputs_found = helper
        .scan_block_deterministic(BLOCK_HEIGHT)
        .await
        .expect("Failed to scan block");

    println!("Scanned block {}, found {} owned output(s)", BLOCK_HEIGHT, outputs_found);

    // Verify we found the expected output
    let expected_txid = txid_from_hex(TX_ID);
    let output = helper.wallet
        .outputs
        .values()
        .find(|o| o.tx_hash == expected_txid)
        .expect("Expected output not found");

    println!(
        "Found output: tx={}, index={}, amount={}",
        hex::encode(output.tx_hash),
        output.output_index,
        output.amount
    );

    assert_eq!(output.amount, EXPECTED_AMOUNT, "Amount should be 10 XMR");
    assert_eq!(output.subaddress_indices, (0, 0), "Should be primary address");
    assert_eq!(output.height, BLOCK_HEIGHT, "Block height should match");
    assert!(!output.spent, "Output should not be spent");
    assert!(!output.frozen, "Output should not be frozen");
    assert_eq!(helper.wallet.get_balance(), EXPECTED_AMOUNT, "Balance should be 10 XMR");
}

#[tokio::test]
#[ignore] // Run with --ignored to record live responses
async fn test_honked_bagpipe_live() {
    // This test connects to a live node and can be used to update recordings
    use monero_rust::{rpc::ConnectionConfig, WalletState};
    use std::time::Duration;

    let seed = Seed::from_string(
        Language::English,
        zeroize::Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()),
    )
    .expect("valid mnemonic");

    let temp_dir = TempDir::new().unwrap();
    let wallet_path = temp_dir.path().join("honked_bagpipe_live.mw");

    let mut wallet = WalletState::new(
        seed,
        "English".to_string(),
        Network::Stagenet,
        "test_password",
        wallet_path,
        BLOCK_HEIGHT.saturating_sub(10),
    )
    .expect("Failed to create wallet");

    // Connect to local stagenet node or a public one
    let node_address = std::env::var("STAGENET_NODE")
        .unwrap_or_else(|_| "127.0.0.1:38081".to_string());

    let config = ConnectionConfig::new(format!("http://{}", node_address))
        .with_trusted(true)
        .with_timeout(Duration::from_secs(30));

    wallet
        .connect(config)
        .await
        .expect("Failed to connect to stagenet node");

    println!("Connected to stagenet node at {}, daemon height: {}", node_address, wallet.daemon_height);

    // Scan the block containing the transaction
    let outputs_found = wallet
        .scan_block_by_height(BLOCK_HEIGHT)
        .await
        .expect("Failed to scan block");

    println!("Scanned block {}, found {} owned output(s)", BLOCK_HEIGHT, outputs_found);

    // Verify we found the expected output
    let expected_txid = txid_from_hex(TX_ID);
    let output = wallet
        .outputs
        .values()
        .find(|o| o.tx_hash == expected_txid)
        .expect("Expected output not found");

    println!(
        "Found output: tx={}, index={}, amount={}",
        hex::encode(output.tx_hash),
        output.output_index,
        output.amount
    );

    assert_eq!(output.amount, EXPECTED_AMOUNT, "Amount should be 10 XMR");
    assert_eq!(output.subaddress_indices, (0, 0), "Should be primary address");
    assert_eq!(output.height, BLOCK_HEIGHT, "Block height should match");
    assert!(!output.spent, "Output should not be spent");
    assert!(!output.frozen, "Output should not be frozen");
    assert_eq!(wallet.get_balance(), EXPECTED_AMOUNT, "Balance should be 10 XMR");

    wallet.disconnect().await;
}
