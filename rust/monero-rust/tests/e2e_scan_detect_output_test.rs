// ---------------------------------------------------------------------------
// What blocks the 2 ignored tests (test_step2 and test_step3)?
// ---------------------------------------------------------------------------
//
// The test fixture `tests/vectors/honked_bagpipe_rpc.json` was captured from
// a stagenet node with `prune: true` in the get_transactions request.  As a
// result, each transaction entry contains only `pruned_as_hex` (the prefix +
// RctBase) and an empty `prunable_as_hex` field.  The prunable portion of a
// RingCT transaction contains:
//
//   - CLSAG / MLSAG / Bulletproof(+) range proofs
//   - Pseudo-output commitments
//
// `scan_block_for_outputs` (and the monero-serai Scanner) need to decode
// **full** transactions (prefix + RctBase + RctPrunable) in order to:
//
//   1. Verify Pedersen commitment balance and extract the real output amount
//      via ECDH decoding (needs the commitment from RctPrunable for non-
//      coinbase outputs).
//   2. Compute the key image for each owned output (needs the full key
//      derivation path that includes data validated against the prunable
//      section).
//
// To unblock these tests, the fixture must be re-captured from a live
// stagenet node **without** the `prune` flag (or with `prune: false`) so
// that `prunable_as_hex` is populated for both transactions in block
// 1384526.  Specifically, the `get_transactions` entry needs:
//
//   - `as_hex` OR `prunable_as_hex` populated with the full hex-encoded
//     transaction bytes (not just the pruned prefix).
//   - Alternatively, a separate binary fixture from `get_blocks.bin` that
//     returns full block data including prunable sections.
//
// Once the full transaction data is available, the mock RPC will be able to
// return parseable transactions and both `test_step2_scan_and_detect_output`
// and `test_step3_key_image_determinism` will pass.
// ---------------------------------------------------------------------------

use monero_rust::scanner::{derive_address, scan_block_for_outputs, BlockScanResult};
use monero_serai::rpc::{Rpc, RpcConnection, RpcError};
use monero_serai::wallet::seed::Seed;
use serde_json::Value;
use async_trait::async_trait;

const TEST_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const EXPECTED_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";
const TEST_BLOCK_HEIGHT: u64 = 1384526;
const EXPECTED_TX_HASH: &str = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";
const EXPECTED_AMOUNT: u64 = 10_000_000_000_000; // 10 XMR

#[derive(Clone, Debug)]
struct MockRpcConnection {
    test_vectors: Vec<RpcCall>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct RpcCall {
    route: String,
    body: String,
    response: String,
    is_binary: bool,
}

impl MockRpcConnection {
    fn new() -> Self {
        let vectors_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/vectors/honked_bagpipe_rpc.json");

        let json_data = std::fs::read_to_string(&vectors_path)
            .expect("failed to read test vectors file");

        let test_vectors: Vec<RpcCall> = serde_json::from_str(&json_data)
            .expect("failed to parse test vectors JSON");

        Self { test_vectors }
    }

    fn find_response(&self, route: &str, body: &str) -> Option<String> {
        for call in &self.test_vectors {
            if call.route == route {
                if route == "get_block" {
                    let body_json: Value = serde_json::from_str(body).ok()?;
                    let call_body_json: Value = serde_json::from_str(&call.body).ok()?;

                    if body_json.get("height") == call_body_json.get("height") {
                        return Some(call.response.clone());
                    }
                }
                else {
                    return Some(call.response.clone());
                }
            }
        }
        None
    }
}

#[async_trait]
impl RpcConnection for MockRpcConnection {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let body_str = String::from_utf8(body)
            .map_err(|_| RpcError::InvalidNode)?;

        self.find_response(route, &body_str)
            .map(|s| s.into_bytes())
            .ok_or(RpcError::InvalidNode)
    }
}

#[test]
fn test_step1_address_derivation() {
    let result = derive_address(TEST_SEED, "stagenet");
    assert!(result.is_ok(), "Address derivation should succeed");

    let address = result.unwrap();
    assert_eq!(address, EXPECTED_ADDRESS);
}

// Needs full prunable tx data from a live stagenet node
#[tokio::test]
#[ignore = "Mock RPC vectors don't include full prunable transaction data"]
async fn test_step2_scan_and_detect_output() {
    let address = derive_address(TEST_SEED, "stagenet")
        .expect("Address derivation should succeed");
    assert_eq!(address, EXPECTED_ADDRESS);

    let mock_rpc = MockRpcConnection::new();
    let rpc = Rpc::new_with_connection(mock_rpc);

    let scan_result: BlockScanResult = scan_block_for_outputs(
        &rpc,
        TEST_BLOCK_HEIGHT,
        TEST_SEED,
        "stagenet"
    ).await.expect("Block scan should succeed");

    assert!(!scan_result.outputs.is_empty(), "Should detect at least one output");

    let output = &scan_result.outputs[0];

    assert_eq!(output.tx_hash, EXPECTED_TX_HASH);
    assert_eq!(output.amount, EXPECTED_AMOUNT);
    assert_eq!(output.output_index, 0);
    assert!(!output.spent);
    assert_eq!(output.subaddress_index, None);

    assert!(!output.key_image.is_empty());
    assert!(hex::decode(&output.key_image).is_ok());
    assert_eq!(output.key_image.len(), 64);

    assert!(!output.key.is_empty());
    assert!(!output.key_offset.is_empty());
    assert!(!output.commitment_mask.is_empty());
    assert!(!output.received_output_bytes.is_empty());
}

// Needs full prunable tx data
#[tokio::test]
#[ignore = "Mock RPC vectors don't include full prunable transaction data"]
async fn test_step3_key_image_determinism() {
    let mock_rpc = MockRpcConnection::new();
    let rpc = Rpc::new_with_connection(mock_rpc);

    let scan1 = scan_block_for_outputs(&rpc, TEST_BLOCK_HEIGHT, TEST_SEED, "stagenet")
        .await
        .expect("First scan should succeed");

    let mock_rpc2 = MockRpcConnection::new();
    let rpc2 = Rpc::new_with_connection(mock_rpc2);

    let scan2 = scan_block_for_outputs(&rpc2, TEST_BLOCK_HEIGHT, TEST_SEED, "stagenet")
        .await
        .expect("Second scan should succeed");

    assert!(!scan1.outputs.is_empty());
    assert!(!scan2.outputs.is_empty());

    assert_eq!(
        scan1.outputs[0].key_image,
        scan2.outputs[0].key_image,
        "Key image should be deterministic"
    );
}

#[tokio::test]
async fn test_step4_scan_block_no_outputs() {
    let mock_rpc = MockRpcConnection::new();
    let rpc = Rpc::new_with_connection(mock_rpc);

    let result = scan_block_for_outputs(&rpc, 999999, TEST_SEED, "stagenet").await;

    // Mock doesn't have vectors for this block
    assert!(result.is_err());
}

#[tokio::test]
async fn test_step5_invalid_seed_handling() {
    let mock_rpc = MockRpcConnection::new();
    let rpc = Rpc::new_with_connection(mock_rpc);

    let result = scan_block_for_outputs(
        &rpc,
        TEST_BLOCK_HEIGHT,
        "invalid short seed",
        "stagenet"
    ).await;

    assert!(result.is_err());

    let error = result.unwrap_err();
    assert!(
        error.contains("mnemonic") || error.contains("seed"),
        "Error should mention mnemonic/seed: {}", error
    );
}

#[tokio::test]
async fn test_step6_network_validation() {
    let mock_rpc = MockRpcConnection::new();
    let rpc = Rpc::new_with_connection(mock_rpc);

    let result = scan_block_for_outputs(
        &rpc,
        TEST_BLOCK_HEIGHT,
        TEST_SEED,
        "invalid_network"
    ).await;

    assert!(result.is_err());

    let error = result.unwrap_err();
    assert!(
        error.contains("network") || error.contains("Network"),
        "Error should mention network: {}", error
    );
}

#[test]
fn test_step7_direct_scanner_e2e() {
    use std::collections::HashSet;
    use std::io::Cursor;
    use zeroize::Zeroizing;
    use monero_serai::{
        wallet::{
            seed::Seed,
            address::{Network, AddressSpec},
            ViewPair, Scanner,
        },
    };

    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string()))
        .expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend, Zeroizing::new(view));
    let address = pair.address(Network::Stagenet, AddressSpec::Standard);
    assert_eq!(address.to_string(), EXPECTED_ADDRESS);

    let mut scanner = Scanner::from_view(pair, Some(HashSet::new()));

    let (tx_hex, prunable_hash_hex) = get_transaction_info(EXPECTED_TX_HASH);
    let tx_bytes = hex::decode(&tx_hex).expect("failed to decode transaction hex");
    let prunable_hash = hex::decode(&prunable_hash_hex).expect("failed to decode prunable hash hex");
    let mut prunable_hash_array = [0u8; 32];
    prunable_hash_array.copy_from_slice(&prunable_hash);

    let mut cursor = Cursor::new(&tx_bytes);
    let transaction = parse_pruned_transaction(&mut cursor, prunable_hash_array)
        .expect("failed to parse pruned transaction");

    let scan_result = scanner.scan_transaction(&transaction);
    let outputs = scan_result.ignore_timelock();

    assert!(!outputs.is_empty(), "Should detect at least one output");

    let output = &outputs[0];
    assert_eq!(output.data.commitment.amount, EXPECTED_AMOUNT);
    assert_eq!(output.metadata.subaddress, None);
}

fn spend_key_from_seed(seed: &Seed) -> curve25519_dalek::edwards::EdwardsPoint {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};

    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    &spend_scalar * &ED25519_BASEPOINT_TABLE
}

fn view_key_from_seed(seed: &Seed) -> curve25519_dalek::scalar::Scalar {
    use sha3::{Digest, Keccak256};

    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let view_bytes: [u8; 32] = Keccak256::digest(&spend_bytes).into();
    curve25519_dalek::scalar::Scalar::from_bytes_mod_order(view_bytes)
}

fn get_transaction_info(tx_id: &str) -> (String, String) {
    let vectors_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/honked_bagpipe_rpc.json");

    let json_data = std::fs::read_to_string(&vectors_path)
        .expect("failed to read test vectors file");

    #[derive(serde::Deserialize)]
    struct VectorCall {
        route: String,
        response: String,
    }

    #[derive(serde::Deserialize)]
    struct GetTransactionsResponse {
        txs: Vec<TxInfo>,
    }

    #[derive(serde::Deserialize)]
    struct TxInfo {
        pruned_as_hex: String,
        tx_hash: String,
        prunable_hash: String,
    }

    let vectors: Vec<VectorCall> = serde_json::from_str(&json_data)
        .expect("failed to parse test vectors JSON");

    for call in vectors {
        if call.route == "get_transactions" {
            let response: GetTransactionsResponse = serde_json::from_str(&call.response)
                .expect("failed to parse get_transactions response");

            for tx_info in response.txs {
                if tx_info.tx_hash == tx_id {
                    return (tx_info.pruned_as_hex, tx_info.prunable_hash);
                }
            }
        }
    }

    panic!("transaction not found: {}", tx_id);
}

fn parse_pruned_transaction<R: std::io::Read>(r: &mut R, _prunable_hash: [u8; 32]) -> std::io::Result<monero_serai::transaction::Transaction> {
    use monero_serai::{
        transaction::{Transaction, TransactionPrefix},
        ringct::{RctBase, RctSignatures, RctPrunable},
    };

    let prefix = TransactionPrefix::read(r)?;

    if prefix.version != 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid version"
        ));
    }

    let (rct_base, _rct_type) = RctBase::read(prefix.outputs.len(), r)?;

    let rct_sigs_complete = RctSignatures {
        base: rct_base,
        prunable: RctPrunable::Null,
    };

    Ok(Transaction {
        prefix,
        signatures: vec![],
        rct_signatures: rct_sigs_complete,
    })
}
