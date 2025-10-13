// Integration test using real stagenet data to verify output scanning.
// TODO: Replace live node lookups with hardcoded responses for deterministic testing.

use monero_rust::WalletState;
use monero_wallet::address::Network;
use std::path::PathBuf;

// Stagenet test vector:
// Polyseed: naive cake plug stereo fatal hour because cart ecology acoustic one sting gravity tail fish beyond
// Block 2032104, TXID 243b176f1e5e0592eb0c3c82a3f1a2db81d63bdcb545852d44f92b97e9a9cd57
// 0.01 sXMR to primary address
const TEST_BLOCK_HEIGHT: u64 = 2032104;
const TEST_ADDRESS: &str = "54psCSW7BPg37GD1rNnd2J2FWzDADGd2sVVw9rp9qBmFb7if1tsMnuB5UVs1DioQUDCyFpNAjKAyd7svQAHjvXEaPS7Fcdf";
const EXPECTED_AMOUNT: u64 = 10_000_000_000; // 0.01 XMR in piconeros
const TEST_TXID: &str = "243b176f1e5e0592eb0c3c82a3f1a2db81d63bdcb545852d44f92b97e9a9cd57";

const SECRET_VIEW_KEY: &str = "20b100c6ce60d5582bac22e93aa1dd4508a509a19714605bff86db577857720b";
const PUBLIC_SPEND_KEY: &str = "4f09d7fe4ebbab0c9a4c00ae88d95307799a90ab95cfb4aa622efe6f979a0ecb";

fn txid_from_hex(hex: &str) -> [u8; 32] {
    let bytes = hex::decode(hex).expect("valid hex");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    arr
}

#[tokio::test]
async fn test_scan_real_stagenet_output() {
    let spend_public = hex::decode(PUBLIC_SPEND_KEY).unwrap();
    let view_private = hex::decode(SECRET_VIEW_KEY).unwrap();

    let mut spend_public_bytes = [0u8; 32];
    let mut view_private_bytes = [0u8; 32];
    spend_public_bytes.copy_from_slice(&spend_public);
    view_private_bytes.copy_from_slice(&view_private);

    let mut wallet = WalletState::new_view_only(
        spend_public_bytes,
        view_private_bytes,
        Network::Stagenet,
        "test_password",
        PathBuf::from("/tmp/test_stagenet_wallet.bin"),
        TEST_BLOCK_HEIGHT.saturating_sub(10),
    )
    .expect("failed to create view-only wallet");

    let wallet_address = wallet.view_pair.legacy_address(Network::Stagenet).to_string();
    assert_eq!(wallet_address, TEST_ADDRESS, "address mismatch");

    let config = monero_rust::rpc::ConnectionConfig::new(
        "http://node.monerodevs.org:38089".to_string(),
    );
    wallet.connect(config).await.expect("failed to connect to daemon");

    let outputs_found = wallet
        .scan_block_by_height(TEST_BLOCK_HEIGHT)
        .await
        .expect("failed to scan block");

    println!(
        "scanned block {}, found {} owned output(s)",
        TEST_BLOCK_HEIGHT, outputs_found
    );

    let expected_txid = txid_from_hex(TEST_TXID);
    let output = wallet
        .outputs
        .values()
        .find(|o| o.tx_hash == expected_txid)
        .expect("expected output not found");

    println!(
        "found output: tx={}, index={}, amount={}",
        hex::encode(output.tx_hash),
        output.output_index,
        output.amount
    );

    assert_eq!(output.amount, EXPECTED_AMOUNT);
    assert_eq!(output.subaddress_indices, (0, 0));
    assert_eq!(output.height, TEST_BLOCK_HEIGHT);
    assert!(!output.spent);
    assert!(!output.frozen);
    assert_eq!(wallet.get_balance(), EXPECTED_AMOUNT);
}

#[test]
fn test_vector_parsing() {
    let txid = txid_from_hex(TEST_TXID);
    assert_eq!(txid.len(), 32);
    assert_eq!(hex::encode(txid), TEST_TXID);
    assert_eq!(EXPECTED_AMOUNT, 10_000_000_000);
}
