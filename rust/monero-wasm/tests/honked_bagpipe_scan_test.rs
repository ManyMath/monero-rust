//! Output scanning test using the honked bagpipe stagenet wallet.

use std::collections::HashSet;
use std::io::Cursor;
use zeroize::Zeroizing;
use monero_serai::{
    transaction::Transaction,
    wallet::{
        seed::Seed,
        address::{Network, AddressSpec},
        ViewPair, Scanner,
    },
};

const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const EXPECTED_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";
const TX_ID: &str = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";
const EXPECTED_AMOUNT: u64 = 10_000_000_000_000;

#[derive(serde::Deserialize)]
struct RpcCall {
    route: String,
    #[allow(dead_code)]
    body: String,
    response: String,
    #[allow(dead_code)]
    is_binary: bool,
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

fn load_test_vectors() -> Vec<RpcCall> {
    let vectors_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/honked_bagpipe_rpc.json");

    let json_data = std::fs::read_to_string(&vectors_path)
        .expect("failed to read test vectors file");

    serde_json::from_str(&json_data)
        .expect("failed to parse test vectors JSON")
}

fn get_transaction_info(tx_id: &str) -> (String, String) {
    let vectors = load_test_vectors();

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

#[test]
fn test_address_derivation() {
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend, Zeroizing::new(view));
    let address = pair.address(Network::Stagenet, AddressSpec::Standard);

    assert_eq!(address.to_string(), EXPECTED_ADDRESS);
}

#[test]
fn test_scan_actual_transaction() {
    let seed = Seed::from_string(Zeroizing::new(HONKED_BAGPIPE_MNEMONIC.to_string()))
        .expect("valid mnemonic");

    let spend = spend_key_from_seed(&seed);
    let view = view_key_from_seed(&seed);

    let pair = ViewPair::new(spend, Zeroizing::new(view));
    let address = pair.address(Network::Stagenet, AddressSpec::Standard);
    assert_eq!(address.to_string(), EXPECTED_ADDRESS);

    let mut scanner = Scanner::from_view(pair, Some(HashSet::new()));

    let (tx_hex, prunable_hash_hex) = get_transaction_info(TX_ID);

    let tx_bytes = hex::decode(&tx_hex).expect("failed to decode transaction hex");
    let prunable_hash = hex::decode(&prunable_hash_hex).expect("failed to decode prunable hash hex");
    let mut prunable_hash_array = [0u8; 32];
    prunable_hash_array.copy_from_slice(&prunable_hash);

    let mut cursor = Cursor::new(&tx_bytes);
    let transaction = parse_pruned_transaction(&mut cursor, prunable_hash_array)
        .expect("failed to parse pruned transaction");

    let scan_result = scanner.scan_transaction(&transaction);
    let outputs = scan_result.ignore_timelock();

    assert_eq!(outputs.len(), 1);
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
    use monero_serai::hash_to_scalar;

    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    hash_to_scalar(&spend_bytes)
}

fn parse_pruned_transaction<R: std::io::Read>(r: &mut R, _prunable_hash: [u8; 32]) -> std::io::Result<Transaction> {
    use monero_serai::{
        transaction::TransactionPrefix,
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
