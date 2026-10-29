//! Needs `MONERO_REGTEST_RPC`: `eval "$(scripts/regtest-node.sh start)"`.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use monero_oxide::transaction::{Input, NotPruned, Transaction};
use monero_rust::monero_rpc::{HttpRpc, Rpc};
use monero_rust::native::{
    broadcast_raw_transaction_with_rpc, broadcast_transaction_with_rpc,
    create_transaction_with_rpc, StoredOutputData, TransactionResult,
};
use monero_rust::scanner::{derive_address, scan_block_for_outputs_with_lookahead, Lookahead};
use monero_rust::WalletOutput;
use monero_rust::{process_batch_with_reorg_detection, ScanBatchOutcome, WalletState};
use serde_json::{json, Value};

const MINER: &str = "velvet lymph giddy number token physics poetry unquoted nibs useful \
    sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing \
    fences goat unquoted";
const SPENDER: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders \
    cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful \
    layout lower eden";

const NETWORK: monero_rust::Network = monero_rust::Network::Mainnet;
const LOOKAHEAD: Lookahead = Lookahead {
    account: 0,
    subaddress: 5,
};
const MIN_HEIGHT: usize = 750;
const XMR: u64 = 1_000_000_000_000;

static CHAIN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn address(seed: &str) -> String {
    derive_address(seed, NETWORK, "").unwrap().to_string()
}

async fn mine(rpc: &Rpc<HttpRpc>, blocks: usize) {
    rpc.generate_blocks(&address(MINER), blocks)
        .await
        .expect("generateblocks");
}

async fn pool_hashes(rpc: &Rpc<HttpRpc>) -> Vec<String> {
    let pool: Value = rpc
        .rpc_call::<Option<()>, Value>("get_transaction_pool_hashes", None)
        .await
        .expect("get_transaction_pool_hashes");
    pool["tx_hashes"]
        .as_array()
        .map(|hashes| {
            hashes
                .iter()
                .map(|hash| hash.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

async fn mine_pool(rpc: &Rpc<HttpRpc>) {
    for _ in 0..20 {
        mine(rpc, 1).await;
        if pool_hashes(rpc).await.is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    panic!("the pool is still not empty after 20 blocks");
}

async fn drop_from_pool(rpc: &Rpc<HttpRpc>, tx_id: &str) {
    let _: Value = rpc
        .json_rpc_call("flush_txpool", Some(json!({ "txids": [tx_id] })))
        .await
        .expect("flush_txpool");
    assert!(!pool_hashes(rpc).await.contains(&tx_id.to_string()));
}

fn ring_of(tx_blob_hex: &str, key_image: &str) -> Vec<u64> {
    let bytes = hex::decode(tx_blob_hex).unwrap();
    let tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).unwrap();
    for input in &tx.prefix().inputs {
        if let Input::ToKey {
            key_offsets,
            key_image: image,
            ..
        } = input
        {
            if hex::encode(image.to_bytes()) == key_image {
                let mut index = 0;
                return key_offsets
                    .iter()
                    .map(|offset| {
                        index += offset;
                        index
                    })
                    .collect();
            }
        }
    }
    panic!("transaction does not spend {key_image}");
}

async fn refresh(rpc: &Rpc<HttpRpc>, state: &mut WalletState, from: u64) -> bool {
    let mut reorged = false;
    loop {
        let tip = rpc.get_height().await.unwrap() as u64;
        let mut results = Vec::new();
        for height in from..tip {
            results.push(
                scan_block_for_outputs_with_lookahead(rpc, height, SPENDER, NETWORK, LOOKAHEAD, "")
                    .await
                    .expect("scan"),
            );
        }
        match process_batch_with_reorg_detection(&results, state, None, tip, from).unwrap() {
            ScanBatchOutcome::Reorg(info) => {
                println!(
                    "reorg at {}: {} outputs removed, {} unspent",
                    info.split_height, info.outputs_removed, info.outputs_unspent
                );
                reorged = true;
            }
            ScanBatchOutcome::Normal(batch) => {
                state.add_outputs(batch.outputs_to_store);
                for block in &results {
                    state.mark_spent_detecting_conflicts(
                        &block.spent_key_images,
                        &block.spent_key_image_tx_hashes,
                        block.block_height,
                    );
                }
                for (height, hash) in &batch.block_hashes {
                    state.record_block_hash(*height, hash.clone());
                }
                state.current_height = tip - 1;
                state.daemon_height = tip;
                return reorged;
            }
        }
    }
}

fn wallet_holding(outputs: Vec<WalletOutput>) -> WalletState {
    let mut state = WalletState::new();
    state.add_outputs(outputs);
    state
}

fn restart(state: &WalletState) -> WalletState {
    let outputs = serde_json::to_string(state.outputs()).unwrap();
    let rings = serde_json::to_string(state.ring_database()).unwrap();
    let mut state = WalletState::new();
    state
        .restore_ring_database(serde_json::from_str(&rings).unwrap())
        .unwrap();
    state.replace_outputs(serde_json::from_str(&outputs).unwrap());
    state
}

async fn build(
    rpc: &Rpc<HttpRpc>,
    state: &mut WalletState,
    seed: &str,
    input: StoredOutputData,
    recipient: &str,
    amount: u64,
) -> TransactionResult {
    let _ = state;
    create_transaction_with_rpc(
        rpc,
        seed,
        "mainnet",
        vec![input],
        &[(address(recipient), amount)],
        "",
    )
    .await
    .expect("build and sign")
}

async fn relay(
    rpc: &Rpc<HttpRpc>,
    state: &mut WalletState,
    transaction: &TransactionResult,
) -> Result<(), String> {
    broadcast_transaction_with_rpc(rpc, transaction, state, false).await
}

async fn relay_raw(
    rpc: &Rpc<HttpRpc>,
    state: &mut WalletState,
    tx_blob_hex: &str,
) -> Result<(), String> {
    let _ = state;
    broadcast_raw_transaction_with_rpc(rpc, tx_blob_hex, false).await
}

struct Funded {
    state: WalletState,
    key_image: String,
    received_at: u64,
    funding_blob: String,
    funding_tx_id: String,
    miner: WalletState,
}

async fn fund_spender(rpc: &Rpc<HttpRpc>) -> Funded {
    let height = rpc.get_height().await.unwrap();
    if height < MIN_HEIGHT {
        mine(rpc, MIN_HEIGHT - height).await;
    }
    let mined_at = rpc.get_height().await.unwrap() as u64;
    mine(rpc, 66).await;
    let coinbase =
        scan_block_for_outputs_with_lookahead(rpc, mined_at, MINER, NETWORK, LOOKAHEAD, "")
            .await
            .unwrap()
            .outputs
            .into_iter()
            .next()
            .expect("the block was mined to the miner");
    let mut miner = wallet_holding(vec![coinbase.clone()]);
    let funding = build(
        rpc,
        &mut miner,
        MINER,
        StoredOutputData::from(&coinbase),
        SPENDER,
        5 * XMR,
    )
    .await;
    relay(rpc, &mut miner, &funding)
        .await
        .expect("relay funding transaction");

    mine_pool(rpc).await;
    let received_at = rpc.get_height().await.unwrap() as u64 - 1;
    mine(rpc, 11).await;

    let mut state = wallet_holding(vec![]);
    refresh(rpc, &mut state, received_at).await;
    assert_eq!(state.outputs().len(), 1, "the spender received one output");
    let output = &state.outputs()[0];
    assert_eq!(output.block_height, received_at);
    assert!(output.global_index.is_some());
    Funded {
        key_image: output.key_image.clone(),
        state,
        received_at,
        funding_blob: funding.tx_blob.clone(),
        funding_tx_id: funding.tx_id.clone(),
        miner,
    }
}

async fn spend(
    rpc: &Rpc<HttpRpc>,
    state: &mut WalletState,
    key_image: &str,
    amount: u64,
) -> TransactionResult {
    let input = state
        .outputs()
        .iter()
        .find(|output| output.key_image == key_image)
        .map(StoredOutputData::from)
        .expect("the output is in wallet state");
    build(rpc, state, SPENDER, input, MINER, amount).await
}

#[tokio::test(flavor = "multi_thread")]
async fn regtest_respend_after_pool_drop_reuses_the_ring_and_the_daemon_accepts_it() {
    let Some(node) = common::regtest_node() else {
        return;
    };
    let _chain = CHAIN.lock().await;
    let rpc = HttpRpc::new(node).unwrap();
    let Funded {
        mut state,
        key_image,
        ..
    } = fund_spender(&rpc).await;

    let first = spend(&rpc, &mut state, &key_image, XMR).await;
    relay(&rpc, &mut state, &first)
        .await
        .expect("the daemon accepts the first spend");
    assert!(pool_hashes(&rpc).await.contains(&first.tx_id));
    drop_from_pool(&rpc, &first.tx_id).await;

    let mut state = restart(&state);

    let second = spend(&rpc, &mut state, &key_image, 2 * XMR).await;
    assert_ne!(first.tx_id, second.tx_id);
    assert_eq!(
        ring_of(&first.tx_blob, &key_image),
        ring_of(&second.tx_blob, &key_image),
        "the respend reuses the ring the network already saw"
    );
    relay(&rpc, &mut state, &second)
        .await
        .expect("the daemon accepts the respend with the reused ring");
    mine_pool(&rpc).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn regtest_reorg_of_the_receiving_block_keeps_the_ring() {
    let Some(node) = common::regtest_node() else {
        return;
    };
    let _chain = CHAIN.lock().await;
    let rpc = HttpRpc::new(node).unwrap();
    let Funded {
        mut state,
        key_image,
        received_at,
        funding_blob,
        funding_tx_id,
        mut miner,
    } = fund_spender(&rpc).await;
    let index_before = state.outputs()[0].global_index;

    let first = spend(&rpc, &mut state, &key_image, XMR).await;
    relay(&rpc, &mut state, &first)
        .await
        .expect("the daemon accepts the first spend");
    let first_ring = ring_of(&first.tx_blob, &key_image);
    assert!(state.outputs()[0].ring.is_some());
    drop_from_pool(&rpc, &first.tx_id).await;

    let tip = rpc.get_height().await.unwrap() as u64;
    let _: Value = rpc
        .rpc_call("pop_blocks", Some(json!({ "nblocks": tip - received_at })))
        .await
        .expect("pop_blocks");
    assert_eq!(rpc.get_height().await.unwrap() as u64, received_at);
    if !pool_hashes(&rpc).await.contains(&funding_tx_id) {
        relay_raw(&rpc, &mut miner, &funding_blob)
            .await
            .expect("re-relay the funding transaction");
    }
    mine(&rpc, 14).await;
    assert!(
        pool_hashes(&rpc).await.is_empty(),
        "the funding tx was mined again"
    );

    let reorged = refresh(&rpc, &mut state, received_at).await;
    assert!(reorged, "the crate detected the reorg");
    let output = state
        .outputs()
        .iter()
        .find(|output| output.key_image == key_image)
        .expect("the same output, with the same key image, is found again");
    println!(
        "global index before {:?}, after {:?}; ring after rescan: {}",
        index_before,
        output.global_index,
        if output.ring.is_some() {
            "kept"
        } else {
            "gone"
        }
    );

    let second = spend(&rpc, &mut state, &key_image, XMR).await;
    let second_ring = ring_of(&second.tx_blob, &key_image);
    let accepted = relay(&rpc, &mut state, &second).await;
    println!("daemon verdict on the respend: {accepted:?}");
    mine(&rpc, 1).await;

    let shared = first_ring
        .iter()
        .filter(|index| second_ring.contains(index))
        .count();
    assert_eq!(
        first_ring, second_ring,
        "the daemon was handed two rings for key image {key_image}, sharing {shared} of 16 \
         members"
    );
}
