use std::path::Path;
use std::sync::{Arc, Mutex};
use monero_serai::rpc::{RpcConnection, RpcError};
use serde::Deserialize;
use base64::Engine;

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
}

impl MockRpc {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read_to_string(path)?;
        let calls: Vec<RpcCall> = serde_json::from_str(&data)?;

        Ok(MockRpc {
            calls: Arc::new(calls),
            call_sequence: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn find_matching_call(&self, route: &str, body: &[u8]) -> Option<RpcCall> {
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
                    serde_json::from_str::<serde_json::Value>(&call.body)
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
                    serde_json::from_str::<serde_json::Value>(&call.body)
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
                                if let Ok(mut resp) = serde_json::from_str::<serde_json::Value>(&call.response) {
                                    if let Some(resp_txs) = resp.get_mut("txs").and_then(|v| v.as_array_mut()) {
                                        if resp_txs.len() == 1 {
                                            let original_tx = resp_txs[0].clone();
                                            while resp_txs.len() < req_arr.len() {
                                                resp_txs.push(original_tx.clone());
                                            }

                                            // Also update txs_as_hex
                                            if let Some(resp_hex) = resp.get_mut("txs_as_hex").and_then(|v| v.as_array_mut()) {
                                                if resp_hex.len() == 1 {
                                                    let original_hex = resp_hex[0].clone();
                                                    while resp_hex.len() < req_arr.len() {
                                                        resp_hex.push(original_hex.clone());
                                                    }
                                                }
                                            }

                                            let mut matched_call = call.clone();
                                            matched_call.response = serde_json::to_string(&resp).unwrap();
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
                        req_outputs
                    ) {
                        let stored_len = stored.get("outputs").and_then(|v| v.as_array()).map(|a| a.len());

                        if stored_len == Some(req_arr.len()) {
                            return Some(call.clone());
                        }
                    }
                }
            }

            if call.is_binary {
                if let Ok(stored_body) = base64::engine::general_purpose::STANDARD.decode(&call.body) {
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
                            if let Some(stored_outputs) = stored.get("outputs").and_then(|v| v.as_array()) {
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

                    if let Some(mut matched_call) = best_call {
                        if let Ok(mut resp) = serde_json::from_str::<serde_json::Value>(&matched_call.response) {
                            if let Some(resp_outs) = resp.get_mut("outs").and_then(|v| v.as_array_mut()) {
                                let original_len = resp_outs.len();

                                if original_len != req_count {
                                    if original_len > req_count {
                                        resp_outs.truncate(req_count);
                                    } else {
                                        let original_outs = resp_outs.clone();
                                        while resp_outs.len() < req_count {
                                            let idx = resp_outs.len() % original_outs.len();
                                            resp_outs.push(original_outs[idx].clone());
                                        }
                                    }
                                    matched_call.response = serde_json::to_string(&resp).unwrap();
                                }

                                return Some(matched_call);
                            }
                        }
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
                            if let Some(stored_hashes) = stored.get("txs_hashes").and_then(|v| v.as_array()) {
                                if stored_hashes.len() == 1 && req_arr.len() > 0 {
                                    let stored_hash = &stored_hashes[0];
                                    if req_arr.iter().all(|h| h == stored_hash) {
                                        // Duplicate the single transaction to match the request
                                        if let Ok(mut resp) = serde_json::from_str::<serde_json::Value>(&call.response) {
                                            if let Some(resp_txs) = resp.get_mut("txs").and_then(|v| v.as_array_mut()) {
                                                if resp_txs.len() == 1 {
                                                    let original_tx = resp_txs[0].clone();
                                                    while resp_txs.len() < req_arr.len() {
                                                        resp_txs.push(original_tx.clone());
                                                    }

                                                    // Also update txs_as_hex
                                                    if let Some(resp_hex) = resp.get_mut("txs_as_hex").and_then(|v| v.as_array_mut()) {
                                                        if resp_hex.len() == 1 {
                                                            let original_hex = resp_hex[0].clone();
                                                            while resp_hex.len() < req_arr.len() {
                                                                resp_hex.push(original_hex.clone());
                                                            }
                                                        }
                                                    }

                                                    let mut matched_call = call.clone();
                                                    matched_call.response = serde_json::to_string(&resp).unwrap();
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

        None
    }
}

#[async_trait::async_trait]
impl RpcConnection for MockRpc {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let call = self.find_matching_call(route, &body).ok_or(RpcError::InvalidNode)?;

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

pub fn create_rpc(mock: MockRpc) -> monero_serai::rpc::Rpc<SafeMockRpc> {
    monero_serai::rpc::Rpc::new_with_connection(SafeMockRpc::new(mock))
}
