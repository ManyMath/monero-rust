#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use monero_oxide::block::{Block, BlockHeader};
use monero_oxide::ed25519::CompressedPoint;
use monero_oxide::transaction::{Input, Output, Timelock, Transaction, TransactionPrefix};
use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError, MAX_RESPONSE_BYTES};
use serde_json::{json, Value};

const RESTRICTED_TRANSACTIONS_COUNT: usize = 100;

const LARGEST_TRANSACTION_HEX: usize = 2 * 299_400;

fn make_tx(marker: u64) -> Transaction {
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

#[derive(Clone, Debug)]
struct Request {
    route: String,
    ids: usize,
    limit: usize,
}

#[derive(Clone, Debug, Default)]
struct RestrictedNode {
    txs: Arc<HashMap<[u8; 32], Vec<u8>>>,
    block: Option<([u8; 32], Vec<u8>)>,
    fail_request: Option<usize>,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl RestrictedNode {
    fn with_txs(txs: &[Transaction]) -> Self {
        RestrictedNode {
            txs: Arc::new(txs.iter().map(|tx| (tx.hash(), tx.serialize())).collect()),
            ..Default::default()
        }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    fn get_transactions(&self, request: &Value) -> Value {
        let ids: Vec<&str> = request["txs_hashes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect();
        if ids.len() > RESTRICTED_TRANSACTIONS_COUNT {
            return json!({
                "credits": 0,
                "status": "Too many transactions requested in restricted mode",
                "top_hash": "",
                "untrusted": false,
            });
        }
        let mut txs = Vec::new();
        let mut missed = Vec::new();
        for id in ids {
            let hash: [u8; 32] = hex::decode(id).unwrap().try_into().unwrap();
            match self.txs.get(&hash) {
                Some(blob) => txs.push(json!({
                    "as_hex": hex::encode(blob),
                    "as_json": "",
                    "in_pool": false,
                    "prunable_as_hex": "",
                    "prunable_hash": hex::encode([0u8; 32]),
                    "pruned_as_hex": "",
                    "tx_hash": id,
                })),
                None => missed.push(id.to_string()),
            }
        }
        let mut reply = json!({ "status": "OK", "untrusted": false, "txs": txs });
        if !missed.is_empty() {
            reply["missed_tx"] = json!(missed);
        }
        reply
    }
}

#[async_trait]
impl RpcConnection for RestrictedNode {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        self.post_limited(route, body, usize::MAX).await
    }

    async fn post_limited(
        &self,
        route: &str,
        body: Vec<u8>,
        limit: usize,
    ) -> Result<Vec<u8>, RpcError> {
        let request: Value = serde_json::from_slice(&body).unwrap();
        let index = {
            let mut requests = self.requests.lock().unwrap();
            requests.push(Request {
                route: route.to_string(),
                ids: request["txs_hashes"].as_array().map_or(0, Vec::len),
                limit,
            });
            requests.len() - 1
        };
        if self.fail_request == Some(index) {
            return Err(RpcError::ConnectionError);
        }
        let reply = match route {
            "get_transactions" => self.get_transactions(&request),
            "json_rpc" => {
                let (hash, blob) = self.block.clone().expect("a block to serve");
                match request["method"].as_str().unwrap() {
                    "get_block" => {
                        json!({ "result": { "blob": hex::encode(blob), "status": "OK" } })
                    }
                    "get_block_header_by_height" => json!({
                        "result": { "block_header": { "hash": hex::encode(hash) }, "status": "OK" }
                    }),
                    other => panic!("unexpected method {other}"),
                }
            }
            other => panic!("unexpected route {other}"),
        };
        Ok(serde_json::to_vec(&reply).unwrap())
    }
}

fn transaction_requests(node: &RestrictedNode) -> Vec<Request> {
    node.requests()
        .into_iter()
        .filter(|request| request.route == "get_transactions")
        .collect()
}

#[tokio::test]
async fn transactions_are_asked_for_in_slices_a_restricted_node_answers() {
    let txs: Vec<Transaction> = (0..250).map(make_tx).collect();
    let node = RestrictedNode::with_txs(&txs);
    let rpc = Rpc::new_with_connection(node.clone());

    let hashes: Vec<[u8; 32]> = txs.iter().map(Transaction::hash).collect();
    let fetched = rpc.get_transactions(&hashes).await.unwrap();
    assert_eq!(
        fetched.iter().map(Transaction::hash).collect::<Vec<_>>(),
        hashes,
        "every transaction, in request order"
    );

    let requests = transaction_requests(&node);
    assert_eq!(
        requests.iter().map(|r| r.ids).collect::<Vec<_>>(),
        vec![100, 100, 50]
    );
    for request in &requests {
        assert!(
            request.limit >= request.ids * LARGEST_TRANSACTION_HEX,
            "a reply limit of {} bytes cannot hold {} transactions",
            request.limit,
            request.ids
        );
        assert!(request.limit <= MAX_RESPONSE_BYTES);
    }

    let node = RestrictedNode::with_txs(&txs);
    let rpc = Rpc::new_with_connection(node.clone());
    assert_eq!(
        rpc.get_transactions(&hashes[..100]).await.unwrap().len(),
        100
    );
    assert_eq!(transaction_requests(&node).len(), 1);
}

#[tokio::test]
async fn a_failed_slice_fails_the_whole_request() {
    let txs: Vec<Transaction> = (0..250).map(make_tx).collect();
    let node = RestrictedNode {
        fail_request: Some(1),
        ..RestrictedNode::with_txs(&txs)
    };
    let rpc = Rpc::new_with_connection(node.clone());
    let hashes: Vec<[u8; 32]> = txs.iter().map(Transaction::hash).collect();
    assert_eq!(
        rpc.get_transactions(&hashes).await.unwrap_err(),
        RpcError::ConnectionError
    );
    assert_eq!(transaction_requests(&node).len(), 2);
}

#[tokio::test]
async fn missing_transactions_are_reported_from_every_slice() {
    let txs: Vec<Transaction> = (0..250).map(make_tx).collect();
    let mut hashes: Vec<[u8; 32]> = txs.iter().map(Transaction::hash).collect();
    let node = RestrictedNode::with_txs(&txs);
    let rpc = Rpc::new_with_connection(node.clone());
    hashes[5] = [0xaa; 32];
    hashes[205] = [0xbb; 32];
    assert_eq!(
        rpc.get_transactions(&hashes).await.unwrap_err(),
        RpcError::TransactionsNotFound(vec![[0xaa; 32], [0xbb; 32]])
    );
    assert_eq!(transaction_requests(&node).len(), 3);
}

#[tokio::test]
async fn a_block_with_more_than_100_transactions_is_read_through_a_restricted_node() {
    let txs: Vec<Transaction> = (1..=150).map(make_tx).collect();
    let block = Block::new(
        BlockHeader {
            hardfork_version: 16,
            hardfork_signal: 16,
            timestamp: 1_600_000_000,
            previous: [7; 32],
            nonce: 0,
        },
        Transaction::V2 {
            prefix: TransactionPrefix {
                additional_timelock: Timelock::Block(1060),
                inputs: vec![Input::Gen(1000)],
                outputs: vec![],
                extra: vec![],
            },
            proofs: None,
        },
        txs.iter().map(Transaction::hash).collect(),
    )
    .unwrap();
    let node = RestrictedNode {
        block: Some((block.hash(), block.serialize())),
        ..RestrictedNode::with_txs(&txs)
    };
    let rpc = Rpc::new_with_connection(node.clone());

    let fetched = rpc.get_block_transactions(block.hash()).await.unwrap();
    assert_eq!(fetched.len(), 151);
    assert_eq!(fetched[0].hash(), block.miner_transaction().hash());
    assert_eq!(
        fetched[1..]
            .iter()
            .map(Transaction::hash)
            .collect::<Vec<_>>(),
        block.transactions
    );
    assert_eq!(
        transaction_requests(&node)
            .iter()
            .map(|r| r.ids)
            .collect::<Vec<_>>(),
        vec![100, 50]
    );
}
