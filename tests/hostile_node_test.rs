//! Reject malformed daemon responses before using them for scanning or decoy selection.

use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError};
use serde::Serialize;

/// Return the configured response body for each route.
#[derive(Clone, Debug)]
struct ScriptedNode {
    responses: Vec<(&'static str, Vec<u8>)>,
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl RpcConnection for ScriptedNode {
    async fn post(&self, route: &str, _body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        for (candidate, response) in &self.responses {
            if *candidate == route {
                return Ok(response.clone());
            }
        }
        panic!("the test node was asked for an unscripted route: {route}");
    }
}

fn node(responses: Vec<(&'static str, Vec<u8>)>) -> Rpc<ScriptedNode> {
    Rpc::new_with_connection(ScriptedNode { responses })
}

#[derive(Clone, Serialize)]
struct Out {
    #[serde(with = "serde_bytes")]
    key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    mask: Vec<u8>,
    unlocked: bool,
    height: u64,
    #[serde(with = "serde_bytes")]
    txid: Vec<u8>,
}

#[derive(Serialize)]
struct Outs {
    outs: Vec<Out>,
    status: String,
}

fn out(unlocked: bool, height: u64) -> Out {
    let point = curve25519_dalek::constants::ED25519_BASEPOINT_POINT
        .compress()
        .to_bytes()
        .to_vec();
    Out {
        key: point.clone(),
        mask: point,
        unlocked,
        height,
        txid: vec![0; 32],
    }
}

fn out_response(count: usize, unlocked: bool, height: u64) -> Vec<u8> {
    monero_epee_bin_serde::to_bytes(&Outs {
        outs: vec![out(unlocked, height); count],
        status: "OK".to_string(),
    })
    .expect("encode get_outs response")
}

#[derive(Serialize)]
struct Distributions {
    distributions: Vec<Distribution>,
    status: String,
}

#[derive(Serialize)]
struct Distribution {
    amount: u64,
    start_height: u64,
    binary: bool,
    compress: bool,
    #[serde(with = "serde_bytes")]
    compressed_data: Vec<u8>,
    base: u64,
}

fn distribution_response(distributions: Vec<Distribution>) -> Vec<u8> {
    monero_epee_bin_serde::to_bytes(&Distributions {
        distributions,
        status: "OK".to_string(),
    })
    .expect("encode distribution response")
}

#[tokio::test]
async fn get_unlocked_outputs_rejects_more_outputs_than_were_requested() {
    // Return three outputs for two requested indices. The extra entry would
    // index past the candidate list.
    let rpc = node(vec![("get_outs.bin", out_response(3, true, 1))]);

    let err = rpc
        .get_unlocked_outputs(&[10, 20], 1_000)
        .await
        .expect_err("a get_outs reply longer than the request should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode),
        "expected InvalidNode, got {err:?}"
    );
}

#[tokio::test]
async fn get_unlocked_outputs_rejects_fewer_outputs_than_were_requested() {
    // Return two outputs for three requested indices. Reject the incomplete list.
    let rpc = node(vec![("get_outs.bin", out_response(2, true, 1))]);

    let err = rpc
        .get_unlocked_outputs(&[10, 20, 30], 1_000)
        .await
        .expect_err("a get_outs reply shorter than the request should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode | RpcError::InternalError(_)),
        "expected the malformed binary reply to fail closed, got {err:?}"
    );
}

#[tokio::test]
async fn get_unlocked_outputs_honors_the_binary_unlock_metadata() {
    let rpc = node(vec![("get_outs.bin", out_response(2, false, 2_000))]);

    let outputs = rpc
        .get_unlocked_outputs(&[10, 20], 1_000)
        .await
        .expect("well-formed locked outputs should parse");
    assert_eq!(outputs, vec![None, None]);
}

#[tokio::test]
async fn get_output_distribution_rejects_an_empty_distributions_array() {
    // Reject the empty reply before calling `swap_remove(0)`.
    let rpc = node(vec![(
        "get_output_distribution.bin",
        distribution_response(vec![]),
    )]);

    let err = rpc
        .get_output_distribution(0, 1_000)
        .await
        .expect_err("an empty distributions array should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode | RpcError::InternalError(_)),
        "expected the malformed binary reply to fail closed, got {err:?}"
    );
}

#[tokio::test]
async fn get_output_distribution_rejects_a_cumulative_count_that_overflows() {
    let rpc = node(vec![(
        "get_output_distribution.bin",
        distribution_response(vec![Distribution {
            amount: 0,
            start_height: 0,
            binary: true,
            compress: true,
            compressed_data: vec![1],
            base: u64::MAX,
        }]),
    )]);

    let err = rpc
        .get_output_distribution(0, 1_000)
        .await
        .expect_err("an overflowing cumulative distribution should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode),
        "expected InvalidNode, got {err:?}"
    );
}

#[tokio::test]
async fn get_output_distribution_accepts_a_monotonic_distribution() {
    // Accept nondecreasing counts, including equal entries for empty blocks.
    let rpc = node(vec![(
        "get_output_distribution.bin",
        distribution_response(vec![Distribution {
            amount: 0,
            start_height: 0,
            binary: true,
            compress: true,
            compressed_data: vec![0, 0, 7, 0, 12],
            base: 0,
        }]),
    )]);

    let distribution = rpc
        .get_output_distribution(0, 1_000)
        .await
        .expect("a monotonic distribution should be accepted");
    assert_eq!(distribution, vec![0, 0, 7, 7, 19]);
}

/// Reject a truncated `get_transactions` reply.
///
/// Checking individual hashes would accept a valid prefix while missing the
/// remaining transactions. The scanner would then miss both received outputs
/// and key images for spent outputs in the omitted transactions.
#[tokio::test]
async fn get_transactions_rejects_fewer_transactions_than_were_requested() {
    let rpc = node(vec![(
        "get_transactions",
        b"{\"status\":\"OK\",\"txs\":[],\"missed_tx\":[]}".to_vec(),
    )]);

    let err = rpc
        .get_transactions(&[[1u8; 32], [2u8; 32]])
        .await
        .expect_err("a short get_transactions reply should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode),
        "expected InvalidNode, got {err:?}"
    );
}

/// Reject an empty reply to `get_transaction` before it calls `swap_remove(0)`.
#[tokio::test]
async fn get_transaction_rejects_an_empty_reply_rather_than_indexing_it() {
    let rpc = node(vec![(
        "get_transactions",
        b"{\"status\":\"OK\",\"txs\":[],\"missed_tx\":[]}".to_vec(),
    )]);

    let err = rpc
        .get_transaction([1u8; 32])
        .await
        .expect_err("an empty get_transactions reply should be refused, not indexed");
    assert!(
        matches!(err, RpcError::InvalidNode),
        "expected InvalidNode, got {err:?}"
    );
}
