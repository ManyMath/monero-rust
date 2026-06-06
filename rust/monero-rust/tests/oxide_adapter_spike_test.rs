#![cfg(feature = "oxide-adapter-spike")]

use monero_rust::{
    monero_backend::{block::Block, transaction::Input as CurrentInput, transaction::Transaction},
    oxide_adapter::{
        summarize_block, summarize_transaction, OxideAdapterError, OxideTimelockSummary,
    },
    scanner::compute_block_id,
};

#[cfg(feature = "oxide-wallet-adapter-spike")]
use monero_rust::monero_backend::rpc::{BlockCompleteEntry, BlockOutputIndices, TxOutputIndices};
#[cfg(feature = "oxide-wallet-adapter-spike")]
use monero_rust::monero_backend::wallet::{
    address::SubaddressIndex as CurrentSubaddressIndex, Scanner as CurrentScanner,
    ViewPair as CurrentViewPair,
};
#[cfg(feature = "oxide-wallet-adapter-spike")]
use monero_rust::{
    oxide_adapter::{
        derive_oxide_wallet_output_key_image, infer_first_ringct_output_index,
        oxide_wallet_summaries_to_multi_wallet_scan_result,
        oxide_wallet_summary_to_block_scan_result, scan_block_with_wallet,
        scan_rpc_block_with_wallet, scan_rpc_blocks_with_wallet,
        scan_validated_rpc_blocks_as_block_scan_results,
        scan_validated_rpc_blocks_as_multi_wallet_scan_results,
        scan_validated_rpc_blocks_with_wallet, validate_scan_summary_chain, OxideNetwork,
        OxideWalletScanConfig,
    },
    process_single_wallet_batch,
    scanner::derive_keys,
};

#[cfg(feature = "oxide-wallet-adapter-spike")]
use curve25519_dalek::{
    edwards::CompressedEdwardsY as CurrentCompressedEdwardsY, scalar::Scalar as CurrentScalar,
};
#[cfg(feature = "oxide-wallet-adapter-spike")]
use std::collections::HashSet;
#[cfg(feature = "oxide-wallet-adapter-spike")]
use zeroize::Zeroizing;

#[cfg(feature = "oxide-wallet-adapter-spike")]
const HONKED_BAGPIPE_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
#[cfg(feature = "oxide-wallet-adapter-spike")]
const HEMLOCK_MNEMONIC: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";

#[derive(serde::Deserialize)]
struct PrunedTxFixture {
    as_hex_full: String,
    pruned_as_hex: String,
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

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn tx_construction_miner_output_index() -> u64 {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("vectors/tx_construction_test_vectors.json"))
            .expect("tx construction fixture should parse");

    let recorded = vectors
        .as_array()
        .expect("fixture should be an array")
        .iter()
        .find(|entry| entry["route"] == "get_transactions")
        .expect("get_transactions vector should be present");
    let response: serde_json::Value = serde_json::from_str(
        recorded["response"]
            .as_str()
            .expect("response should be a string"),
    )
    .expect("get_transactions response should parse");
    response["txs"][0]["output_indices"][0]
        .as_u64()
        .expect("miner output index should be present")
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[derive(serde::Deserialize)]
struct RpcCall {
    route: String,
    response: String,
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[derive(serde::Deserialize)]
struct GetTransactionsResponse {
    txs: Vec<HonkedTxInfo>,
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[derive(Clone, serde::Deserialize)]
struct HonkedTxInfo {
    as_hex: String,
    tx_hash: String,
    output_indices: Vec<u64>,
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn honked_bagpipe_transactions_for_block(result: &serde_json::Value) -> Vec<HonkedTxInfo> {
    let vectors: Vec<RpcCall> =
        serde_json::from_str(include_str!("vectors/honked_bagpipe_rpc.json"))
            .expect("honked bagpipe RPC fixture should parse");
    let transactions_response = vectors
        .into_iter()
        .find(|entry| entry.route == "get_transactions")
        .expect("get_transactions vector should be present");
    let transactions: GetTransactionsResponse =
        serde_json::from_str(&transactions_response.response)
            .expect("get_transactions response should parse");

    result["tx_hashes"]
        .as_array()
        .expect("block tx hashes should be present")
        .iter()
        .map(|hash| {
            let hash = hash.as_str().expect("tx hash should be a string");
            transactions
                .txs
                .iter()
                .find(|tx| tx.tx_hash == hash)
                .unwrap_or_else(|| panic!("transaction {} should be present", hash))
                .clone()
        })
        .collect()
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn current_view_pair_from_keys(
    public_spend_key: [u8; 32],
    private_view_key: [u8; 32],
) -> CurrentViewPair {
    let spend = CurrentCompressedEdwardsY(public_spend_key)
        .decompress()
        .expect("current backend should accept public spend key");
    let view = CurrentScalar::from_bytes_mod_order(private_view_key);
    CurrentViewPair::new(spend, Zeroizing::new(view))
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn current_subaddress_tuple(subaddress: Option<CurrentSubaddressIndex>) -> Option<(u32, u32)> {
    subaddress.map(|subaddress| (subaddress.account(), subaddress.address()))
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn honked_rpc_block_entry_and_indices() -> (BlockCompleteEntry, BlockOutputIndices) {
    let result = honked_bagpipe_block_result();
    let transactions = honked_bagpipe_transactions_for_block(&result);
    let block_weight = result["block_header"]["block_weight"]
        .as_u64()
        .expect("block weight should be present");
    (
        BlockCompleteEntry {
            block: hex::decode(result["blob"].as_str().expect("blob should be present"))
                .expect("block blob should decode"),
            txs: transactions
                .iter()
                .map(|tx| hex::decode(&tx.as_hex).expect("transaction hex should decode"))
                .collect(),
            pruned: false,
            block_weight,
        },
        BlockOutputIndices {
            indices: transactions
                .iter()
                .map(|tx| TxOutputIndices {
                    indices: tx.output_indices.clone(),
                })
                .collect(),
        },
    )
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn miner_only_rpc_block_entry_and_indices() -> (BlockCompleteEntry, BlockOutputIndices) {
    let result = tx_construction_miner_only_block_result();
    let block_weight = result["block_header"]["block_weight"]
        .as_u64()
        .expect("block weight should be present");
    (
        BlockCompleteEntry {
            block: hex::decode(result["blob"].as_str().expect("blob should be present"))
                .expect("block blob should decode"),
            txs: vec![],
            pruned: false,
            block_weight,
        },
        BlockOutputIndices { indices: vec![] },
    )
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn honked_wallet_key_bytes() -> ([u8; 32], [u8; 32], String) {
    wallet_key_bytes_from_mnemonic(HONKED_BAGPIPE_MNEMONIC)
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn wallet_key_bytes_from_mnemonic(mnemonic: &str) -> ([u8; 32], [u8; 32], String) {
    let keys = derive_keys(mnemonic, "stagenet", "")
        .expect("current backend should derive fixture wallet keys");
    let public_spend_key: [u8; 32] = hex::decode(&keys.public_spend_key)
        .expect("public spend key should decode")
        .try_into()
        .expect("public spend key should be 32 bytes");
    let private_view_key: [u8; 32] = hex::decode(&keys.secret_view_key)
        .expect("secret view key should decode")
        .try_into()
        .expect("secret view key should be 32 bytes");
    (public_spend_key, private_view_key, keys.address.clone())
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
    let miner_output_index = tx_construction_miner_output_index();

    let summary = scan_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &blob,
        &[],
        Some(miner_output_index),
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
    assert_eq!(
        summary.block_timestamp,
        result["block_header"]["timestamp"]
            .as_u64()
            .expect("block timestamp should be present")
    );
    assert_eq!(
        hex::encode(summary.block_hash),
        result["block_header"]["hash"]
            .as_str()
            .expect("block hash should be present")
    );
    assert_eq!(
        hex::encode(summary.previous_block_hash),
        result["block_header"]["prev_hash"]
            .as_str()
            .expect("previous block hash should be present")
    );
    assert_eq!(summary.rpc_pruned, None);
    assert_eq!(summary.rpc_block_weight, None);
    assert!(summary.transaction_hashes.is_empty());
    assert!(summary.spent_key_images.is_empty());
    assert_eq!(summary.scanned_output_count, 1);
    assert_eq!(summary.outputs.len(), 1);

    let output = &summary.outputs[0];
    assert_eq!(
        hex::encode(output.transaction),
        result["miner_tx_hash"]
            .as_str()
            .expect("miner tx hash should be present")
    );
    assert_eq!(output.block_height, summary.block_height);
    assert!(output.is_coinbase);
    assert_eq!(output.index_in_transaction, 0);
    assert_eq!(output.index_on_blockchain, miner_output_index);
    assert_eq!(
        output.amount,
        result["block_header"]["reward"]
            .as_u64()
            .expect("block reward should be present")
    );
    assert_eq!(
        output.additional_timelock,
        OxideTimelockSummary::Block(1_386_923)
    );
    assert_eq!(output.subaddress, None);
    assert_eq!(output.payment_id, None);
    assert!(!output.oxide_received_output_bytes.is_empty());
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_scanner_matches_current_backend_output() {
    const TARGET_TX_ID: &str = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";
    const EXPECTED_AMOUNT: u64 = 10_000_000_000_000;

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
    let private_spend_key: [u8; 32] = hex::decode(&keys.secret_spend_key)
        .expect("secret spend key should decode")
        .try_into()
        .expect("secret spend key should be 32 bytes");

    let result = honked_bagpipe_block_result();
    let blob = hex::decode(result["blob"].as_str().expect("blob should be present"))
        .expect("block blob should decode");
    let transactions = honked_bagpipe_transactions_for_block(&result);
    let transaction_blobs = transactions
        .iter()
        .map(|tx| hex::decode(&tx.as_hex).expect("transaction hex should decode"))
        .collect::<Vec<_>>();
    let transaction_blob_refs = transaction_blobs
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    let transaction_output_index_refs = transactions
        .iter()
        .map(|tx| tx.output_indices.as_slice())
        .collect::<Vec<_>>();
    let first_ringct_output_index =
        infer_first_ringct_output_index(&blob, &transaction_output_index_refs)
            .expect("first RingCT output index should be inferred")
            .expect("block should contain RingCT output indices");

    assert_eq!(first_ringct_output_index, 6_693_928);

    let summary = scan_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &blob,
        &transaction_blob_refs,
        Some(first_ringct_output_index),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan block transactions");

    let target_tx = transactions
        .iter()
        .find(|tx| tx.tx_hash == TARGET_TX_ID)
        .expect("target transaction should be present");
    let target_tx_bytes = hex::decode(&target_tx.as_hex).expect("target transaction should decode");
    let current_tx = Transaction::read::<&[u8]>(&mut target_tx_bytes.as_ref())
        .expect("current backend should parse target transaction");
    let current_pair = current_view_pair_from_keys(public_spend_key, private_view_key);
    let mut current_scanner = CurrentScanner::from_view(current_pair, Some(HashSet::new()));
    let current_outputs = current_scanner
        .scan_transaction(&current_tx)
        .ignore_timelock();

    assert_eq!(current_outputs.len(), 1);
    let current_block =
        Block::read::<&[u8]>(&mut blob.as_ref()).expect("current backend should parse block");
    assert_eq!(summary.block_hash, compute_block_id(&current_block));
    assert_eq!(summary.previous_block_hash, current_block.header.previous);
    assert_eq!(summary.rpc_pruned, None);
    assert_eq!(summary.rpc_block_weight, None);
    assert_eq!(
        summary.block_timestamp,
        result["block_header"]["timestamp"]
            .as_u64()
            .expect("block timestamp should be present")
    );
    assert_eq!(summary.transaction_count, transactions.len() + 1);
    assert_eq!(
        summary
            .transaction_hashes
            .iter()
            .map(hex::encode)
            .collect::<Vec<_>>(),
        result["tx_hashes"]
            .as_array()
            .expect("block tx hashes should be present")
            .iter()
            .map(|hash| hash
                .as_str()
                .expect("tx hash should be a string")
                .to_string())
            .collect::<Vec<_>>()
    );
    let mut expected_spent_key_images = Vec::new();
    for tx in &transactions {
        let tx_bytes = hex::decode(&tx.as_hex).expect("transaction should decode");
        let current_tx = Transaction::read::<&[u8]>(&mut tx_bytes.as_ref())
            .expect("current backend should parse transaction");
        for input in &current_tx.prefix.inputs {
            if let CurrentInput::ToKey { key_image, .. } = input {
                expected_spent_key_images.push((
                    tx.tx_hash.clone(),
                    hex::encode(key_image.compress().to_bytes()),
                ));
            }
        }
    }
    let oxide_spent_key_images = summary
        .spent_key_images
        .iter()
        .map(|spent| (hex::encode(spent.transaction), hex::encode(spent.key_image)))
        .collect::<Vec<_>>();
    assert_eq!(oxide_spent_key_images, expected_spent_key_images);
    assert_eq!(summary.scanned_output_count, current_outputs.len());
    assert_eq!(summary.outputs.len(), current_outputs.len());

    let current_output = &current_outputs[0];
    let oxide_output = &summary.outputs[0];
    assert_eq!(hex::encode(oxide_output.transaction), TARGET_TX_ID);
    assert_eq!(oxide_output.block_height, summary.block_height);
    assert!(!oxide_output.is_coinbase);
    assert_eq!(
        oxide_output.index_in_transaction,
        u64::from(current_output.absolute.o)
    );
    assert_eq!(
        oxide_output.index_on_blockchain,
        target_tx.output_indices[oxide_output.index_in_transaction as usize]
    );
    assert_eq!(oxide_output.amount, EXPECTED_AMOUNT);
    assert_eq!(oxide_output.additional_timelock, OxideTimelockSummary::None);
    assert_eq!(oxide_output.amount, current_output.data.commitment.amount);
    assert_eq!(
        oxide_output.key,
        current_output.data.key.compress().to_bytes()
    );
    assert_eq!(
        oxide_output.key_offset,
        current_output.data.key_offset.to_bytes()
    );
    assert_eq!(
        oxide_output.commitment_mask,
        current_output.data.commitment.mask.to_bytes()
    );
    assert_eq!(
        oxide_output.subaddress,
        current_subaddress_tuple(current_output.metadata.subaddress)
    );
    assert_eq!(oxide_output.payment_id, None);
    assert!(!oxide_output.oxide_received_output_bytes.is_empty());
    assert_ne!(
        oxide_output.oxide_received_output_bytes,
        current_output.serialize()
    );

    let current_spend = CurrentScalar::from_bytes_mod_order(private_spend_key);
    let current_key_image = monero_rust::monero_backend::ringct::generate_key_image(
        &Zeroizing::new(current_spend + current_output.data.key_offset),
    )
    .compress()
    .to_bytes();
    let oxide_key_image = derive_oxide_wallet_output_key_image(private_spend_key, oxide_output)
        .expect("correct spend key should derive oxide output key image");
    assert_eq!(oxide_key_image, current_key_image);

    let mut wrong_spend_key = private_spend_key;
    wrong_spend_key[0] ^= 1;
    let err = derive_oxide_wallet_output_key_image(wrong_spend_key, oxide_output)
        .expect_err("wrong spend key should not own oxide output");
    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_rpc_block_expansion_matches_current_backend_output() {
    const TARGET_TX_ID: &str = "07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5";
    const EXPECTED_AMOUNT: u64 = 10_000_000_000_000;

    let (public_spend_key, private_view_key, address) = honked_wallet_key_bytes();
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let result = honked_bagpipe_block_result();

    let summary = scan_rpc_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan RPC-expanded block");

    assert_eq!(summary.legacy_address, address);
    assert_eq!(summary.block_height, 1_384_526);
    assert_eq!(summary.transaction_count, 3);
    assert_eq!(
        summary
            .transaction_hashes
            .iter()
            .map(hex::encode)
            .collect::<Vec<_>>(),
        result["tx_hashes"]
            .as_array()
            .expect("block tx hashes should be present")
            .iter()
            .map(|hash| hash
                .as_str()
                .expect("tx hash should be a string")
                .to_string())
            .collect::<Vec<_>>()
    );
    assert!(!summary.spent_key_images.is_empty());
    assert_eq!(
        hex::encode(summary.block_hash),
        result["block_header"]["hash"]
            .as_str()
            .expect("block hash should be present")
    );
    assert_eq!(
        hex::encode(summary.previous_block_hash),
        result["block_header"]["prev_hash"]
            .as_str()
            .expect("previous block hash should be present")
    );
    assert_eq!(summary.rpc_pruned, Some(false));
    assert_eq!(
        summary.rpc_block_weight,
        Some(
            result["block_header"]["block_weight"]
                .as_u64()
                .expect("block weight should be present")
        )
    );
    assert_eq!(summary.scanned_output_count, 1);
    assert_eq!(summary.outputs.len(), 1);
    assert_eq!(hex::encode(summary.outputs[0].transaction), TARGET_TX_ID);
    assert_eq!(summary.outputs[0].block_height, summary.block_height);
    assert!(!summary.outputs[0].is_coinbase);
    assert_eq!(summary.outputs[0].index_in_transaction, 1);
    assert_eq!(summary.outputs[0].index_on_blockchain, 6_693_930);
    assert_eq!(summary.outputs[0].amount, EXPECTED_AMOUNT);
    assert_eq!(
        summary.outputs[0].additional_timelock,
        OxideTimelockSummary::None
    );
    assert_eq!(summary.outputs[0].subaddress, None);
    assert_eq!(summary.outputs[0].payment_id, None);
    assert_eq!(summary.outputs[0].key.len(), 32);
    assert_eq!(summary.outputs[0].key_offset.len(), 32);
    assert_eq!(summary.outputs[0].commitment_mask.len(), 32);
    assert!(!summary.outputs[0].oxide_received_output_bytes.is_empty());
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_summary_maps_to_current_block_scan_result() {
    const EXPECTED_AMOUNT: u64 = 10_000_000_000_000;

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
    let private_spend_key: [u8; 32] = hex::decode(&keys.secret_spend_key)
        .expect("secret spend key should decode")
        .try_into()
        .expect("secret spend key should be 32 bytes");
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let result = honked_bagpipe_block_result();

    let summary = scan_rpc_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan RPC-expanded block");
    let daemon_height = summary.block_height as u64 + 100;

    let block_result =
        oxide_wallet_summary_to_block_scan_result(&summary, Some(private_spend_key), daemon_height)
            .expect("oxide scan summary should map to current scan result");

    assert_eq!(block_result.block_height, summary.block_height as u64);
    assert_eq!(block_result.block_hash, hex::encode(summary.block_hash));
    assert_eq!(block_result.block_timestamp, summary.block_timestamp);
    assert_eq!(block_result.tx_count, summary.transaction_count);
    assert_eq!(block_result.daemon_height, daemon_height);
    assert_eq!(
        block_result.spent_key_images,
        summary
            .spent_key_images
            .iter()
            .map(|spent| hex::encode(spent.key_image))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        block_result.spent_key_image_tx_hashes,
        summary
            .spent_key_images
            .iter()
            .map(|spent| hex::encode(spent.transaction))
            .collect::<Vec<_>>()
    );

    assert_eq!(block_result.outputs.len(), 1);
    let oxide_output = &summary.outputs[0];
    let output = &block_result.outputs[0];
    assert_eq!(output.tx_hash, hex::encode(oxide_output.transaction));
    assert_eq!(output.output_index, oxide_output.index_in_transaction as u8);
    assert_eq!(output.amount, EXPECTED_AMOUNT);
    assert_eq!(output.amount_xmr, "10.000000000000");
    assert_eq!(output.key, hex::encode(oxide_output.key));
    assert_eq!(output.key_offset, hex::encode(oxide_output.key_offset));
    assert_eq!(
        output.commitment_mask,
        hex::encode(oxide_output.commitment_mask)
    );
    assert_eq!(output.subaddress_index, oxide_output.subaddress);
    assert_eq!(output.payment_id, oxide_output.payment_id);
    assert_eq!(
        output.received_output_bytes,
        hex::encode(&oxide_output.oxide_received_output_bytes)
    );
    assert_eq!(output.block_height, summary.block_height as u64);
    assert!(!output.spent);
    assert_eq!(output.spent_height, None);
    assert!(!output.is_coinbase);
    assert!(!output.frozen);
    assert_eq!(
        output.key_image,
        hex::encode(
            derive_oxide_wallet_output_key_image(private_spend_key, oxide_output)
                .expect("mapped output should derive key image")
        )
    );

    let batch = process_single_wallet_batch(
        std::slice::from_ref(&block_result),
        Some(&[0]),
        daemon_height,
        block_result.block_height,
    );
    assert_eq!(batch.outputs_to_store.len(), 1);
    assert_eq!(batch.outputs_to_store[0].tx_hash, output.tx_hash);
    assert_eq!(batch.blocks_with_outputs.len(), 1);
    assert_eq!(
        batch.block_hashes,
        vec![(block_result.block_height, block_result.block_hash)]
    );
    assert_eq!(
        batch.spent_key_images,
        summary
            .spent_key_images
            .iter()
            .map(|spent| hex::encode(spent.key_image))
            .collect::<Vec<_>>()
    );

    assert_eq!(
        result["block_header"]["hash"]
            .as_str()
            .expect("block hash should be present"),
        batch.block_hashes[0].1
    );
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_rpc_block_expansion_matches_current_backend_for_unmatched_wallet() {
    let (public_spend_key, private_view_key, address) =
        wallet_key_bytes_from_mnemonic(HEMLOCK_MNEMONIC);
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let result = honked_bagpipe_block_result();

    let summary = scan_rpc_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan RPC-expanded block for unmatched wallet");

    let current_pair = current_view_pair_from_keys(public_spend_key, private_view_key);
    let mut current_scanner = CurrentScanner::from_view(current_pair, Some(HashSet::new()));
    let current_output_count = block_entry
        .txs
        .iter()
        .map(|tx_bytes| {
            let tx = Transaction::read::<&[u8]>(&mut tx_bytes.as_slice())
                .expect("current backend should parse fixture transaction");
            current_scanner
                .scan_transaction(&tx)
                .ignore_timelock()
                .len()
        })
        .sum::<usize>();

    assert_eq!(summary.legacy_address, address);
    assert_eq!(summary.block_height, 1_384_526);
    assert_eq!(summary.transaction_count, 3);
    assert_eq!(
        summary
            .transaction_hashes
            .iter()
            .map(hex::encode)
            .collect::<Vec<_>>(),
        result["tx_hashes"]
            .as_array()
            .expect("block tx hashes should be present")
            .iter()
            .map(|hash| hash
                .as_str()
                .expect("tx hash should be a string")
                .to_string())
            .collect::<Vec<_>>()
    );
    assert!(!summary.spent_key_images.is_empty());
    assert_eq!(summary.scanned_output_count, current_output_count);
    assert_eq!(current_output_count, 0);
    assert!(summary.outputs.is_empty());
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_rpc_block_expansion_rejects_missing_transaction_blob() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (mut block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    block_entry.txs.pop();

    let err = scan_rpc_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect_err("RPC expansion should reject an incomplete transaction list");

    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_direct_block_scan_rejects_missing_transaction_blob() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let result = honked_bagpipe_block_result();
    let blob = hex::decode(result["blob"].as_str().expect("blob should be present"))
        .expect("block blob should decode");
    let mut transactions = honked_bagpipe_transactions_for_block(&result);
    transactions.pop();
    let transaction_blobs = transactions
        .iter()
        .map(|tx| hex::decode(&tx.as_hex).expect("transaction hex should decode"))
        .collect::<Vec<_>>();
    let transaction_blob_refs = transaction_blobs
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();

    let err = scan_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &blob,
        &transaction_blob_refs,
        None,
        &[(0, 1), (1, 0)],
    )
    .expect_err("direct block scan should reject an incomplete transaction list");

    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_rpc_block_expansion_rejects_mismatched_transaction_blob() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (mut block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    block_entry.txs.swap(0, 1);

    let err = scan_rpc_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect_err("RPC expansion should reject transaction blobs with mismatched hashes");

    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_rpc_block_expansion_rejects_pruned_blob_for_unpruned_entry() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (mut block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let fixture = pruned_tx_fixture();
    block_entry.txs[0] =
        hex::decode(fixture.pruned_as_hex).expect("pruned fixture transaction should decode");
    block_entry.pruned = false;

    let err = scan_rpc_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect_err("unpruned RPC expansion should reject pruned transaction blobs");

    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_rpc_batch_scans_miner_and_matching_blocks() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (miner_entry, miner_indices) = miner_only_rpc_block_entry_and_indices();
    let (honked_entry, honked_indices) = honked_rpc_block_entry_and_indices();
    let honked_result = honked_bagpipe_block_result();

    let summaries = scan_rpc_blocks_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &[miner_entry, honked_entry],
        &[miner_indices, honked_indices],
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan RPC block sequence");

    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].scanned_output_count, 0);
    assert_eq!(summaries[0].rpc_pruned, Some(false));
    assert_eq!(summaries[0].rpc_block_weight, Some(85));
    assert!(summaries[0].outputs.is_empty());
    assert_eq!(summaries[1].block_height, 1_384_526);
    assert_eq!(
        hex::encode(summaries[1].previous_block_hash),
        honked_result["block_header"]["prev_hash"]
            .as_str()
            .expect("previous block hash should be present")
    );
    assert_eq!(summaries[1].scanned_output_count, 1);
    assert_eq!(summaries[1].rpc_pruned, Some(false));
    assert_eq!(summaries[1].rpc_block_weight, Some(4_487));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_scan_summary_chain_validates_single_parent_boundary() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let honked_result = honked_bagpipe_block_result();
    let expected_parent_hash: [u8; 32] = hex::decode(
        honked_result["block_header"]["prev_hash"]
            .as_str()
            .expect("previous block hash should be present"),
    )
    .expect("previous block hash should decode")
    .try_into()
    .expect("previous block hash should be 32 bytes");

    let summary = scan_rpc_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan RPC-expanded block");

    validate_scan_summary_chain(None, &[]).expect("empty scan summary chains are valid");
    validate_scan_summary_chain(Some(expected_parent_hash), std::slice::from_ref(&summary))
        .expect("single scan summary should match expected parent hash");

    let mut wrong_parent_hash = expected_parent_hash;
    wrong_parent_hash[0] ^= 1;
    let err = validate_scan_summary_chain(Some(wrong_parent_hash), std::slice::from_ref(&summary))
        .expect_err("wrong expected parent hash should be rejected");
    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_scan_summary_chain_rejects_unrelated_fixture_batch() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (miner_entry, miner_indices) = miner_only_rpc_block_entry_and_indices();
    let (honked_entry, honked_indices) = honked_rpc_block_entry_and_indices();

    let summaries = scan_rpc_blocks_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &[miner_entry, honked_entry],
        &[miner_indices, honked_indices],
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan RPC block sequence");

    let err = validate_scan_summary_chain(None, &summaries)
        .expect_err("unrelated block fixtures should not validate as one chain");
    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_scan_summary_chain_rejects_height_gap() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();

    let first = scan_rpc_block_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan RPC-expanded block");
    let mut second = first.clone();
    second.block_height = first.block_height + 2;
    second.previous_block_hash = first.block_hash;

    let err = validate_scan_summary_chain(None, &[first, second])
        .expect_err("hash-linked summaries with a height gap should be rejected");
    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_validated_rpc_batch_accepts_expected_single_parent() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let honked_result = honked_bagpipe_block_result();
    let expected_parent_hash: [u8; 32] = hex::decode(
        honked_result["block_header"]["prev_hash"]
            .as_str()
            .expect("previous block hash should be present"),
    )
    .expect("previous block hash should decode")
    .try_into()
    .expect("previous block hash should be 32 bytes");

    let summaries = scan_validated_rpc_blocks_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &[block_entry],
        &[output_indices],
        &[(0, 1), (1, 0)],
        Some(expected_parent_hash),
    )
    .expect("validated RPC scan should accept matching starting parent");

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].block_height, 1_384_526);
    assert_eq!(summaries[0].scanned_output_count, 1);
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_validated_rpc_batch_maps_to_current_block_scan_results() {
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
    let private_spend_key: [u8; 32] = hex::decode(&keys.secret_spend_key)
        .expect("secret spend key should decode")
        .try_into()
        .expect("secret spend key should be 32 bytes");
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let honked_result = honked_bagpipe_block_result();
    let expected_parent_hash: [u8; 32] = hex::decode(
        honked_result["block_header"]["prev_hash"]
            .as_str()
            .expect("previous block hash should be present"),
    )
    .expect("previous block hash should decode")
    .try_into()
    .expect("previous block hash should be 32 bytes");
    let daemon_height = honked_result["block_header"]["height"]
        .as_u64()
        .expect("block height should be present")
        + 100;

    let block_results = scan_validated_rpc_blocks_as_block_scan_results(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        std::slice::from_ref(&block_entry),
        std::slice::from_ref(&output_indices),
        &[(0, 1), (1, 0)],
        Some(expected_parent_hash),
        Some(private_spend_key),
        daemon_height,
    )
    .expect("validated oxide RPC scan should map into current block scan results");

    assert_eq!(block_results.len(), 1);
    let block_result = &block_results[0];
    assert_eq!(block_result.block_height, 1_384_526);
    assert_eq!(
        block_result.block_hash,
        honked_result["block_header"]["hash"]
            .as_str()
            .expect("block hash should be present")
    );
    assert_eq!(block_result.daemon_height, daemon_height);
    assert_eq!(block_result.outputs.len(), 1);
    assert!(!block_result.outputs[0].key_image.is_empty());
    assert_eq!(block_result.spent_key_images.len(), 4);
    assert_eq!(
        block_result.spent_key_images.len(),
        block_result.spent_key_image_tx_hashes.len()
    );
    assert_eq!(
        block_result
            .spent_key_image_tx_hashes
            .iter()
            .collect::<HashSet<_>>()
            .len(),
        2
    );

    let batch = process_single_wallet_batch(
        &block_results,
        Some(&[0]),
        daemon_height,
        block_result.block_height,
    );
    assert_eq!(batch.outputs_to_store.len(), 1);
    assert_eq!(batch.block_hashes.len(), 1);
    assert_eq!(batch.block_hashes[0].1, block_result.block_hash);

    let mut wrong_parent_hash = expected_parent_hash;
    wrong_parent_hash[0] ^= 1;
    let err = scan_validated_rpc_blocks_as_block_scan_results(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &[block_entry],
        &[output_indices],
        &[(0, 1), (1, 0)],
        Some(wrong_parent_hash),
        Some(private_spend_key),
        daemon_height,
    )
    .expect_err("validated oxide RPC scan should reject a wrong parent before mapping");
    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_summaries_map_to_current_multi_wallet_scan_result() {
    let keys = derive_keys(HONKED_BAGPIPE_MNEMONIC, "stagenet", "")
        .expect("current backend should derive fixture wallet keys");
    let honked_public_spend_key: [u8; 32] = hex::decode(&keys.public_spend_key)
        .expect("public spend key should decode")
        .try_into()
        .expect("public spend key should be 32 bytes");
    let honked_private_view_key: [u8; 32] = hex::decode(&keys.secret_view_key)
        .expect("secret view key should decode")
        .try_into()
        .expect("secret view key should be 32 bytes");
    let honked_private_spend_key: [u8; 32] = hex::decode(&keys.secret_spend_key)
        .expect("secret spend key should decode")
        .try_into()
        .expect("secret spend key should be 32 bytes");
    let (hemlock_public_spend_key, hemlock_private_view_key, hemlock_address) =
        wallet_key_bytes_from_mnemonic(HEMLOCK_MNEMONIC);
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let honked_result = honked_bagpipe_block_result();
    let daemon_height = honked_result["block_header"]["height"]
        .as_u64()
        .expect("block height should be present")
        + 100;

    let honked_summary = scan_rpc_block_with_wallet(
        honked_public_spend_key,
        honked_private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan matching wallet");
    let hemlock_summary = scan_rpc_block_with_wallet(
        hemlock_public_spend_key,
        hemlock_private_view_key,
        OxideNetwork::Stagenet,
        &block_entry,
        Some(&output_indices),
        &[(0, 1), (1, 0)],
    )
    .expect("monero-wallet scanner should scan unmatched wallet");

    let multi_result = oxide_wallet_summaries_to_multi_wallet_scan_result(
        &[honked_summary.clone(), hemlock_summary.clone()],
        &[Some(honked_private_spend_key), None],
        daemon_height,
    )
    .expect("same-block oxide scan summaries should map to multi-wallet result");

    assert_eq!(
        multi_result.block_height,
        honked_summary.block_height as u64
    );
    assert_eq!(
        multi_result.block_hash,
        hex::encode(honked_summary.block_hash)
    );
    assert_eq!(multi_result.block_timestamp, honked_summary.block_timestamp);
    assert_eq!(multi_result.tx_count, honked_summary.transaction_count);
    assert_eq!(multi_result.daemon_height, daemon_height);
    assert_eq!(
        multi_result.spent_key_images,
        honked_summary
            .spent_key_images
            .iter()
            .map(|spent| hex::encode(spent.key_image))
            .collect::<Vec<_>>()
    );
    assert_eq!(multi_result.wallet_results.len(), 2);

    let honked_wallet = multi_result
        .wallet_results
        .get(&keys.address)
        .expect("matching wallet result should be keyed by address");
    assert_eq!(honked_wallet.address, keys.address);
    assert_eq!(honked_wallet.outputs.len(), 1);
    assert_eq!(honked_wallet.outputs[0].amount, 10_000_000_000_000);
    assert_eq!(
        honked_wallet.outputs[0].key_image,
        hex::encode(
            derive_oxide_wallet_output_key_image(
                honked_private_spend_key,
                &honked_summary.outputs[0]
            )
            .expect("mapped full-wallet output should derive key image")
        )
    );

    let hemlock_wallet = multi_result
        .wallet_results
        .get(&hemlock_address)
        .expect("unmatched wallet result should be keyed by address");
    assert_eq!(hemlock_wallet.address, hemlock_address);
    assert!(hemlock_wallet.outputs.is_empty());

    let err = oxide_wallet_summaries_to_multi_wallet_scan_result(
        std::slice::from_ref(&honked_summary),
        &[],
        daemon_height,
    )
    .expect_err("parallel spend-key metadata is required");
    assert!(matches!(err, OxideAdapterError::Parse(_)));

    let mut wrong_block_summary = hemlock_summary;
    wrong_block_summary.block_height += 1;
    let err = oxide_wallet_summaries_to_multi_wallet_scan_result(
        &[honked_summary, wrong_block_summary],
        &[Some(honked_private_spend_key), None],
        daemon_height,
    )
    .expect_err("multi-wallet mapping should reject summaries from different blocks");
    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_validated_rpc_batch_maps_to_current_multi_wallet_scan_results() {
    let keys = derive_keys(HONKED_BAGPIPE_MNEMONIC, "stagenet", "")
        .expect("current backend should derive fixture wallet keys");
    let honked_public_spend_key: [u8; 32] = hex::decode(&keys.public_spend_key)
        .expect("public spend key should decode")
        .try_into()
        .expect("public spend key should be 32 bytes");
    let honked_private_view_key: [u8; 32] = hex::decode(&keys.secret_view_key)
        .expect("secret view key should decode")
        .try_into()
        .expect("secret view key should be 32 bytes");
    let honked_private_spend_key: [u8; 32] = hex::decode(&keys.secret_spend_key)
        .expect("secret spend key should decode")
        .try_into()
        .expect("secret spend key should be 32 bytes");
    let (hemlock_public_spend_key, hemlock_private_view_key, hemlock_address) =
        wallet_key_bytes_from_mnemonic(HEMLOCK_MNEMONIC);
    let (block_entry, output_indices) = honked_rpc_block_entry_and_indices();
    let honked_result = honked_bagpipe_block_result();
    let expected_parent_hash: [u8; 32] = hex::decode(
        honked_result["block_header"]["prev_hash"]
            .as_str()
            .expect("previous block hash should be present"),
    )
    .expect("previous block hash should decode")
    .try_into()
    .expect("previous block hash should be 32 bytes");
    let daemon_height = honked_result["block_header"]["height"]
        .as_u64()
        .expect("block height should be present")
        + 100;
    let wallet_configs = vec![
        OxideWalletScanConfig {
            public_spend_key: honked_public_spend_key,
            private_view_key: honked_private_view_key,
            private_spend_key: Some(honked_private_spend_key),
            subaddresses: vec![(0, 1), (1, 0)],
        },
        OxideWalletScanConfig {
            public_spend_key: hemlock_public_spend_key,
            private_view_key: hemlock_private_view_key,
            private_spend_key: None,
            subaddresses: vec![(0, 1), (1, 0)],
        },
    ];

    let multi_results = scan_validated_rpc_blocks_as_multi_wallet_scan_results(
        OxideNetwork::Stagenet,
        std::slice::from_ref(&block_entry),
        std::slice::from_ref(&output_indices),
        &wallet_configs,
        Some(expected_parent_hash),
        daemon_height,
    )
    .expect("validated oxide RPC scan should map into current multi-wallet results");

    assert_eq!(multi_results.len(), 1);
    let multi_result = &multi_results[0];
    assert_eq!(multi_result.block_height, 1_384_526);
    assert_eq!(
        multi_result.block_hash,
        honked_result["block_header"]["hash"]
            .as_str()
            .expect("block hash should be present")
    );
    assert_eq!(multi_result.daemon_height, daemon_height);
    assert_eq!(multi_result.wallet_results.len(), 2);
    assert_eq!(
        multi_result
            .wallet_results
            .get(&keys.address)
            .expect("matching wallet should be present")
            .outputs
            .len(),
        1
    );
    assert!(
        multi_result
            .wallet_results
            .get(&keys.address)
            .expect("matching wallet should be present")
            .outputs[0]
            .key_image
            .len()
            > 0
    );
    assert!(multi_result
        .wallet_results
        .get(&hemlock_address)
        .expect("unmatched wallet should be present")
        .outputs
        .is_empty());
    assert_eq!(multi_result.spent_key_images.len(), 4);
    assert_eq!(
        multi_result.spent_key_images.len(),
        multi_result.spent_key_image_tx_hashes.len()
    );

    let mut wrong_parent_hash = expected_parent_hash;
    wrong_parent_hash[0] ^= 1;
    let err = scan_validated_rpc_blocks_as_multi_wallet_scan_results(
        OxideNetwork::Stagenet,
        &[block_entry],
        &[output_indices],
        &wallet_configs,
        Some(wrong_parent_hash),
        daemon_height,
    )
    .expect_err("validated multi-wallet scan should reject a wrong parent before mapping");
    assert!(matches!(err, OxideAdapterError::Parse(_)));

    let err = scan_validated_rpc_blocks_as_multi_wallet_scan_results(
        OxideNetwork::Stagenet,
        &[],
        &[],
        &[],
        None,
        daemon_height,
    )
    .expect_err("multi-wallet scan should require at least one wallet config");
    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_validated_rpc_batch_rejects_unrelated_fixture_chain() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (miner_entry, miner_indices) = miner_only_rpc_block_entry_and_indices();
    let (honked_entry, honked_indices) = honked_rpc_block_entry_and_indices();

    let err = scan_validated_rpc_blocks_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &[miner_entry, honked_entry],
        &[miner_indices, honked_indices],
        &[(0, 1), (1, 0)],
        None,
    )
    .expect_err("validated RPC scan should reject non-contiguous fixture blocks");

    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_wallet_rpc_batch_rejects_mismatched_output_index_blocks() {
    let (public_spend_key, private_view_key, _) = honked_wallet_key_bytes();
    let (miner_entry, _) = miner_only_rpc_block_entry_and_indices();
    let (honked_entry, honked_indices) = honked_rpc_block_entry_and_indices();

    let err = scan_rpc_blocks_with_wallet(
        public_spend_key,
        private_view_key,
        OxideNetwork::Stagenet,
        &[miner_entry, honked_entry],
        &[honked_indices],
        &[(0, 1), (1, 0)],
    )
    .expect_err("block/output-index metadata should be parallel");

    assert!(matches!(err, OxideAdapterError::Parse(_)));
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
#[test]
fn oxide_first_ringct_output_index_rejects_inconsistent_rpc_indices() {
    let result = honked_bagpipe_block_result();
    let blob = hex::decode(result["blob"].as_str().expect("blob should be present"))
        .expect("block blob should decode");
    let mut transactions = honked_bagpipe_transactions_for_block(&result);
    transactions[0].output_indices[1] += 1;
    let transaction_output_index_refs = transactions
        .iter()
        .map(|tx| tx.output_indices.as_slice())
        .collect::<Vec<_>>();

    let err = infer_first_ringct_output_index(&blob, &transaction_output_index_refs)
        .expect_err("non-contiguous output indices should be rejected");

    assert!(matches!(err, OxideAdapterError::Parse(_)));
}
