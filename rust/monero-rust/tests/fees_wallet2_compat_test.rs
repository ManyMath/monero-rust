//! Fee selection compared with wallet2 at 22578c3f7. The replies under
//! vectors/wallet2_fees/ were captured by scripts/wallet2-fees/run_real.py.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use monero_rust::monero_rpc::{
    Fee, FeePriority, Rpc, RpcConnection, RpcError, DEFAULT_MAX_FEE_PER_BYTE,
};
use monero_rust::native::{estimate_fee_with_rpc_and_policy, FeePolicy};
use serde_json::{json, Value};

const FEES: [u64; 4] = [20_000, 80_000, 320_000, 4_000_000];
const MASK: u64 = 10_000;
const FULL_REWARD_ZONE: u64 = 300_000;
const HEIGHT: u64 = 1_000;

fn epee_escape(blob: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(blob.len());
    for &byte in blob {
        match byte {
            0x08 => out.extend(b"\\b"),
            0x0c => out.extend(b"\\f"),
            b'\n' => out.extend(b"\\n"),
            b'\r' => out.extend(b"\\r"),
            b'\t' => out.extend(b"\\t"),
            0x0b => out.extend(b"\\v"),
            b'"' => out.extend(b"\\\""),
            b'\\' => out.extend(b"\\\\"),
            b'/' => out.extend(b"\\/"),
            other => out.push(other),
        }
    }
    out
}

fn monerod_backlog_response(entries: &[(u64, u64)], status: &str) -> Vec<u8> {
    let mut body =
        b"{\r\n  \"id\": \"0\",\r\n  \"jsonrpc\": \"2.0\",\r\n  \"result\": {\r\n".to_vec();
    if !entries.is_empty() {
        let mut blob = Vec::new();
        for (weight, fee) in entries {
            blob.extend(weight.to_le_bytes());
            blob.extend(fee.to_le_bytes());
            blob.extend(0u64.to_le_bytes()); // time_in_pool
        }
        body.extend(b"    \"backlog\": \"");
        body.extend(epee_escape(&blob));
        body.extend(b"\",\r\n");
    }
    body.extend(b"    \"credits\": 0,\r\n    \"status\": \"");
    body.extend(status.as_bytes());
    body.extend(b"\",\r\n    \"top_hash\": \"\",\r\n    \"untrusted\": false\r\n  }\r\n}");
    body
}

#[derive(Clone, Debug)]
struct Node {
    fee_estimate: Value,
    backlog: Vec<(u64, u64)>,
    backlog_status: &'static str,
    info: Value,
    block_weights: Option<Vec<u64>>,
    headers_status: &'static str,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
}

impl Node {
    fn new() -> Self {
        Node {
            fee_estimate: json!({
                "credits": 0, "fee": FEES[0], "fees": FEES, "quantization_mask": MASK,
                "status": "OK", "top_hash": "", "untrusted": false
            }),
            backlog: vec![],
            backlog_status: "OK",
            info: json!({
                "height": HEIGHT, "block_weight_limit": 2 * FULL_REWARD_ZONE,
                "block_size_limit": 2 * FULL_REWARD_ZONE, "status": "OK"
            }),
            block_weights: Some(vec![0; 10]),
            headers_status: "OK",
            requests: Arc::default(),
        }
    }

    fn rpc(&self) -> Rpc<Node> {
        Rpc::new_with_connection(self.clone())
    }

    fn calls(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|(method, _)| method.clone())
            .collect()
    }

    fn params_of(&self, method: &str) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(called, _)| called == method)
            .map(|(_, params)| params.clone())
            .collect()
    }
}

#[async_trait]
impl RpcConnection for Node {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        assert_eq!(route, "json_rpc");
        let request: Value = serde_json::from_slice(&body).unwrap();
        let method = request["method"].as_str().unwrap().to_string();
        self.requests
            .lock()
            .unwrap()
            .push((method.clone(), request["params"].clone()));
        let result = match method.as_str() {
            "get_fee_estimate" => self.fee_estimate.clone(),
            "get_txpool_backlog" => {
                return Ok(monerod_backlog_response(&self.backlog, self.backlog_status))
            }
            "get_info" => self.info.clone(),
            "get_last_block_header" => json!({
                "block_header": { "major_version": 16 }, "status": "OK"
            }),
            "getblockheadersrange" => match &self.block_weights {
                Some(weights) => json!({
                    "headers": weights.iter()
                        .map(|weight| json!({ "block_weight": weight, "block_size": weight }))
                        .collect::<Vec<_>>(),
                    "status": self.headers_status
                }),
                None => {
                    return Ok(serde_json::to_vec(&json!({
                        "id": "0", "jsonrpc": "2.0",
                        "error": { "code": -2, "message": "Invalid start/end heights." }
                    }))
                    .unwrap())
                }
            },
            other => panic!("unexpected method {other}"),
        };
        Ok(serde_json::to_vec(&json!({ "id": "0", "jsonrpc": "2.0", "result": result })).unwrap())
    }
}

async fn automatic(node: &Node) -> u64 {
    node.rpc()
        .get_fee_checked_for_priority_at_height(
            FeePriority::Automatic,
            Some(HEIGHT),
            DEFAULT_MAX_FEE_PER_BYTE,
        )
        .await
        .expect("automatic fee")
        .per_weight
}

async fn checked(node: &Node, priority: FeePriority) -> Result<Fee, RpcError> {
    node.rpc()
        .get_fee_checked_for_priority(priority, DEFAULT_MAX_FEE_PER_BYTE)
        .await
}

#[tokio::test]
async fn explicit_priorities_select_the_daemons_tiers() {
    let node = Node::new();
    for (priority, tier) in [
        (FeePriority::Low, FEES[0]),
        (FeePriority::Normal, FEES[1]),
        (FeePriority::Elevated, FEES[2]),
        (FeePriority::Priority, FEES[3]),
    ] {
        assert_eq!(
            checked(&node, priority).await,
            Ok(Fee {
                per_weight: tier,
                mask: MASK
            })
        );
    }
    assert!(
        node.calls().iter().all(|call| call == "get_fee_estimate"),
        "an explicit priority needs no congestion data: {:?}",
        node.calls()
    );
    assert_eq!(
        node.params_of("get_fee_estimate")[0],
        json!({ "grace_blocks": 10 })
    );
}

#[tokio::test]
async fn automatic_uses_the_low_tier_when_nothing_is_congested() {
    assert_eq!(automatic(&Node::new()).await, FEES[0]);
}

#[tokio::test]
async fn automatic_steps_up_on_a_backlog_in_monerods_wire_format() {
    let mut node = Node::new();
    node.backlog = vec![(FULL_REWARD_ZONE, FULL_REWARD_ZONE * FEES[0])];
    assert_eq!(
        automatic(&node).await,
        FEES[1],
        "one block of backlog selects tier 2"
    );

    node.backlog = vec![(FULL_REWARD_ZONE - 1, (FULL_REWARD_ZONE - 1) * FEES[0])];
    assert_eq!(automatic(&node).await, FEES[0]);
    assert!(
        node.calls().contains(&"getblockheadersrange".to_string()),
        "with less than a block of backlog wallet2 goes on to the block-fill check \
         (wallet2.cpp:8942-8968)"
    );

    node.backlog = vec![(
        FULL_REWARD_ZONE,
        FULL_REWARD_ZONE * FEES[0] - FULL_REWARD_ZONE,
    )];
    assert_eq!(automatic(&node).await, FEES[0]);
}

#[tokio::test]
async fn automatic_steps_up_on_full_blocks_when_the_pool_is_empty() {
    let mut node = Node::new();
    node.block_weights = Some(vec![FULL_REWARD_ZONE; 10]);
    assert_eq!(
        automatic(&node).await,
        FEES[1],
        "full recent blocks select tier 2"
    );
    assert_eq!(
        node.calls(),
        [
            "get_fee_estimate",
            "get_txpool_backlog",
            "get_info",
            "getblockheadersrange"
        ]
    );
    assert_eq!(
        node.params_of("getblockheadersrange")[0],
        json!({ "start_height": HEIGHT - 10, "end_height": HEIGHT - 1 }),
        "wallet2 asks for the ten blocks below its own height (wallet2.cpp:8958-8959)"
    );

    node.block_weights = Some(vec![243_000; 10]);
    assert_eq!(automatic(&node).await, FEES[1]);
    node.block_weights = Some(vec![242_999; 10]);
    assert_eq!(automatic(&node).await, FEES[0]);
}

#[tokio::test]
async fn captured_monerod_backlog_reply_selects_tier_two() {
    #[derive(Clone, Debug)]
    struct Captured;
    #[async_trait]
    impl RpcConnection for Captured {
        async fn post(&self, _: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
            let request: Value = serde_json::from_slice(&body).unwrap();
            Ok(match request["method"].as_str().unwrap() {
                "get_fee_estimate" => {
                    include_bytes!("vectors/wallet2_fees/real_backlog_get_fee_estimate.json")
                        .to_vec()
                }
                "get_txpool_backlog" => {
                    include_bytes!("vectors/wallet2_fees/real_backlog_get_txpool_backlog.bin")
                        .to_vec()
                }
                "get_info" => {
                    include_bytes!("vectors/wallet2_fees/real_backlog_get_info.json").to_vec()
                }
                "getblockheadersrange" => {
                    include_bytes!("vectors/wallet2_fees/real_backlog_getblockheadersrange.json")
                        .to_vec()
                }
                other => panic!("unexpected method {other}"),
            })
        }
    }
    let tiers: Value = serde_json::from_slice(include_bytes!(
        "vectors/wallet2_fees/real_backlog_get_fee_estimate.json"
    ))
    .unwrap();
    let normal = tiers["result"]["fees"][1].as_u64().unwrap();
    let fee = Rpc::new_with_connection(Captured)
        .get_fee_for_priority_at_height(FeePriority::Automatic, None)
        .await
        .unwrap();
    assert_eq!(fee.per_weight, normal);
}

#[tokio::test]
async fn backlog_fee_rate_threshold_uses_wallet2s_double_arithmetic() {
    let mut node = Node::new();
    let weight = 1u64 << 40;
    node.backlog = vec![(weight, weight * FEES[0] - 1)];
    assert_eq!(
        (weight * FEES[0] - 1) as f64 / weight as f64,
        FEES[0] as f64
    );
    assert_eq!(automatic(&node).await, FEES[1]);
}

#[tokio::test]
async fn congestion_arithmetic_wraps_where_wallet2s_does() {
    let mut node = Node::new();
    node.backlog = vec![(1 << 63, u64::MAX), (1 << 63, u64::MAX)];
    node.block_weights = Some(vec![FULL_REWARD_ZONE; 10]);
    assert_eq!(automatic(&node).await, FEES[1]);
    node.block_weights = Some(vec![0; 10]);
    assert_eq!(automatic(&node).await, FEES[0]);

    let mut node = Node::new();
    let mut weights = vec![0; 10];
    weights[0] = 1 << 63;
    weights[1] = 1 << 63;
    node.block_weights = Some(weights);
    assert_eq!(automatic(&node).await, FEES[0]);

    #[derive(Clone, Debug)]
    struct NoWeights(Node);
    #[async_trait]
    impl RpcConnection for NoWeights {
        async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
            let request: Value = serde_json::from_slice(&body).unwrap();
            if request["method"] == "getblockheadersrange" {
                let headers = vec![json!({ "block_size": FULL_REWARD_ZONE }); 10];
                return Ok(serde_json::to_vec(
                    &json!({ "result": { "headers": headers, "status": "OK" } }),
                )
                .unwrap());
            }
            self.0.post(route, body).await
        }
    }
    let fee = Rpc::new_with_connection(NoWeights(Node::new()))
        .get_fee_for_priority_at_height(FeePriority::Automatic, Some(HEIGHT))
        .await
        .unwrap();
    assert_eq!(fee.per_weight, FEES[0]);
}

#[tokio::test]
async fn a_zero_first_tier_is_not_stepped_over() {
    let mut node = Node::new();
    node.fee_estimate["fees"] = json!([0, 80_000, 320_000, 4_000_000]);
    node.backlog = vec![(FULL_REWARD_ZONE, FULL_REWARD_ZONE)];
    let fee = node
        .rpc()
        .get_fee_checked_for_priority_at_height(
            FeePriority::Automatic,
            Some(HEIGHT),
            DEFAULT_MAX_FEE_PER_BYTE,
        )
        .await;
    assert_eq!(fee, Err(RpcError::ZeroFee));
    assert_eq!(node.calls(), ["get_fee_estimate"]);
}

#[tokio::test]
async fn automatic_asks_for_headers_when_the_wallet_is_ahead_of_the_node() {
    let mut node = Node::new();
    node.block_weights = None;
    let fee = node
        .rpc()
        .get_fee_checked_for_priority_at_height(
            FeePriority::Automatic,
            Some(HEIGHT + 5),
            DEFAULT_MAX_FEE_PER_BYTE,
        )
        .await
        .unwrap();
    assert_eq!(fee.per_weight, FEES[0]);
    assert_eq!(
        node.params_of("getblockheadersrange"),
        [json!({ "start_height": HEIGHT - 5, "end_height": HEIGHT + 4 })]
    );
}

#[tokio::test]
async fn default_fee_cap_admits_every_tier_of_a_regtest_daemon() {
    let mut node = Node::new();
    let tiers = [1_200_000u64, 4_700_000, 19_000_000, 240_000_000];
    node.fee_estimate["fee"] = json!(tiers[0]);
    node.fee_estimate["fees"] = json!(tiers);
    for (priority, tier) in [
        (FeePriority::Low, tiers[0]),
        (FeePriority::Normal, tiers[1]),
        (FeePriority::Elevated, tiers[2]),
        (FeePriority::Priority, tiers[3]),
    ] {
        assert_eq!(
            checked(&node, priority).await,
            Ok(Fee {
                per_weight: tier,
                mask: MASK
            }),
            "{priority:?}"
        );
    }
}

#[tokio::test]
async fn a_tier_above_what_any_daemon_can_report_is_refused() {
    for (priority, index, rate) in [
        (FeePriority::Low, 0, 1_200_001u64),
        (FeePriority::Normal, 1, 4_700_001),
        (FeePriority::Elevated, 2, 19_000_001),
        (FeePriority::Priority, 3, 240_000_001),
    ] {
        let mut node = Node::new();
        let mut tiers = FEES;
        tiers[index] = rate;
        node.fee_estimate["fee"] = json!(tiers[0]);
        node.fee_estimate["fees"] = json!(tiers);
        assert!(
            matches!(
                checked(&node, priority).await,
                Err(RpcError::FeeExceedsLimit { per_weight, .. }) if per_weight == rate
            ),
            "{priority:?} at {rate}"
        );
    }
}

#[tokio::test]
async fn a_rate_above_the_callers_limit_is_refused() {
    let node = Node::new();
    assert_eq!(
        node.rpc()
            .get_fee_checked_for_priority(FeePriority::Normal, FEES[1])
            .await,
        Ok(Fee {
            per_weight: FEES[1],
            mask: MASK
        })
    );
    assert_eq!(
        node.rpc()
            .get_fee_checked_for_priority(FeePriority::Normal, FEES[1] - 1)
            .await,
        Err(RpcError::FeeExceedsLimit {
            per_weight: FEES[1],
            limit: FEES[1] - 1
        })
    );
}

#[tokio::test]
async fn fee_cap_bounds_the_fee_a_hostile_quantization_mask_can_force() {
    let mut node = Node::new();
    node.fee_estimate["quantization_mask"] = json!(1_000_000_000_000u64);
    let policy = FeePolicy {
        priority: FeePriority::Low,
        wallet_height: None,
        max_fee_per_weight: DEFAULT_MAX_FEE_PER_BYTE,
    };
    let ceiling = DEFAULT_MAX_FEE_PER_BYTE * 1720;
    match estimate_fee_with_rpc_and_policy(&node.rpc(), 1, 2, policy).await {
        Err(_) => {}
        Ok(estimate) => assert!(
            estimate.fee <= ceiling,
            "a node-chosen mask produced a fee of {} piconero, above the {} the per-byte cap \
             allows for this transaction",
            estimate.fee,
            ceiling
        ),
    }
}

#[tokio::test]
async fn zero_fee_and_zero_mask_are_refused() {
    let mut node = Node::new();
    node.fee_estimate["fees"] = json!([0, 80_000, 320_000, 4_000_000]);
    assert_eq!(
        checked(&node, FeePriority::Low).await,
        Err(RpcError::ZeroFee)
    );

    let mut node = Node::new();
    node.fee_estimate["quantization_mask"] = json!(0);
    assert_eq!(
        checked(&node, FeePriority::Low).await,
        Err(RpcError::ZeroMask)
    );
}

#[tokio::test]
async fn fee_estimate_with_a_failed_status_is_not_used() {
    for status in ["BUSY", "Failed", "PAYMENT REQUIRED"] {
        let mut node = Node::new();
        node.fee_estimate["status"] = json!(status);
        let fee = checked(&node, FeePriority::Low).await;
        assert!(
            fee.is_err(),
            "used the tiers of a reply with status {status}: {fee:?}"
        );
    }
}

#[tokio::test]
async fn congestion_replies_with_a_failed_status_are_not_used() {
    let mut node = Node::new();
    node.fee_estimate["status"] = json!("BUSY");
    assert_eq!(
        checked(&node, FeePriority::Low).await,
        Err(RpcError::NodeStatus("BUSY".to_string()))
    );

    let mut node = Node::new();
    node.backlog = vec![(FULL_REWARD_ZONE, FULL_REWARD_ZONE * FEES[0])];
    node.backlog_status = "BUSY";
    assert_eq!(automatic(&node).await, FEES[0], "backlog");

    let mut node = Node::new();
    node.block_weights = Some(vec![FULL_REWARD_ZONE; 10]);
    node.info["status"] = json!("PAYMENT REQUIRED");
    assert_eq!(automatic(&node).await, FEES[0], "get_info");

    let mut node = Node::new();
    node.block_weights = Some(vec![FULL_REWARD_ZONE; 10]);
    node.headers_status = "Failed";
    assert_eq!(automatic(&node).await, FEES[0], "headers");

    let mut node = Node::new();
    node.block_weights = Some(vec![FULL_REWARD_ZONE; 10]);
    node.info.as_object_mut().unwrap().remove("status");
    assert_eq!(automatic(&node).await, FEES[0], "get_info without status");
}

#[tokio::test]
async fn fee_estimate_without_the_requested_tier_is_refused_as_such() {
    let mut node = Node::new();
    node.fee_estimate["fees"] = json!([FEES[0]]);
    assert_eq!(
        checked(&node, FeePriority::Low).await,
        Ok(Fee {
            per_weight: FEES[0],
            mask: MASK
        })
    );
    let error = checked(&node, FeePriority::Normal).await.unwrap_err();
    assert!(
        error.to_string().starts_with("invalid fee estimate"),
        "{error}"
    );
}

#[tokio::test]
async fn fee_estimate_with_a_missing_member_is_refused_as_such() {
    let mut node = Node::new();
    node.fee_estimate
        .as_object_mut()
        .unwrap()
        .remove("quantization_mask");
    let error = checked(&node, FeePriority::Low).await.unwrap_err();
    assert!(
        error.to_string().starts_with("invalid fee estimate"),
        "{error}"
    );

    let mut node = Node::new();
    node.fee_estimate.as_object_mut().unwrap().remove("fee");
    node.fee_estimate.as_object_mut().unwrap().remove("fees");
    let error = checked(&node, FeePriority::Low).await.unwrap_err();
    assert!(
        error.to_string().starts_with("invalid fee estimate"),
        "{error}"
    );

    let mut node = Node::new();
    node.fee_estimate.as_object_mut().unwrap().remove("fee");
    assert_eq!(
        checked(&node, FeePriority::Low).await,
        Ok(Fee {
            per_weight: FEES[0],
            mask: MASK
        })
    );
}

#[tokio::test]
#[ignore = "known defect: every fee selection repeats get_fee_estimate, which wallet2 sends once per height"]
async fn consecutive_fee_selections_ask_for_the_estimate_once() {
    let node = Node::new();
    let rpc = node.rpc();
    for _ in 0..2 {
        rpc.get_fee_checked_for_priority_at_height(
            FeePriority::Automatic,
            Some(HEIGHT),
            DEFAULT_MAX_FEE_PER_BYTE,
        )
        .await
        .unwrap();
    }
    let count = |method: &str| node.calls().iter().filter(|call| *call == method).count();
    assert_eq!(
        count("get_fee_estimate"),
        1,
        "calls were {:?}",
        node.calls()
    );
    assert_eq!(count("get_txpool_backlog"), 2);
    assert_eq!(count("getblockheadersrange"), 2);
}

#[test]
#[ignore = "known defect: adjust_recipients_for_fee gives the whole remainder to the first recipient"]
fn fee_subtracted_from_recipients_is_spread_like_wallet2() {
    let recipients: Vec<(String, u64)> = ["a", "b", "c"]
        .iter()
        .map(|name| (name.to_string(), 100))
        .collect();
    let amounts: Vec<u64> = monero_rust::adjust_recipients_for_fee(&recipients, 11)
        .into_iter()
        .map(|(_, amount)| amount)
        .collect();
    assert_eq!(amounts, [96, 96, 97]);
}
