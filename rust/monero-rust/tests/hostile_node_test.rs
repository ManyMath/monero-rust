//! The node is not trusted, so the shape of what it answers with is not either.
//!
//! Every response below is well-formed JSON that a real daemon would never send. They are
//! here because the send path consumes these three replies *positionally* - decoy selection
//! reads the output at position `i` as the one it asked for at `indexes[i]`, and reads the
//! output distribution as a running total - so a reply of the wrong length is not a parse
//! failure, it is a silent mispairing, an out-of-bounds index, or an underflow.
//!
//! Each test constructs the input its guard exists to reject and watches it be rejected.

use monero_rust::monero_backend::rpc::{Rpc, RpcConnection, RpcError};

/// Answers each route with a canned body, whatever was asked.
#[derive(Clone, Debug)]
struct ScriptedNode {
    responses: Vec<(&'static str, String)>,
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl RpcConnection for ScriptedNode {
    async fn post(&self, route: &str, _body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        for (candidate, response) in &self.responses {
            if *candidate == route {
                return Ok(response.clone().into_bytes());
            }
        }
        panic!("the test node was asked for an unscripted route: {route}");
    }
}

fn node(responses: Vec<(&'static str, String)>) -> Rpc<ScriptedNode> {
    Rpc::new_with_connection(ScriptedNode { responses })
}

fn json_rpc(result: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":\"0\",\"result\":{result}}}")
}

/// Build a parseable `get_outs` entry for response-count tests.
fn out_entry() -> String {
    let key = "a".repeat(64);
    let mask = "b".repeat(64);
    let txid = "c".repeat(64);
    format!("{{\"key\":\"{key}\",\"mask\":\"{mask}\",\"txid\":\"{txid}\"}}")
}

#[tokio::test]
async fn get_unlocked_outputs_rejects_more_outputs_than_were_requested() {
    // Return three outputs for two requested indices. The extra entry would
    // index past the candidate list.
    let outs = [out_entry(), out_entry(), out_entry()].join(",");
    let rpc = node(vec![(
        "get_outs",
        format!("{{\"status\":\"OK\",\"outs\":[{outs}]}}"),
    )]);

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
    let outs = [out_entry(), out_entry()].join(",");
    let rpc = node(vec![(
        "get_outs",
        format!("{{\"status\":\"OK\",\"outs\":[{outs}]}}"),
    )]);

    let err = rpc
        .get_unlocked_outputs(&[10, 20, 30], 1_000)
        .await
        .expect_err("a get_outs reply shorter than the request should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode),
        "expected InvalidNode, got {err:?}"
    );
}

#[tokio::test]
async fn get_unlocked_outputs_rejects_a_short_timelock_reply() {
    // Let the output-count check pass, then return one timelock for two
    // transactions. This must fail before indexing the timelocks.
    let outs = [out_entry(), out_entry()].join(",");
    let tx = format!(
        "{{\"tx_hash\":\"{}\",\"as_hex\":\"\",\"pruned_as_hex\":\"{}\"}}",
        "c".repeat(64),
        // version 2, timelock 0, no inputs, no outputs, no extra
        "0200000000"
    );
    let rpc = node(vec![
        (
            "get_outs",
            format!("{{\"status\":\"OK\",\"outs\":[{outs}]}}"),
        ),
        (
            "get_transactions",
            format!("{{\"status\":\"OK\",\"txs\":[{tx}],\"missed_tx\":[]}}"),
        ),
    ]);

    let err = rpc
        .get_unlocked_outputs(&[10, 20], 1_000)
        .await
        .expect_err("a get_transactions reply shorter than the request should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode),
        "expected InvalidNode, got {err:?}"
    );
}

#[tokio::test]
async fn get_output_distribution_rejects_an_empty_distributions_array() {
    // Reject the empty reply before calling `swap_remove(0)`.
    let rpc = node(vec![(
        "json_rpc",
        json_rpc("{\"status\":\"OK\",\"distributions\":[]}"),
    )]);

    let err = rpc
        .get_output_distribution(0, 1_000)
        .await
        .expect_err("an empty distributions array should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode),
        "expected InvalidNode, got {err:?}"
    );
}

#[tokio::test]
async fn get_output_distribution_rejects_a_distribution_that_decreases() {
    // A decreasing cumulative count would underflow when decoy selection
    // subtracts adjacent entries to count outputs per block.
    let rpc = node(vec![(
        "json_rpc",
        json_rpc("{\"status\":\"OK\",\"distributions\":[{\"distribution\":[10,20,15,30]}]}"),
    )]);

    let err = rpc
        .get_output_distribution(0, 1_000)
        .await
        .expect_err("a non-monotonic distribution should be refused");
    assert!(
        matches!(err, RpcError::InvalidNode),
        "expected InvalidNode, got {err:?}"
    );
}

#[tokio::test]
async fn get_output_distribution_accepts_a_monotonic_distribution() {
    // Accept nondecreasing counts, including equal entries for empty blocks.
    let rpc = node(vec![(
        "json_rpc",
        json_rpc("{\"status\":\"OK\",\"distributions\":[{\"distribution\":[0,0,7,7,19]}]}"),
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
        "{\"status\":\"OK\",\"txs\":[],\"missed_tx\":[]}".to_string(),
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
        "{\"status\":\"OK\",\"txs\":[],\"missed_tx\":[]}".to_string(),
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
