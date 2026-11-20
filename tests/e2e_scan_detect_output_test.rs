use async_trait::async_trait;
use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError};
use monero_rust::scanner::{
    derive_address, scan_block_for_outputs, scan_block_multi_wallet, BlockScanResult, Lookahead,
    WalletScanConfig,
};
use monero_rust::seed_compat::Seed;
use monero_rust::Network;
use serde_json::Value;

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
    // Retain this fixture field for deserialization; this test serves only JSON routes.
    #[allow(dead_code)]
    is_binary: bool,
}

impl MockRpcConnection {
    fn new() -> Self {
        let vectors_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/vectors/honked_bagpipe_rpc.json");

        let json_data =
            std::fs::read_to_string(&vectors_path).expect("failed to read test vectors file");

        let test_vectors: Vec<RpcCall> =
            serde_json::from_str(&json_data).expect("failed to parse test vectors JSON");

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
                } else {
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
        if route == "getblocks.bin" {
            #[derive(serde::Serialize)]
            struct Blob(#[serde(with = "serde_bytes")] Vec<u8>);
            #[derive(serde::Serialize)]
            struct Entry {
                #[serde(with = "serde_bytes")]
                block: Vec<u8>,
                txs: Vec<Blob>,
                pruned: bool,
                block_weight: u64,
            }
            #[derive(serde::Serialize)]
            struct TxIndices {
                indices: Vec<u64>,
            }
            #[derive(serde::Serialize)]
            struct BlockIndices {
                indices: Vec<TxIndices>,
            }
            #[derive(serde::Serialize)]
            struct Response {
                status: String,
                blocks: Vec<Entry>,
                start_height: u64,
                current_height: u64,
                output_indices: Vec<BlockIndices>,
                untrusted: bool,
                credits: u64,
                top_hash: String,
                daemon_time: u64,
            }

            let block_response: Value = serde_json::from_str(
                &self
                    .test_vectors
                    .iter()
                    .find(|call| call.route == "json_rpc")
                    .ok_or(RpcError::InvalidNode)?
                    .response,
            )
            .map_err(|_| RpcError::InvalidNode)?;
            let block = hex::decode(
                block_response["result"]["blob"]
                    .as_str()
                    .ok_or(RpcError::InvalidNode)?,
            )
            .map_err(|_| RpcError::InvalidNode)?;
            let transactions_response: Value = serde_json::from_str(
                &self
                    .test_vectors
                    .iter()
                    .find(|call| call.route == "get_transactions")
                    .ok_or(RpcError::InvalidNode)?
                    .response,
            )
            .map_err(|_| RpcError::InvalidNode)?;
            let txs = transactions_response["txs"]
                .as_array()
                .ok_or(RpcError::InvalidNode)?
                .iter()
                .map(|tx| {
                    hex::decode(tx["as_hex"].as_str().ok_or(RpcError::InvalidNode)?)
                        .map(Blob)
                        .map_err(|_| RpcError::InvalidNode)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let indices = transactions_response["txs"]
                .as_array()
                .ok_or(RpcError::InvalidNode)?
                .iter()
                .map(|tx| {
                    Ok(TxIndices {
                        indices: serde_json::from_value(tx["output_indices"].clone())
                            .map_err(|_| RpcError::InvalidNode)?,
                    })
                })
                .collect::<Result<Vec<_>, RpcError>>()?;
            let height_response: Value = serde_json::from_str(
                &self
                    .test_vectors
                    .iter()
                    .find(|call| call.route == "get_height")
                    .ok_or(RpcError::InvalidNode)?
                    .response,
            )
            .map_err(|_| RpcError::InvalidNode)?;
            return monero_epee_bin_serde::to_bytes(&Response {
                status: "OK".to_string(),
                blocks: vec![Entry {
                    block,
                    txs,
                    pruned: false,
                    block_weight: 0,
                }],
                start_height: TEST_BLOCK_HEIGHT,
                current_height: height_response["height"]
                    .as_u64()
                    .unwrap_or(TEST_BLOCK_HEIGHT + 1),
                // The captured response omits the miner transaction's entry.
                output_indices: vec![BlockIndices { indices }],
                untrusted: false,
                credits: 0,
                top_hash: String::new(),
                daemon_time: 0,
            })
            .map_err(|_| RpcError::InvalidNode);
        }
        if route == "get_o_indexes.bin" {
            #[derive(serde::Serialize)]
            struct OutputIndexes {
                o_indexes: Vec<u64>,
                status: String,
                untrusted: bool,
                credits: usize,
                top_hash: String,
            }
            return monero_epee_bin_serde::to_bytes(&OutputIndexes {
                o_indexes: vec![6_693_929, 6_693_930],
                status: "OK".to_string(),
                untrusted: false,
                credits: 0,
                top_hash: String::new(),
            })
            .map_err(|_| RpcError::InvalidNode);
        }
        if route.ends_with(".bin") {
            return Err(RpcError::InvalidNode);
        }
        let body_str = String::from_utf8(body).map_err(|_| RpcError::InvalidNode)?;

        self.find_response(route, &body_str)
            .map(|s| s.into_bytes())
            .ok_or(RpcError::InvalidNode)
    }
}

#[test]
fn test_step1_address_derivation() {
    let result = derive_address(TEST_SEED, monero_rust::Network::Stagenet, "");
    assert!(result.is_ok(), "Address derivation should succeed");

    let address = result.unwrap();
    assert_eq!(address.to_string(), EXPECTED_ADDRESS);
}

#[tokio::test]
async fn test_step2_scan_and_detect_output() {
    let address = derive_address(TEST_SEED, monero_rust::Network::Stagenet, "")
        .expect("Address derivation should succeed");
    assert_eq!(address.to_string(), EXPECTED_ADDRESS);

    let mock_rpc = MockRpcConnection::new();
    let rpc = Rpc::new_with_connection(mock_rpc);

    let scan_result: BlockScanResult =
        scan_block_for_outputs(&rpc, TEST_BLOCK_HEIGHT, TEST_SEED, Network::Stagenet, "")
            .await
            .expect("Block scan should succeed");

    assert!(
        !scan_result.outputs.is_empty(),
        "Should detect at least one output"
    );

    let output = &scan_result.outputs[0];

    assert_eq!(output.tx_hash, EXPECTED_TX_HASH);
    assert_eq!(output.amount, EXPECTED_AMOUNT);
    assert_eq!(output.output_index, 1);
    assert_eq!(output.global_index, Some(6_693_930));
    assert!(!output.spent);
    assert_eq!(output.subaddress_index, None);

    // Key image computation is only enabled on WASM targets; on native it is
    // left empty by scan_block_for_outputs, so we only assert on the fields
    // that are always populated.
    assert!(!output.key.is_empty());
    assert!(!output.key_offset.is_empty());
    assert!(!output.commitment_mask.is_empty());
    assert!(!output.received_output_bytes.is_empty());
}

#[tokio::test]
async fn single_block_multi_wallet_scan_returns_spendable_indexes() {
    let rpc = Rpc::new_with_connection(MockRpcConnection::new());
    let result = scan_block_multi_wallet(
        &rpc,
        TEST_BLOCK_HEIGHT,
        vec![WalletScanConfig {
            mnemonic: TEST_SEED.to_string(),
            network: Network::Stagenet,
            lookahead: Lookahead {
                account: 0,
                subaddress: 20,
            },
            passphrase: String::new(),
        }],
    )
    .await
    .expect("indexed multi-wallet scan");
    let output = result
        .wallet_results
        .get(EXPECTED_ADDRESS)
        .and_then(|wallet| wallet.outputs.first())
        .expect("owned output");
    assert_eq!(output.global_index, Some(6_693_930));
}

#[tokio::test]
async fn test_step3_key_image_determinism() {
    let mock_rpc = MockRpcConnection::new();
    let rpc = Rpc::new_with_connection(mock_rpc);

    let scan1 = scan_block_for_outputs(&rpc, TEST_BLOCK_HEIGHT, TEST_SEED, Network::Stagenet, "")
        .await
        .expect("First scan should succeed");

    let mock_rpc2 = MockRpcConnection::new();
    let rpc2 = Rpc::new_with_connection(mock_rpc2);

    let scan2 = scan_block_for_outputs(&rpc2, TEST_BLOCK_HEIGHT, TEST_SEED, Network::Stagenet, "")
        .await
        .expect("Second scan should succeed");

    assert!(!scan1.outputs.is_empty());
    assert!(!scan2.outputs.is_empty());

    assert_eq!(
        scan1.outputs[0].key_image, scan2.outputs[0].key_image,
        "Key image should be deterministic"
    );
}

#[tokio::test]
async fn test_step4_scan_block_no_outputs() {
    let mock_rpc = MockRpcConnection::new();
    let rpc = Rpc::new_with_connection(mock_rpc);

    let result = scan_block_for_outputs(&rpc, 999999, TEST_SEED, Network::Stagenet, "").await;

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
        Network::Stagenet,
        "",
    )
    .await;

    assert!(result.is_err());

    let error = result.unwrap_err();
    assert!(
        error.contains("mnemonic") || error.contains("seed"),
        "Error should mention mnemonic/seed: {}",
        error
    );
}

#[test]
fn test_step7_direct_scanner_e2e() {
    use monero_oxide::transaction::{NotPruned, Pruned, Transaction};
    use monero_rust::scanner::scan_single_transaction;
    use monero_rust::seed_compat::Seed;
    use monero_wallet::address::Network;
    use monero_wallet::ed25519::{Point, Scalar as OxideScalar};
    use monero_wallet::{Scanner, ViewPair};
    use zeroize::Zeroizing;

    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string())).expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(Point::from(spend), Zeroizing::new(OxideScalar::from(view)))
        .expect("view pair");
    let address = pair.legacy_address(Network::Stagenet);
    assert_eq!(address.to_string(), EXPECTED_ADDRESS);

    let mut scanner = Scanner::new(pair);

    let tx_hex = get_transaction_hex(EXPECTED_TX_HASH);
    let tx_bytes = hex::decode(&tx_hex).expect("failed to decode transaction hex");

    let transaction = Transaction::<NotPruned>::read::<&[u8]>(&mut tx_bytes.as_ref())
        .expect("failed to parse transaction");

    let outputs = scan_single_transaction(
        &mut scanner,
        transaction.hash(),
        &Transaction::<Pruned>::from(transaction),
    )
    .expect("scan failed");

    assert!(!outputs.is_empty(), "Should detect at least one output");

    let output = &outputs[0];
    assert_eq!(output.commitment().amount, EXPECTED_AMOUNT);
    assert_eq!(output.subaddress(), None);
}

fn spend_key_from_seed(seed: &Seed) -> curve25519_dalek::edwards::EdwardsPoint {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};

    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    &spend_scalar * ED25519_BASEPOINT_TABLE
}

fn view_key_from_seed(seed: &Seed) -> curve25519_dalek::scalar::Scalar {
    use sha3::{Digest, Keccak256};

    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let view_bytes: [u8; 32] = Keccak256::digest(&spend_bytes).into();
    curve25519_dalek::scalar::Scalar::from_bytes_mod_order(view_bytes)
}

fn get_transaction_hex(tx_id: &str) -> String {
    let vectors_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/honked_bagpipe_rpc.json");

    let json_data =
        std::fs::read_to_string(&vectors_path).expect("failed to read test vectors file");

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
        as_hex: String,
        tx_hash: String,
    }

    let vectors: Vec<VectorCall> =
        serde_json::from_str(&json_data).expect("failed to parse test vectors JSON");

    for call in vectors {
        if call.route == "get_transactions" {
            let response: GetTransactionsResponse = serde_json::from_str(&call.response)
                .expect("failed to parse get_transactions response");

            for tx_info in response.txs {
                if tx_info.tx_hash == tx_id {
                    return tx_info.as_hex;
                }
            }
        }
    }

    panic!("transaction not found: {}", tx_id);
}
