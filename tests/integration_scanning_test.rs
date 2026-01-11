// Integration test for output scanning with real stagenet data
//
// This test uses a real stagenet wallet and transaction to verify that
// the scanner correctly detects owned outputs.
//
// TODO: Replace live node lookups with hardcoded responses for deterministic testing
//       - Hardcode the response from get_scannable_block_by_number(2032104)
//       - Parse hardcoded data as if it were a live response
//       - Add separate end-to-end tests that verify node responses match hardcoded data
//       - This will make tests reproducible without external dependencies
//
// Run with: cargo test --test integration_scanning_test -- --nocapture

use monero_rust::WalletState;
use monero_wallet::address::Network;
use std::path::PathBuf;

/// Real stagenet test vector
///
/// Polyseed: naive cake plug stereo fatal hour because cart ecology acoustic one sting gravity tail fish beyond
/// Address: 54psCSW7BPg37GD1rNnd2J2FWzDADGd2sVVw9rp9qBmFb7if1tsMnuB5UVs1DioQUDCyFpNAjKAyd7svQAHjvXEaPS7Fcdf
/// Block Height: 2032104
/// Amount: 0.01 sXMR = 10,000,000,000 piconeros
/// TXID: 243b176f1e5e0592eb0c3c82a3f1a2db81d63bdcb545852d44f92b97e9a9cd57
/// Tx Public Key: 1e8e8860d1b277cff07bd33578dae4153aa9956e3a42de5ba2f8f9f9e02bfdb0
/// Encrypted Payment ID: e70460abed2e4ebf
///
/// Keys:
/// - Secret spend: 4b185b97d1c5ec5b8fd3558391108d00ac979eabaeeb8e7479a3630e07caee02
/// - Secret view:  20b100c6ce60d5582bac22e93aa1dd4508a509a19714605bff86db577857720b
/// - Public spend: 4f09d7fe4ebbab0c9a4c00ae88d95307799a90ab95cfb4aa622efe6f979a0ecb
/// - Public view:  f7191a8f471e461abfb676eba6b9a6479040a00fbb5be8291edb3cc7b2c0f3c6
///
/// Transaction has 2 outputs:
/// - Output 0: Stealth addr 9c1cda652afb9f86e02825217c6249433c466c083d90ddc19a1581366903f092 (idx 9682524)
/// - Output 1: Stealth addr dbb573c96849ab29931b7d87f0963c73ca80ad5fd89052ff974ba6abb55a3694 (idx 9682525)
const TEST_BLOCK_HEIGHT: u64 = 2032104;
const TEST_ADDRESS: &str = "54psCSW7BPg37GD1rNnd2J2FWzDADGd2sVVw9rp9qBmFb7if1tsMnuB5UVs1DioQUDCyFpNAjKAyd7svQAHjvXEaPS7Fcdf";
const EXPECTED_AMOUNT: u64 = 10_000_000_000; // 0.01 XMR in piconeros
const TEST_TXID: &str = "243b176f1e5e0592eb0c3c82a3f1a2db81d63bdcb545852d44f92b97e9a9cd57";

// Private keys (from polyseed derivation)
#[allow(dead_code)]
const SECRET_SPEND_KEY: &str = "4b185b97d1c5ec5b8fd3558391108d00ac979eabaeeb8e7479a3630e07caee02";
const SECRET_VIEW_KEY: &str = "20b100c6ce60d5582bac22e93aa1dd4508a509a19714605bff86db577857720b";
const PUBLIC_SPEND_KEY: &str = "4f09d7fe4ebbab0c9a4c00ae88d95307799a90ab95cfb4aa622efe6f979a0ecb";
#[allow(dead_code)]
const PUBLIC_VIEW_KEY: &str = "f7191a8f471e461abfb676eba6b9a6479040a00fbb5be8291edb3cc7b2c0f3c6";

fn txid_from_hex(hex: &str) -> [u8; 32] {
    let bytes = hex::decode(hex).expect("Valid hex");
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    arr
}

#[tokio::test]
async fn test_scan_real_stagenet_output() {
    println!("\n=== Integration Test: Real Stagenet Output Detection ===\n");

    // Step 1: Create wallet from raw private keys
    println!("Step 1: Creating view-only wallet from private keys...");

    let spend_public = hex::decode(PUBLIC_SPEND_KEY).expect("Valid public spend key hex");
    let view_private = hex::decode(SECRET_VIEW_KEY).expect("Valid secret view key hex");

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
        TEST_BLOCK_HEIGHT.saturating_sub(10), // Start scanning a few blocks before
    ).expect("Create view-only wallet");

    println!("✓ View-only wallet created");

    // Verify the address matches expected
    // Access the view_pair to generate the address
    let wallet_address = wallet.view_pair.legacy_address(Network::Stagenet).to_string();
    println!("Generated Address: {}", wallet_address);
    println!("Expected Address:  {}", TEST_ADDRESS);

    assert_eq!(
        wallet_address, TEST_ADDRESS,
        "Wallet address must match test vector"
    );
    println!("✓ Address matches expected value\n");

    // Step 2: Connect to stagenet daemon
    println!("Step 2: Connecting to stagenet daemon...");

    // TODO: Replace this live node connection with hardcoded block data
    //       Instead of connecting to a real node, we should:
    //       1. Have a hardcoded ScannableBlock for block 2032104
    //       2. Call wallet.scan_block(hardcoded_block, 2032104) directly
    //       3. This makes tests deterministic and removes external dependency
    let config = monero_rust::rpc::ConnectionConfig::new("http://node.monerodevs.org:38089".to_string());
    wallet.connect(config).await.expect("Connect to daemon");

    println!("✓ Connected to daemon\n");

    // Step 3: Scan the block containing the transaction
    println!("Step 3: Scanning block {} for transaction...", TEST_BLOCK_HEIGHT);

    // TODO: Replace wallet.scan_block_by_height() with wallet.scan_block(hardcoded_block, height)
    //       once we have hardcoded the block data from the node
    let outputs_found = wallet.scan_block_by_height(TEST_BLOCK_HEIGHT)
        .await
        .expect("Scan block");

    println!("✓ Scanned block {}, found {} owned output(s)\n", TEST_BLOCK_HEIGHT, outputs_found);

    // Step 4: Verify the output was detected
    println!("Step 4: Verifying detected output...");

    let expected_txid = txid_from_hex(TEST_TXID);
    let found_output = wallet.outputs.values()
        .find(|o| o.tx_hash == expected_txid);

    if found_output.is_none() {
        println!("❌ ERROR: Expected output not found!");
        println!("Transaction: {}", TEST_TXID);
        println!("\nOutputs found in wallet:");
        for (_ki, output) in &wallet.outputs {
            println!("  - TX: {}, Index: {}, Amount: {}",
                     hex::encode(output.tx_hash),
                     output.output_index,
                     output.amount);
        }
        panic!("Expected output from test transaction not detected");
    }

    let output = found_output.unwrap();
    println!("✓ Found output from test transaction!");
    println!("\nOutput Details:");
    println!("  - Transaction: {}", hex::encode(output.tx_hash));
    println!("  - Output Index: {}", output.output_index);
    println!("  - Amount: {} piconeros (0.{:02} sXMR)",
             output.amount,
             output.amount / 10_000_000_000);
    println!("  - Subaddress: ({}, {})",
             output.subaddress_indices.0,
             output.subaddress_indices.1);
    println!("  - Block Height: {}", output.height);
    println!("  - Key Image: {}", hex::encode(output.key_image));

    // Step 5: Verify output properties
    println!("\nStep 5: Verifying output properties...");

    // Verify amount
    assert_eq!(
        output.amount,
        EXPECTED_AMOUNT,
        "Amount should be 0.01 sXMR = {} piconeros",
        EXPECTED_AMOUNT
    );
    println!("✓ Amount is correct: {} piconeros", output.amount);

    // Verify it's to primary address (0, 0)
    assert_eq!(
        output.subaddress_indices,
        (0, 0),
        "Output should be to primary address (0, 0)"
    );
    println!("✓ Subaddress is correct: ({}, {})",
             output.subaddress_indices.0,
             output.subaddress_indices.1);

    // Verify block height
    assert_eq!(
        output.height,
        TEST_BLOCK_HEIGHT,
        "Output height should match block height"
    );
    println!("✓ Block height is correct: {}", output.height);

    // Verify it's marked as unspent and not frozen
    assert!(!output.spent, "Output should not be marked as spent");
    assert!(!output.frozen, "Output should not be frozen");
    println!("✓ Output is unspent and not frozen");

    // Verify balance calculation
    let balance = wallet.get_balance();
    assert_eq!(balance, EXPECTED_AMOUNT, "Wallet balance should equal detected output amount");
    println!("✓ Wallet balance is correct: {} piconeros\n", balance);

    println!("=== Integration Test: SUCCESS ===");
    println!("All verifications passed! ✓\n");
}

#[test]
fn test_vector_parsing() {
    // Test that we can parse the test vector data correctly
    let txid = txid_from_hex(TEST_TXID);
    assert_eq!(txid.len(), 32);
    assert_eq!(hex::encode(txid), TEST_TXID);

    // Verify amount calculation
    assert_eq!(EXPECTED_AMOUNT, 10_000_000_000);
    assert_eq!(EXPECTED_AMOUNT as f64 / 1_000_000_000_000.0, 0.01);
}
