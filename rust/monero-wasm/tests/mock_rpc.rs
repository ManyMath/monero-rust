use std::path::Path;
use std::collections::HashMap;
use monero_serai::rpc::{RpcConnection, RpcError};
use serde::Deserialize;
use base64::Engine;

#[derive(Deserialize)]
struct RpcCall {
    route: String,
    #[allow(dead_code)]
    body: String,
    response: String,
    is_binary: bool,
}

#[derive(Clone, Debug)]
pub struct MockRpc {
    calls: HashMap<String, (String, bool)>,
}

impl MockRpc {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read_to_string(path)?;
        let calls_vec: Vec<RpcCall> = serde_json::from_str(&data)?;

        let mut calls = HashMap::new();
        for call in calls_vec {
            calls.insert(call.route, (call.response, call.is_binary));
        }

        Ok(MockRpc { calls })
    }
}

#[async_trait::async_trait]
impl RpcConnection for MockRpc {
    async fn post(&self, route: &str, _body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        self.calls
            .get(route)
            .map(|(response, is_binary)| {
                if *is_binary {
                    // Decode base64 for binary responses
                    base64::engine::general_purpose::STANDARD
                        .decode(response)
                        .unwrap_or_else(|_| response.as_bytes().to_vec())
                } else {
                    response.as_bytes().to_vec()
                }
            })
            .ok_or(RpcError::ConnectionError)
    }
}

/// For testing
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

/// For testing.
pub fn create_rpc(mock: MockRpc) -> monero_serai::rpc::Rpc<SafeMockRpc> {
    monero_serai::rpc::Rpc::new_with_connection(SafeMockRpc::new(mock))
}
