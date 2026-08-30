#![cfg(not(target_arch = "wasm32"))]

mod common;

use monero_oxide::block::Block;
use monero_rust::monero_rpc::{BlockCompleteEntry, GetBlocksFastResponse};
use monero_rust::scanner::{
    process_batch_multi_wallet_response, process_batch_response, Lookahead, WalletScanConfig,
};
use monero_rust::Network;

const STAGENET_SEED: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";

fn make_coinbase_block(height: u64, previous: [u8; 32]) -> Block {
    use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    use monero_oxide::ed25519::CompressedPoint;
    use monero_oxide::transaction::*;

    Block::new(
        monero_oxide::block::BlockHeader {
            hardfork_version: 16,
            hardfork_signal: 16,
            timestamp: 1600000000 + height * 120,
            previous,
            nonce: 0,
        },
        Transaction::V2 {
            prefix: TransactionPrefix {
                additional_timelock: Timelock::Block((height + 60) as usize),
                inputs: vec![Input::Gen(height as usize)],
                outputs: vec![Output {
                    amount: Some(0),
                    key: CompressedPoint::from(ED25519_BASEPOINT_POINT.compress().to_bytes()),
                    view_tag: Some(0),
                }],
                extra: vec![
                    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0,
                ],
            },
            proofs: None,
        },
        vec![],
    )
    .expect("synthetic coinbase block should be valid")
}

/// Build a batch whose blocks link through their parent hashes, so tests reach
/// scanning after the chain-consistency checks.
fn make_response(start_height: u64, count: usize, daemon_height: u64) -> GetBlocksFastResponse {
    let mut blocks = Vec::with_capacity(count);
    let mut previous = [0u8; 32];
    for i in 0..count {
        let block = make_coinbase_block(start_height + i as u64, previous);
        previous = block.hash();
        blocks.push(BlockCompleteEntry {
            block: block.serialize(),
            txs: vec![],
            pruned: false,
            block_weight: 0,
        });
    }

    GetBlocksFastResponse {
        status: "OK".to_string(),
        blocks,
        start_height,
        current_height: daemon_height,
        output_indices: vec![],
        untrusted: false,
        credits: 0,
        top_hash: String::new(),
        daemon_time: 0,
    }
}

fn lookahead() -> Lookahead {
    Lookahead {
        account: 0,
        subaddress: 5,
    }
}

// -- Error handling --

#[tokio::test]
async fn test_invalid_seed_rejected() {
    let resp = make_response(100, 1, 101);
    let err = process_batch_response(
        resp,
        100,
        "bad seed",
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .unwrap_err();
    assert!(err.contains("mnemonic") || err.contains("seed"), "{err}");
}

#[tokio::test]
async fn test_empty_configs_rejected() {
    let resp = make_response(100, 1, 101);
    let err = process_batch_multi_wallet_response(resp, 100, vec![], None)
        .await
        .unwrap_err();
    assert!(err.contains("No wallet"));
}

#[tokio::test]
async fn test_one_bad_seed_fails_multi_wallet_batch() {
    let resp = make_response(100, 1, 101);
    let configs = vec![
        WalletScanConfig {
            mnemonic: STAGENET_SEED.to_string(),
            network: Network::Stagenet,
            lookahead: lookahead(),
            passphrase: String::new(),
        },
        WalletScanConfig {
            mnemonic: "bad".to_string(),
            network: Network::Stagenet,
            lookahead: lookahead(),
            passphrase: String::new(),
        },
    ];
    assert!(
        process_batch_multi_wallet_response(resp, 100, configs, None)
            .await
            .is_err()
    );
}

// -- Edge cases --

#[tokio::test]
async fn test_empty_batch_returns_empty() {
    let resp = make_response(100, 0, 100);
    assert!(process_batch_response(
        resp,
        100,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        ""
    )
    .await
    .unwrap()
    .0
    .is_empty());
}

#[tokio::test]
async fn test_height_mismatch_detected() {
    let block = make_coinbase_block(999, [0u8; 32]);
    let resp = GetBlocksFastResponse {
        status: "OK".to_string(),
        blocks: vec![BlockCompleteEntry {
            block: block.serialize(),
            txs: vec![],
            pruned: false,
            block_weight: 0,
        }],
        start_height: 100, // doesn't match Gen(999)
        current_height: 200,
        output_indices: vec![],
        untrusted: false,
        credits: 0,
        top_hash: String::new(),
        daemon_time: 0,
    };
    let err = process_batch_response(
        resp,
        100,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
}

// -- Multi-wallet structure --

#[tokio::test]
async fn test_multi_wallet_produces_entry_per_wallet() {
    let resp = make_response(100, 3, 103);
    let other_seed = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
    let configs = vec![
        WalletScanConfig {
            mnemonic: STAGENET_SEED.to_string(),
            network: Network::Stagenet,
            lookahead: lookahead(),
            passphrase: String::new(),
        },
        WalletScanConfig {
            mnemonic: other_seed.to_string(),
            network: Network::Stagenet,
            lookahead: lookahead(),
            passphrase: String::new(),
        },
    ];
    let (results, _) = process_batch_multi_wallet_response(resp, 100, configs, None)
        .await
        .unwrap();
    assert_eq!(results.len(), 3);
    for r in &results {
        assert_eq!(r.wallet_results.len(), 2);
    }
}

// Batch range and chain validation
//
// The daemon supplies both `response.start_height` and the blocks. Internally
// consistent heights alone cannot establish that the requested range was returned.

#[tokio::test]
async fn rejects_a_batch_that_starts_later_than_the_one_requested() {
    // The blocks consistently declare heights 5000-5002, but the request began
    // at 100. Accepting this batch would skip heights 100-4999.
    let resp = make_response(5_000, 3, 10_000);
    let err = process_batch_response(
        resp,
        100,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .expect_err("a batch starting past the requested height must be rejected");
    assert!(err.contains("skip forward"), "{err}");
}

#[tokio::test]
async fn accepts_a_batch_rewound_to_an_earlier_fork_point() {
    // Allow an earlier start when the daemon reports a fork point.
    // History-aware scanning needs this for reorg recovery.
    let resp = make_response(90, 3, 10_000);
    let (results, _) = process_batch_response(
        resp,
        100,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .expect("a rewound batch is how a fork point is reported and must be accepted");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].block_height, 90);
}

#[tokio::test]
async fn rejects_a_block_that_does_not_follow_the_one_before_it() {
    // Each block passes its own height check, but block 101 does not name
    // block 100 as its parent.
    let mut resp = make_response(100, 3, 10_000);
    let spliced = make_coinbase_block(101, [0u8; 32]);
    resp.blocks[1] = BlockCompleteEntry {
        block: spliced.serialize(),
        txs: vec![],
        pruned: false,
        block_weight: 0,
    };

    let err = process_batch_response(
        resp,
        100,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .expect_err("a block that does not follow its predecessor must be rejected");
    assert!(err.contains("does not follow"), "{err}");
}

#[tokio::test]
async fn rejects_transaction_blobs_the_block_does_not_list() {
    // Supply an extra transaction blob when the block lists no transactions.
    // It must not be scanned or credited to the wallet.
    let mut resp = make_response(100, 1, 10_000);
    resp.blocks[0].txs = vec![vec![0u8; 32]];

    let err = process_batch_response(
        resp,
        100,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .expect_err("transaction blobs the block does not list must be rejected");
    assert!(err.contains("blobs were sent with it"), "{err}");
}

#[tokio::test]
async fn multi_wallet_batches_are_bound_to_the_requested_range_too() {
    // Check the separate multi-wallet loop too.
    let resp = make_response(5_000, 2, 10_000);
    let configs = vec![WalletScanConfig {
        mnemonic: STAGENET_SEED.to_string(),
        network: Network::Stagenet,
        lookahead: lookahead(),
        passphrase: String::new(),
    }];
    let err = process_batch_multi_wallet_response(resp, 100, configs, None)
        .await
        .expect_err("a multi-wallet batch starting past the requested height must be rejected");
    assert!(err.contains("skip forward"), "{err}");
}

#[tokio::test]
async fn multi_wallet_batches_reject_an_unlinked_block_too() {
    let mut resp = make_response(100, 3, 10_000);
    let spliced = make_coinbase_block(101, [0u8; 32]);
    resp.blocks[1] = BlockCompleteEntry {
        block: spliced.serialize(),
        txs: vec![],
        pruned: false,
        block_weight: 0,
    };
    let configs = vec![WalletScanConfig {
        mnemonic: STAGENET_SEED.to_string(),
        network: Network::Stagenet,
        lookahead: lookahead(),
        passphrase: String::new(),
    }];
    let err = process_batch_multi_wallet_response(resp, 100, configs, None)
        .await
        .expect_err("a multi-wallet batch with an unlinked block must be rejected");
    assert!(err.contains("does not follow"), "{err}");
}

/// Build a parseable transaction with a distinct hash for each `marker`.
/// `Input::Gen` avoids needing RingCT proofs.
fn make_tx(marker: u64) -> monero_oxide::transaction::Transaction {
    use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    use monero_oxide::ed25519::CompressedPoint;
    use monero_oxide::transaction::*;

    Transaction::V2 {
        prefix: TransactionPrefix {
            additional_timelock: Timelock::Block(marker as usize),
            inputs: vec![Input::Gen(marker as usize)],
            outputs: vec![Output {
                amount: Some(0),
                key: CompressedPoint::from(ED25519_BASEPOINT_POINT.compress().to_bytes()),
                view_tag: Some(0),
            }],
            extra: vec![1; 33],
        },
        proofs: None,
    }
}

/// Build one block at `height` with the transaction hashes in `listed`.
fn make_block_listing(height: u64, previous: [u8; 32], listed: Vec<[u8; 32]>) -> Block {
    let coinbase = make_coinbase_block(height, previous);
    Block::new(
        coinbase.header.clone(),
        coinbase.miner_transaction().clone(),
        listed,
    )
    .expect("synthetic block should be valid")
}

fn response_with(block: Block, blobs: Vec<Vec<u8>>, start_height: u64) -> GetBlocksFastResponse {
    GetBlocksFastResponse {
        status: "OK".to_string(),
        blocks: vec![BlockCompleteEntry {
            block: block.serialize(),
            txs: blobs,
            pruned: false,
            block_weight: 0,
        }],
        start_height,
        current_height: start_height + 1,
        output_indices: vec![],
        untrusted: false,
        credits: 0,
        top_hash: String::new(),
        daemon_time: 0,
    }
}

/// Reject a transaction blob whose hash differs from the block's listed hash.
///
/// The daemon sends blobs separately. A matching count does not prevent it
/// from substituting a transaction while leaving the block unchanged. Such a
/// transaction must not contribute outputs to the wallet balance.
#[tokio::test]
async fn batch_scan_rejects_a_transaction_blob_the_block_does_not_list() {
    let listed = make_tx(1);
    let substituted = make_tx(2);
    assert_ne!(
        listed.hash(),
        substituted.hash(),
        "premise: the two transactions must differ"
    );

    let block = make_block_listing(100, [0u8; 32], vec![listed.hash()]);
    let resp = response_with(block, vec![substituted.serialize()], 100);

    let err = process_batch_response(
        resp,
        100,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .expect_err("a blob the block does not list must be refused");
    assert!(
        err.contains("blob sent in its place"),
        "unexpected error: {err}"
    );
}

/// Accept the same block when its transaction blob matches the listed hash.
#[tokio::test]
async fn batch_scan_accepts_the_transaction_blob_the_block_lists() {
    let tx = make_tx(1);
    let block = make_block_listing(100, [0u8; 32], vec![tx.hash()]);
    let resp = response_with(block, vec![tx.serialize()], 100);

    let (results, _scanner) = process_batch_response(
        resp,
        100,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .expect("a block whose blob matches what it lists must scan");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tx_count, 2, "the miner tx and the listed one");
}

/// Enforce the transaction-hash check in multi-wallet batch scanning too.
#[tokio::test]
async fn multi_wallet_batch_scan_rejects_a_transaction_blob_the_block_does_not_list() {
    let listed = make_tx(1);
    let substituted = make_tx(2);

    let block = make_block_listing(100, [0u8; 32], vec![listed.hash()]);
    let resp = response_with(block, vec![substituted.serialize()], 100);

    let configs = vec![WalletScanConfig {
        mnemonic: STAGENET_SEED.to_string(),
        network: Network::Stagenet,
        lookahead: lookahead(),
        passphrase: String::new(),
    }];

    let err = process_batch_multi_wallet_response(resp, 100, configs, None)
        .await
        .expect_err("a blob the block does not list must be refused");
    assert!(
        err.contains("blob sent in its place"),
        "unexpected error: {err}"
    );
}

#[derive(Clone, Debug)]
struct HistoryNode {
    response: Vec<u8>,
}

#[async_trait::async_trait]
impl monero_rust::monero_rpc::RpcConnection for HistoryNode {
    async fn post(
        &self,
        route: &str,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, monero_rust::monero_rpc::RpcError> {
        if route == "json_rpc" {
            return Ok(serde_json::to_vec(&serde_json::json!({
                "result": {"block_header": {"hash": hex::encode(make_coinbase_block(0, [0; 32]).hash())}}
            }))
            .unwrap());
        }
        assert_eq!(route, "getblocks.bin");
        #[derive(serde::Deserialize)]
        struct Request {
            start_height: u64,
            #[serde(with = "serde_bytes")]
            block_ids: Vec<u8>,
        }
        let request: Request = monero_epee_bin_serde::from_bytes(&body).unwrap();
        assert_eq!(request.start_height, 0, "preserve the protocol sentinel");
        assert!(!request.block_ids.is_empty());
        Ok(self.response.clone())
    }
}

fn history_node(response: GetBlocksFastResponse) -> monero_rust::monero_rpc::Rpc<HistoryNode> {
    #[derive(serde::Serialize)]
    struct Entry {
        block: Vec<u8>,
    }
    #[derive(serde::Serialize)]
    struct Response {
        status: String,
        blocks: Vec<Entry>,
        start_height: u64,
        current_height: u64,
    }
    let response = Response {
        status: response.status,
        blocks: response
            .blocks
            .into_iter()
            .map(|b| Entry { block: b.block })
            .collect(),
        start_height: response.start_height,
        current_height: response.current_height,
    };
    monero_rust::monero_rpc::Rpc::new_with_connection(HistoryNode {
        response: monero_epee_bin_serde::to_bytes(&response).unwrap(),
    })
}

fn history_configs() -> Vec<WalletScanConfig> {
    vec![WalletScanConfig {
        mnemonic: STAGENET_SEED.into(),
        network: Network::Stagenet,
        lookahead: lookahead(),
        passphrase: String::new(),
    }]
}

#[tokio::test]
async fn history_zero_negotiates_a_known_common_block_on_all_processing_routes() {
    use monero_rust::scanner::*;
    let rpc = history_node(make_response(100, 3, 103));
    // Daemon chooses an older common ancestor, not necessarily the first history entry.
    let history = vec![
        (120, hex::encode([9; 32])),
        (100, hex::encode(make_coinbase_block(100, [0; 32]).hash())),
    ];
    let (direct, start) = scan_blocks_batch_with_history(
        &rpc,
        0,
        &history,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        false,
        "",
    )
    .await
    .unwrap();
    assert_eq!(start, 100);
    assert_eq!(
        direct.iter().map(|b| b.block_height).collect::<Vec<_>>(),
        [100, 101, 102]
    );
    let fetch = || fetch_blocks_batch_with_history(&rpc, 0, &history, false);
    let (fetched, _) = fetch().await.unwrap();
    let (single, cache) = process_fetched_batch_cached(
        fetched,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .unwrap();
    assert_eq!(single.len(), 3);
    let (fetched, _) = fetch().await.unwrap();
    let (single, _) = process_fetched_batch_cached(
        fetched,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        Some(cache),
        "",
    )
    .await
    .unwrap();
    assert_eq!(single[0].block_hash, direct[0].block_hash);
    let (fetched, _) = fetch().await.unwrap();
    assert_eq!(
        process_fetched_batch_multi_wallet(fetched, history_configs())
            .await
            .unwrap()
            .len(),
        3
    );
    let (fetched, _) = fetch().await.unwrap();
    let (_, cache) = process_fetched_batch_multi_wallet_cached(fetched, history_configs(), None)
        .await
        .unwrap();
    let (fetched, _) = fetch().await.unwrap();
    assert_eq!(
        process_fetched_batch_multi_wallet_cached(fetched, history_configs(), Some(cache))
            .await
            .unwrap()
            .0
            .len(),
        3
    );
}

#[tokio::test]
async fn history_zero_rejects_unanchored_and_mislabelled_starts() {
    use monero_rust::scanner::fetch_blocks_batch_with_history;
    let good_hash = hex::encode(make_coinbase_block(100, [0; 32]).hash());
    for history in [
        vec![(100, hex::encode([9; 32]))],
        vec![(101, good_hash.clone())],
    ] {
        let rpc = history_node(make_response(100, 3, 103));
        let error = fetch_blocks_batch_with_history(&rpc, 0, &history, false)
            .await
            .err()
            .unwrap();
        assert!(
            error.contains("not in the supplied wallet history"),
            "{error}"
        );
    }
    let mut response = make_response(100, 3, 103);
    response.start_height = 101;
    let rpc = history_node(response);
    let error = fetch_blocks_batch_with_history(&rpc, 0, &[(101, good_hash.clone())], false)
        .await
        .err()
        .unwrap();
    assert!(error.contains("mismatch"), "{error}");
    let rpc = history_node(make_response(100, 0, 100));
    assert!(
        fetch_blocks_batch_with_history(&rpc, 0, &[(100, good_hash)], false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn history_empty_and_ordinary_zero_remain_literal_genesis_requests() {
    use monero_rust::scanner::*;
    let rpc = history_node(make_response(100, 3, 103));
    let error = fetch_blocks_batch_with_history(&rpc, 0, &[], false)
        .await
        .err()
        .unwrap();
    assert!(error.contains("never skip forward"), "{error}");
    let error = process_batch_response(
        make_response(100, 3, 103),
        0,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        None,
        "",
    )
    .await
    .unwrap_err();
    assert!(error.contains("never skip forward"), "{error}");
}

#[tokio::test]
async fn history_anchor_does_not_bypass_parent_linkage() {
    use monero_rust::scanner::*;
    let mut response = make_response(100, 3, 103);
    response.blocks[1].block = make_coinbase_block(101, [9; 32]).serialize();
    let rpc = history_node(response);
    let history = [(100, hex::encode(make_coinbase_block(100, [0; 32]).hash()))];
    let error = scan_blocks_batch_with_history(
        &rpc,
        0,
        &history,
        STAGENET_SEED,
        Network::Stagenet,
        lookahead(),
        false,
        "",
    )
    .await
    .unwrap_err();
    assert!(error.contains("does not follow"), "{error}");
}
