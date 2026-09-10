// Test binaries use different subsets of these shared helpers. Recorded-RPC
// helpers also require the mock-rpc feature.
#![allow(dead_code)]

use base64::Engine;
use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Deserialize, Clone, Debug)]
struct RpcCall {
    route: String,
    body: String,
    response: String,
    is_binary: bool,
}

#[derive(Clone, Debug)]
pub struct MockRpc {
    calls: Arc<Vec<RpcCall>>,
    call_sequence: Arc<Mutex<Vec<usize>>>,
    binary_files: Arc<HashMap<String, Vec<u8>>>,
}

/// Resize a recorded `get_outs` reply to match the requested output count.
///
/// Decoy selection is random, so requests vary between runs. The fixture
/// contains only one output, which this helper repeats as needed to preserve
/// the daemon's one-entry-per-requested-index contract. This does not make the
/// recorded keys match the randomly selected global indices.
fn resize_outs(mut call: RpcCall, wanted: usize) -> RpcCall {
    let Ok(mut resp) = serde_json::from_str::<serde_json::Value>(&call.response) else {
        return call;
    };
    let Some(outs) = resp.get_mut("outs").and_then(|v| v.as_array_mut()) else {
        return call;
    };
    if outs.len() == wanted || outs.is_empty() {
        return call;
    }

    if outs.len() > wanted {
        outs.truncate(wanted);
    } else {
        let recorded = outs.clone();
        while outs.len() < wanted {
            let next = recorded[outs.len() % recorded.len()].clone();
            outs.push(next);
        }
    }
    call.response = serde_json::to_string(&resp).expect("re-serialising a parsed response");
    call
}

#[derive(Deserialize)]
struct BinaryGetOutsRequest {
    #[allow(dead_code)]
    get_txid: bool,
    outputs: Vec<BinaryGetOut>,
}

#[derive(Deserialize)]
struct BinaryGetOut {
    #[allow(dead_code)]
    amount: u64,
    #[allow(dead_code)]
    index: u64,
}

#[derive(Serialize)]
struct BinaryGetOutsResponse {
    outs: Vec<BinaryGetOutKey>,
    status: String,
}

#[derive(Clone, Serialize)]
struct BinaryGetOutKey {
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
struct BinaryDistributionResponse {
    distributions: Vec<BinaryDistribution>,
    status: String,
}

#[derive(Serialize)]
struct BinaryDistribution {
    amount: u64,
    start_height: u64,
    binary: bool,
    compress: bool,
    #[serde(with = "serde_bytes")]
    compressed_data: Vec<u8>,
    base: u64,
}

#[derive(Deserialize)]
struct BinaryGetBlocksRequest {
    #[serde(with = "serde_bytes")]
    block_ids: Vec<u8>,
    start_height: u64,
    prune: bool,
    no_miner_tx: bool,
}

#[derive(Serialize)]
struct BinaryBlockEntryWithoutTxs {
    #[serde(with = "serde_bytes")]
    block: Vec<u8>,
    pruned: bool,
    block_weight: u64,
}

#[derive(Serialize)]
struct BinaryTxOutputIndices {
    indices: Vec<u64>,
}

#[derive(Serialize)]
struct BinaryBlockOutputIndices {
    indices: Vec<BinaryTxOutputIndices>,
}

#[derive(Serialize)]
struct BinaryGetBlocksResponse {
    status: String,
    blocks: Vec<BinaryBlockEntryWithoutTxs>,
    start_height: u64,
    current_height: u64,
    output_indices: Vec<BinaryBlockOutputIndices>,
    untrusted: bool,
    credits: u64,
    top_hash: String,
    daemon_time: u64,
}

fn push_varint(mut value: u64, bytes: &mut Vec<u8>) {
    while value >= 0x80 {
        bytes.push((value as u8) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
}

impl MockRpc {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read_to_string(path)?;
        let calls: Vec<RpcCall> = serde_json::from_str(&data)?;

        Ok(MockRpc {
            calls: Arc::new(calls),
            call_sequence: Arc::new(Mutex::new(Vec::new())),
            binary_files: Arc::new(HashMap::new()),
        })
    }

    pub fn from_json(json: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let calls: Vec<RpcCall> = serde_json::from_str(json)?;
        Ok(MockRpc {
            calls: Arc::new(calls),
            call_sequence: Arc::new(Mutex::new(Vec::new())),
            binary_files: Arc::new(HashMap::new()),
        })
    }

    pub fn with_binary_file<P: AsRef<Path>>(mut self, route: &str, path: P) -> Self {
        let bytes = std::fs::read(path).expect("failed to read binary fixture");
        Arc::make_mut(&mut self.binary_files).insert(route.to_string(), bytes);
        self
    }

    /// Serve a binary response assembled by a test.
    pub fn with_binary_response(mut self, route: &str, bytes: Vec<u8>) -> Self {
        Arc::make_mut(&mut self.binary_files).insert(route.to_string(), bytes);
        self
    }

    fn find_matching_call(&self, route: &str, body: &[u8]) -> Option<RpcCall> {
        if let Some(bytes) = self.binary_files.get(route) {
            return Some(RpcCall {
                route: route.to_string(),
                body: String::new(),
                response: base64::engine::general_purpose::STANDARD.encode(bytes),
                is_binary: true,
            });
        }

        for (idx, call) in self.calls.iter().enumerate() {
            if call.route != route {
                continue;
            }

            let already_used = self.call_sequence.lock().unwrap().contains(&idx);
            if already_used && route != "get_transactions" {
                continue;
            }

            if route == "get_height" {
                self.call_sequence.lock().unwrap().push(idx);
                return Some(call.clone());
            }

            if route == "json_rpc" {
                let body_str = String::from_utf8_lossy(body).to_string();
                if let (Ok(req), Ok(stored)) = (
                    serde_json::from_str::<serde_json::Value>(&body_str),
                    serde_json::from_str::<serde_json::Value>(&call.body),
                ) {
                    if req.get("method") == stored.get("method") {
                        match (req.get("params"), stored.get("params")) {
                            (Some(req_params), Some(stored_params)) => {
                                if req_params.get("height") == stored_params.get("height") {
                                    self.call_sequence.lock().unwrap().push(idx);
                                    return Some(call.clone());
                                }
                            }
                            (None, None) => {
                                self.call_sequence.lock().unwrap().push(idx);
                                return Some(call.clone());
                            }
                            _ => {}
                        }
                    }
                }
                continue;
            }

            if route == "get_transactions" {
                let body_str = String::from_utf8_lossy(body).to_string();
                if let (Ok(req), Ok(stored)) = (
                    serde_json::from_str::<serde_json::Value>(&body_str),
                    serde_json::from_str::<serde_json::Value>(&call.body),
                ) {
                    let req_hashes = req.get("txs_hashes").and_then(|v| v.as_array());
                    let stored_hashes = stored.get("txs_hashes").and_then(|v| v.as_array());

                    if let (Some(req_arr), Some(stored_arr)) = (req_hashes, stored_hashes) {
                        // Exact match
                        if req_arr == stored_arr {
                            return Some(call.clone());
                        }
                        // Check if all requested hashes are the same and match the single stored hash - need to adapt
                        if stored_arr.len() == 1 && req_arr.len() > 0 {
                            let stored_hash = &stored_arr[0];
                            if req_arr.iter().all(|h| h == stored_hash) {
                                // Duplicate the single transaction to match the request
                                if let Ok(mut resp) =
                                    serde_json::from_str::<serde_json::Value>(&call.response)
                                {
                                    if let Some(resp_txs) =
                                        resp.get_mut("txs").and_then(|v| v.as_array_mut())
                                    {
                                        if resp_txs.len() == 1 {
                                            let original_tx = resp_txs[0].clone();
                                            while resp_txs.len() < req_arr.len() {
                                                resp_txs.push(original_tx.clone());
                                            }

                                            // Also update txs_as_hex
                                            if let Some(resp_hex) = resp
                                                .get_mut("txs_as_hex")
                                                .and_then(|v| v.as_array_mut())
                                            {
                                                if resp_hex.len() == 1 {
                                                    let original_hex = resp_hex[0].clone();
                                                    while resp_hex.len() < req_arr.len() {
                                                        resp_hex.push(original_hex.clone());
                                                    }
                                                }
                                            }

                                            let mut matched_call = call.clone();
                                            matched_call.response =
                                                serde_json::to_string(&resp).unwrap();
                                            return Some(matched_call);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                continue;
            }

            if route == "get_outs" {
                let body_str = String::from_utf8_lossy(body).to_string();
                if let Ok(req) = serde_json::from_str::<serde_json::Value>(&body_str) {
                    let req_outputs = req.get("outputs").and_then(|v| v.as_array());

                    if let (Ok(stored), Some(req_arr)) = (
                        serde_json::from_str::<serde_json::Value>(&call.body),
                        req_outputs,
                    ) {
                        let stored_len = stored
                            .get("outputs")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len());

                        if stored_len == Some(req_arr.len()) {
                            return Some(resize_outs(call.clone(), req_arr.len()));
                        }
                    }
                }
            }

            if call.is_binary {
                if let Ok(stored_body) =
                    base64::engine::general_purpose::STANDARD.decode(&call.body)
                {
                    if body == stored_body.as_slice() {
                        self.call_sequence.lock().unwrap().push(idx);
                        return Some(call.clone());
                    }
                }
            } else {
                let body_str = String::from_utf8_lossy(body).to_string();
                if call.body == body_str {
                    self.call_sequence.lock().unwrap().push(idx);
                    return Some(call.clone());
                }
            }
        }

        if route == "json_rpc" {
            let request = serde_json::from_slice::<serde_json::Value>(body).ok();
            let requested_height = request
                .as_ref()
                .filter(|req| req["method"] == "get_block_header_by_height")
                .and_then(|req| req["params"]["height"].as_u64());
            if let Some(requested_height) = requested_height {
                for call in self.calls.iter().filter(|call| call.route == "json_rpc") {
                    let Ok(stored) = serde_json::from_str::<serde_json::Value>(&call.body) else {
                        continue;
                    };
                    if stored["method"] != "get_block_header_by_height"
                        || stored["params"]["height"].as_u64() != requested_height.checked_add(1)
                    {
                        continue;
                    }
                    let Ok(mut response) =
                        serde_json::from_str::<serde_json::Value>(&call.response)
                    else {
                        continue;
                    };
                    let Some(parent_hash) = response["result"]["block_header"]["prev_hash"]
                        .as_str()
                        .map(str::to_owned)
                    else {
                        continue;
                    };
                    response["result"]["block_header"]["hash"] = parent_hash.into();
                    response["result"]["block_header"]["height"] = requested_height.into();
                    let mut synthesized = call.clone();
                    synthesized.response = serde_json::to_string(&response).unwrap();
                    return Some(synthesized);
                }
            }
        }

        if route == "get_outs" {
            let body_str = String::from_utf8_lossy(body).to_string();
            if let Ok(req) = serde_json::from_str::<serde_json::Value>(&body_str) {
                let req_outputs = req.get("outputs").and_then(|v| v.as_array());

                if let Some(req_arr) = req_outputs {
                    let req_count = req_arr.len();
                    let mut best_call: Option<RpcCall> = None;
                    let mut best_score = i32::MIN;

                    for call in self.calls.iter() {
                        if call.route != "get_outs" {
                            continue;
                        }

                        if let Ok(stored) = serde_json::from_str::<serde_json::Value>(&call.body) {
                            if let Some(stored_outputs) =
                                stored.get("outputs").and_then(|v| v.as_array())
                            {
                                let stored_count = stored_outputs.len();
                                let score = if stored_count == req_count {
                                    1000
                                } else if stored_count > req_count {
                                    500 - (stored_count - req_count) as i32
                                } else {
                                    100 - (req_count - stored_count) as i32
                                };

                                if score > best_score {
                                    best_score = score;
                                    best_call = Some(call.clone());
                                }
                            }
                        }
                    }

                    if let Some(matched_call) = best_call {
                        return Some(resize_outs(matched_call, req_count));
                    }
                }
            }
        }

        // Try to adapt get_transactions responses
        if route == "get_transactions" {
            let body_str = String::from_utf8_lossy(body).to_string();
            if let Ok(req) = serde_json::from_str::<serde_json::Value>(&body_str) {
                let req_hashes = req.get("txs_hashes").and_then(|v| v.as_array());

                if let Some(req_arr) = req_hashes {
                    // Look for a single-tx call that can be duplicated
                    for call in self.calls.iter() {
                        if call.route != "get_transactions" {
                            continue;
                        }

                        if let Ok(stored) = serde_json::from_str::<serde_json::Value>(&call.body) {
                            if let Some(stored_hashes) =
                                stored.get("txs_hashes").and_then(|v| v.as_array())
                            {
                                if stored_hashes.len() == 1 && req_arr.len() > 0 {
                                    let stored_hash = &stored_hashes[0];
                                    if req_arr.iter().all(|h| h == stored_hash) {
                                        // Duplicate the single transaction to match the request
                                        if let Ok(mut resp) =
                                            serde_json::from_str::<serde_json::Value>(
                                                &call.response,
                                            )
                                        {
                                            if let Some(resp_txs) =
                                                resp.get_mut("txs").and_then(|v| v.as_array_mut())
                                            {
                                                if resp_txs.len() == 1 {
                                                    let original_tx = resp_txs[0].clone();
                                                    while resp_txs.len() < req_arr.len() {
                                                        resp_txs.push(original_tx.clone());
                                                    }

                                                    // Also update txs_as_hex
                                                    if let Some(resp_hex) = resp
                                                        .get_mut("txs_as_hex")
                                                        .and_then(|v| v.as_array_mut())
                                                    {
                                                        if resp_hex.len() == 1 {
                                                            let original_hex = resp_hex[0].clone();
                                                            while resp_hex.len() < req_arr.len() {
                                                                resp_hex.push(original_hex.clone());
                                                            }
                                                        }
                                                    }

                                                    let mut matched_call = call.clone();
                                                    matched_call.response =
                                                        serde_json::to_string(&resp).unwrap();
                                                    return Some(matched_call);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if route == "getblocks.bin" {
            let request: BinaryGetBlocksRequest = monero_epee_bin_serde::from_bytes(body).ok()?;
            let header_call = self.calls.iter().find(|call| {
                if call.route != "json_rpc" {
                    return false;
                }
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&call.body) else {
                    return false;
                };
                value["method"] == "get_block_header_by_height"
                    && value["params"]["height"].as_u64() == Some(request.start_height)
            })?;
            let header_response: serde_json::Value =
                serde_json::from_str(&header_call.response).ok()?;
            let block_hash = header_response["result"]["block_header"]["hash"].as_str()?;
            let call = self.calls.iter().find(|call| {
                if call.route != "json_rpc" {
                    return false;
                }
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&call.body) else {
                    return false;
                };
                value["method"] == "get_block"
                    && value["params"]["hash"].as_str() == Some(block_hash)
            })?;
            let response: serde_json::Value = serde_json::from_str(&call.response).ok()?;
            let result = response.get("result")?;
            let block = hex::decode(result.get("blob")?.as_str()?).ok()?;
            let block_weight = result
                .get("block_header")
                .and_then(|header| header.get("block_weight"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let current_height = self
                .calls
                .iter()
                .filter(|call| call.route == "get_height")
                .find_map(|call| {
                    serde_json::from_str::<serde_json::Value>(&call.response)
                        .ok()?
                        .get("height")?
                        .as_u64()
                })
                .unwrap_or(request.start_height + 1);
            let response = BinaryGetBlocksResponse {
                status: "OK".to_string(),
                blocks: vec![BinaryBlockEntryWithoutTxs {
                    block,
                    pruned: false,
                    block_weight,
                }],
                start_height: request.start_height,
                current_height,
                output_indices: vec![BinaryBlockOutputIndices {
                    indices: vec![BinaryTxOutputIndices {
                        indices: vec![request.start_height],
                    }],
                }],
                untrusted: false,
                credits: 0,
                top_hash: String::new(),
                daemon_time: 0,
            };
            let encoded = monero_epee_bin_serde::to_bytes(&response).ok()?;
            return Some(RpcCall {
                route: route.to_string(),
                body: String::new(),
                response: base64::engine::general_purpose::STANDARD.encode(encoded),
                is_binary: true,
            });
        }

        if route == "get_output_distribution.bin" {
            let call = self.calls.iter().find(|call| {
                call.route == "json_rpc"
                    && serde_json::from_str::<serde_json::Value>(&call.body)
                        .ok()
                        .and_then(|value| value.get("method").cloned())
                        == Some(serde_json::Value::String(
                            "get_output_distribution".to_string(),
                        ))
            })?;
            let response: serde_json::Value = serde_json::from_str(&call.response).ok()?;
            let distribution = response
                .get("result")?
                .get("distributions")?
                .as_array()?
                .first()?;
            let cumulative = distribution.get("distribution")?.as_array()?;
            let mut previous = 0u64;
            let mut compressed = Vec::with_capacity(cumulative.len());
            for value in cumulative {
                let value = value.as_u64()?;
                push_varint(value.checked_sub(previous)?, &mut compressed);
                previous = value;
            }
            let response = BinaryDistributionResponse {
                distributions: vec![BinaryDistribution {
                    amount: 0,
                    start_height: distribution.get("start_height")?.as_u64()?,
                    binary: true,
                    compress: true,
                    compressed_data: compressed,
                    base: 0,
                }],
                status: "OK".to_string(),
            };
            return Some(RpcCall {
                route: route.to_string(),
                body: String::new(),
                response: base64::engine::general_purpose::STANDARD
                    .encode(monero_epee_bin_serde::to_bytes(&response).ok()?),
                is_binary: true,
            });
        }

        if route == "get_outs.bin" {
            let request: BinaryGetOutsRequest = monero_epee_bin_serde::from_bytes(body).ok()?;
            let call = self.calls.iter().find(|call| call.route == "get_outs")?;
            let response: serde_json::Value = serde_json::from_str(&call.response).ok()?;
            let output = response.get("outs")?.as_array()?.first()?;
            let template = BinaryGetOutKey {
                key: hex::decode(output.get("key")?.as_str()?).ok()?,
                mask: hex::decode(output.get("mask")?.as_str()?).ok()?,
                unlocked: output.get("unlocked")?.as_bool()?,
                height: output.get("height")?.as_u64()?,
                txid: hex::decode(output.get("txid")?.as_str()?).ok()?,
            };
            let response = BinaryGetOutsResponse {
                outs: vec![template; request.outputs.len()],
                status: "OK".to_string(),
            };
            return Some(RpcCall {
                route: route.to_string(),
                body: String::new(),
                response: base64::engine::general_purpose::STANDARD
                    .encode(monero_epee_bin_serde::to_bytes(&response).ok()?),
                is_binary: true,
            });
        }

        None
    }
}

#[async_trait::async_trait]
impl RpcConnection for MockRpc {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let call = self
            .find_matching_call(route, &body)
            .ok_or(RpcError::InvalidNode)?;

        if call.is_binary {
            base64::engine::general_purpose::STANDARD
                .decode(&call.response)
                .map_err(|_| RpcError::InvalidNode)
        } else {
            Ok(call.response.as_bytes().to_vec())
        }
    }
}

#[derive(Clone, Debug)]
pub struct SafeMockRpc {
    inner: MockRpc,
}

impl SafeMockRpc {
    pub fn new(mock: MockRpc) -> Self {
        SafeMockRpc { inner: mock }
    }
}

#[async_trait::async_trait]
impl RpcConnection for SafeMockRpc {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        self.inner.post(route, body).await
    }
}

pub fn create_rpc(mock: MockRpc) -> Rpc<SafeMockRpc> {
    Rpc::new_with_connection(SafeMockRpc::new(mock))
}
