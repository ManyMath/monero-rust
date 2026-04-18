#![cfg(feature = "oxide-adapter-spike")]

use monero_rust::{
    monero_backend::{block::Block, transaction::Transaction},
    oxide_adapter::{
        summarize_block, summarize_transaction, OxideAdapterError, OxideTimelockSummary,
    },
    scanner::compute_block_id,
};

#[cfg(feature = "oxide-wallet-adapter-spike")]
use monero_rust::{
    oxide_adapter::{scan_block_with_wallet, OxideNetwork},
    scanner::derive_keys,
};

#[derive(serde::Deserialize)]
struct PrunedTxFixture {
    as_hex_full: String,
    txid: String,
}

fn pruned_tx_fixture() -> PrunedTxFixture {
    serde_json::from_str(include_str!("vectors/pruned_tx_fixture_0.json"))
        .expect("pruned transaction fixture should parse")
}

fn honked_bagpipe_block_result() -> serde_json::Value {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("vectors/honked_bagpipe_rpc.json"))
            .expect("honked bagpipe RPC fixture should parse");

    let recorded = vectors
        .as_array()
        .expect("fixture should be an array")
        .iter()
        .find(|entry| entry["route"] == "json_rpc")
        .expect("get_block vector should be present");
    let response: serde_json::Value = serde_json::from_str(
        recorded["response"]
            .as_str()
            .expect("response should be a string"),
    )
    .expect("get_block response should parse");
    response["result"].clone()
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn tx_construction_miner_only_block_result() -> serde_json::Value {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("vectors/tx_construction_test_vectors.json"))
            .expect("tx construction fixture should parse");

    let recorded = vectors
        .as_array()
        .expect("fixture should be an array")
        .iter()
        .find(|entry| {
            entry["route"] == "json_rpc"
                && entry["body"]
                    .as_str()
                    .is_some_and(|body| body.contains("\"method\":\"get_block\""))
        })
        .expect("miner-only get_block vector should be present");
    let response: serde_json::Value = serde_json::from_str(
        recorded["response"]
            .as_str()
            .expect("response should be a string"),
    )
    .expect("get_block response should parse");
    response["result"].clone()
}

#[test]
fn oxide_transaction_summary_matches_current_backend_vector() {
    let fixture = pruned_tx_fixture();
    let bytes = hex::decode(&fixture.as_hex_full).expect("full transaction hex should decode");

    let current_tx = Transaction::read::<&[u8]>(&mut bytes.as_ref())
        .expect("current backend should parse full transaction");
    let summary =
        summarize_transaction(&bytes).expect("monero-oxide should parse full transaction");

    assert_eq!(summary.version, 2);
    assert_eq!(hex::encode(summary.hash), fixture.txid);
    assert_eq!(summary.hash, current_tx.hash());
    assert_eq!(summary.serialized_len, bytes.len());
    assert_eq!(summary.input_count, current_tx.prefix.inputs.len());
    assert_eq!(summary.output_count, current_tx.prefix.outputs.len());
    assert_eq!(summary.extra_len, current_tx.prefix.extra.len());
    assert_eq!(summary.miner_input_height, None);
    assert_eq!(summary.timelock, OxideTimelockSummary::None);
    assert!(summary.ring_member_count > 0);
}

#[test]
fn oxide_block_summary_matches_current_backend_vector() {
    let result = honked_bagpipe_block_result();
    let blob = hex::decode(result["blob"].as_str().expect("blob should be present"))
        .expect("block blob should decode");
    let current_block =
        Block::read::<&[u8]>(&mut blob.as_ref()).expect("current backend should parse block");

    let summary = summarize_block(&blob).expect("monero-oxide should parse block");

    assert_eq!(summary.height, 1_384_526);
    assert_eq!(
        hex::encode(summary.hash),
        result["block_header"]["hash"].as_str().unwrap()
    );
    assert_eq!(summary.hash, compute_block_id(&current_block));
    assert_eq!(
        hex::encode(summary.miner_tx_hash),
        result["miner_tx_hash"].as_str().unwrap()
    );
    assert_eq!(summary.miner_tx_hash, current_block.miner_tx.hash());
    assert_eq!(summary.transaction_count, 2);
    assert_eq!(summary.serialized_len, blob.len());
}

#[test]
fn oxide_transaction_summary_rejects_trailing_bytes() {
    let fixture = pruned_tx_fixture();
    let mut bytes = hex::decode(&fixture.as_hex_full).expect("full transaction hex should decode");
    bytes.push(0);

    let err = summarize_transaction(&bytes).expect_err("trailing bytes should be rejected");
    assert!(matches!(err, OxideAdapterError::TrailingBytes { .. }));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_scanner_runs_on_current_block_vector() {
    const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";

    let keys = derive_keys(HONKED_BAGPIPE_MNEMONIC, "stagenet", "")
        .expect("current backend should derive fixture wallet keys");
    let public_spend_key: [u8; 32] = hex::decode(&keys.public_spend_key)
        .expect("public spend key should decode")
        .try_into()
        .expect("public spend key should be 32 bytes");
    let private_view_key: [u8; 32] = hex::decode(&keys.secret_view_key)
        .expect("secret view key should decode")
        .try_into()
        .expect("secret view key should be 32 bytes");

    let result = tx_construction_miner_only_block_result();
    let blob = hex::decode(result["blob"].as_str().expect("blob should be present"))
        .expect("block blob should decode");

    let summary = scan_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &blob,
        &[],
        Some(0),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan miner-only block");

    assert_eq!(summary.legacy_address, keys.address);
    assert_eq!(summary.registered_subaddresses, 2);
    assert_eq!(
        summary.block_height,
        result["block_header"]["height"]
            .as_u64()
            .expect("block height should be present") as usize
    );
    assert_eq!(summary.scanned_output_count, 0);
}
