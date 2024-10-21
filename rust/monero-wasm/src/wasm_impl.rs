//! WASM-specific implementations of platform abstractions

#![cfg(target_arch = "wasm32")]

use crate::abstractions::{
    AbError, AbResult, BlockResponse, GetOutsParams, OutsResponse, RpcClient, TimeProvider,
    TxSubmitResponse, WalletStorage,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use js_sys::Date;
use serde::Deserialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, RequestCredentials, Response, Storage};

/// Browser-based storage using localStorage
pub struct BrowserStorage {
    prefix: String,
}

impl BrowserStorage {
    pub fn new(prefix: impl Into<String>) -> AbResult<Self> {
        Ok(Self {
            prefix: prefix.into(),
        })
    }

    fn get_storage(&self) -> AbResult<Storage> {
        let window = web_sys::window().ok_or_else(|| AbError::Storage("No window object".into()))?;
        window
            .local_storage()
            .map_err(|e| AbError::Storage(format!("Failed to access localStorage: {:?}", e)))?
            .ok_or_else(|| AbError::Storage("localStorage not available".into()))
    }

    fn full_key(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }

    /// Validate key to prevent XSS attacks via localStorage keys
    /// Uses whitelist approach for maximum security
    fn validate_key(&self, key: &str) -> AbResult<()> {
        // Reject empty keys
        if key.is_empty() {
            return Err(AbError::InvalidData("Key cannot be empty".into()));
        }

        // Reject excessively long keys (max 256 chars)
        if key.len() > 256 {
            return Err(AbError::InvalidData("Key exceeds maximum length of 256 characters".into()));
        }

        // Whitelist approach: only allow alphanumeric, hyphen, underscore, and dot
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
            return Err(AbError::InvalidData(
                "Key must contain only alphanumeric characters, hyphens, underscores, or dots".into()
            ));
        }

        // Prevent path traversal attacks
        if key.contains("..") {
            return Err(AbError::InvalidData("Key cannot contain '..' (path traversal)".into()));
        }

        Ok(())
    }
}

#[async_trait(?Send)]
impl WalletStorage for BrowserStorage {
    async fn save(&self, key: &str, data: &[u8]) -> AbResult<()> {
        self.validate_key(key)?;
        let storage = self.get_storage()?;
        let full_key = self.full_key(key);

        let encoded = base64_encode(data);

        storage
            .set_item(&full_key, &encoded)
            .map_err(|e| AbError::Storage(format!("Failed to save: {:?}", e)))?;

        Ok(())
    }

    async fn load(&self, key: &str) -> AbResult<Vec<u8>> {
        self.validate_key(key)?;
        let storage = self.get_storage()?;
        let full_key = self.full_key(key);

        let encoded = storage
            .get_item(&full_key)
            .map_err(|e| AbError::Storage(format!("Failed to load: {:?}", e)))?
            .ok_or_else(|| AbError::NotFound(format!("Key '{}' not found", key)))?;

        base64_decode(&encoded)
    }

    async fn delete(&self, key: &str) -> AbResult<()> {
        self.validate_key(key)?;
        let storage = self.get_storage()?;
        let full_key = self.full_key(key);

        storage
            .remove_item(&full_key)
            .map_err(|e| AbError::Storage(format!("Failed to delete: {:?}", e)))?;

        Ok(())
    }

    async fn list_keys(&self) -> AbResult<Vec<String>> {
        let storage = self.get_storage()?;
        let mut keys = Vec::new();

        let length = storage
            .length()
            .map_err(|e| AbError::Storage(format!("Failed to get length: {:?}", e)))?;

        // Limit to prevent unbounded memory growth (max 10000 keys)
        const MAX_KEYS: u32 = 10000;
        if length > MAX_KEYS {
            return Err(AbError::Storage(format!(
                "Too many keys in storage: {} (max {}). Consider implementing pagination.",
                length, MAX_KEYS
            )));
        }

        keys.reserve(length.min(1000) as usize); // Reserve reasonable capacity

        for i in 0..length {
            if let Ok(Some(key)) = storage.key(i) {
                if let Some(stripped) = key.strip_prefix(&self.prefix) {
                    keys.push(stripped.to_string());
                }
            }
        }

        Ok(keys)
    }

    async fn exists(&self, key: &str) -> AbResult<bool> {
        self.validate_key(key)?;
        let storage = self.get_storage()?;
        let full_key = self.full_key(key);

        Ok(storage
            .get_item(&full_key)
            .map_err(|e| AbError::Storage(format!("Failed to check existence: {:?}", e)))?
            .is_some())
    }
}

/// Browser-based time provider using JavaScript Date API
pub struct JsTimeProvider;

impl JsTimeProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsTimeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeProvider for JsTimeProvider {
    fn now(&self) -> u64 {
        (Date::now() / 1000.0) as u64
    }

    fn now_ms(&self) -> u64 {
        Date::now() as u64
    }
}

/// WASM-compatible RPC client using browser's fetch API
pub struct WasmRpcClient {
    endpoint: String,
}

impl WasmRpcClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    async fn fetch_json(&self, method: &str, params: Value) -> AbResult<Value> {
        let window = web_sys::window()
            .ok_or_else(|| AbError::Network("No window object available".into()))?;

        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "0",
            "method": method,
            "params": params,
        });

        let body_str = serde_json::to_string(&request_body)
            .map_err(|e| AbError::Serialization(format!("Failed to serialize request: {}", e)))?;

        let opts = RequestInit::new();
        opts.set_method("POST");
        opts.set_mode(RequestMode::Cors);
        // Set credentials to 'same-origin' for better security
        // This prevents credentials being sent to cross-origin requests
        opts.set_credentials(RequestCredentials::SameOrigin);
        opts.set_body(&JsValue::from_str(&body_str));

        let request = Request::new_with_str_and_init(&self.endpoint, &opts)
            .map_err(|e| AbError::Network(format!("Failed to create request: {:?}", e)))?;

        request
            .headers()
            .set("Content-Type", "application/json")
            .map_err(|e| AbError::Network(format!("Failed to set headers: {:?}", e)))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| AbError::Network(format!("Fetch failed: {:?}", e)))?;

        let resp: Response = resp_value
            .dyn_into()
            .map_err(|e| AbError::Network(format!("Invalid response type: {:?}", e)))?;

        // Capture HTTP status code for better error context
        let status = resp.status();
        let status_text = resp.status_text();

        let text_promise = resp
            .text()
            .map_err(|e| AbError::Network(format!("Failed to get response text: {:?}", e)))?;

        let text_value = JsFuture::from(text_promise)
            .await
            .map_err(|e| AbError::Network(format!("Failed to read response: {:?}", e)))?;

        let text = text_value
            .as_string()
            .ok_or_else(|| AbError::Network("Response is not a string".into()))?;

        // Check for HTTP errors before parsing JSON
        if status < 200 || status >= 300 {
            return Err(AbError::Rpc(format!(
                "HTTP error {}: {} - Response: {}",
                status, status_text, text
            )));
        }

        let response: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AbError::Serialization(format!("Failed to parse JSON: {} - Response: {}", e, text)))?;

        if let Some(error) = response.get("error") {
            return Err(AbError::Rpc(format!(
                "RPC error (HTTP {}): {} - Full error: {}",
                status, status_text, error
            )));
        }

        response
            .get("result")
            .cloned()
            .ok_or_else(|| AbError::Rpc(format!(
                "No result in response (HTTP {}): {}",
                status, text
            )))
    }
}

#[async_trait(?Send)]
impl RpcClient for WasmRpcClient {
    async fn call<T>(&self, method: &str, params: Value) -> AbResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let result = self.fetch_json(method, params).await?;
        serde_json::from_value(result)
            .map_err(|e| AbError::Serialization(format!("Failed to deserialize result: {}", e)))
    }

    async fn get_height(&self) -> AbResult<u64> {
        let result = self.fetch_json("get_height", serde_json::json!({})).await?;

        result
            .get("height")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| AbError::Rpc("Invalid height response".into()))
    }

    async fn get_blocks(&self, start_height: u64, count: u64) -> AbResult<BlockResponse> {
        // Validate input parameters to prevent DoS attacks
        const MAX_BLOCK_COUNT: u64 = 100;
        if count == 0 {
            return Err(AbError::InvalidData("Block count must be greater than 0".into()));
        }
        if count > MAX_BLOCK_COUNT {
            return Err(AbError::InvalidData(format!(
                "Block count {} exceeds maximum allowed {} (prevents DoS)",
                count, MAX_BLOCK_COUNT
            )));
        }

        let params = serde_json::json!({
            "start_height": start_height,
            "count": count,
        });

        self.call("get_blocks", params).await
    }

    async fn get_outs(&self, params: &GetOutsParams) -> AbResult<OutsResponse> {
        let params_json = serde_json::to_value(params)
            .map_err(|e| AbError::Serialization(format!("Failed to serialize params: {}", e)))?;

        self.call("get_outs", params_json).await
    }

    async fn submit_transaction(&self, tx_blob: &str) -> AbResult<TxSubmitResponse> {
        let params = serde_json::json!({
            "tx_as_hex": tx_blob,
        });

        self.call("send_raw_transaction", params).await
    }

    async fn get_fee_estimate(&self) -> AbResult<u64> {
        let result = self
            .fetch_json("get_fee_estimate", serde_json::json!({}))
            .await?;

        result
            .get("fee")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| AbError::Rpc("Invalid fee estimate response".into()))
    }
}

/// Callback-based RPC client that delegates to JavaScript/Dart
pub struct CallbackRpcClient {
    callback: js_sys::Function,
}

impl CallbackRpcClient {
    pub fn new(callback: js_sys::Function) -> Self {
        Self { callback }
    }

    async fn call_via_callback(&self, method: &str, params: Value) -> AbResult<Value> {
        let this = JsValue::null();
        let method_js = JsValue::from_str(method);
        let params_str = serde_json::to_string(&params)
            .map_err(|e| AbError::Serialization(format!("Failed to serialize params: {}", e)))?;
        let params_js = JsValue::from_str(&params_str);

        let promise = self
            .callback
            .call2(&this, &method_js, &params_js)
            .map_err(|e| AbError::Rpc(format!("Callback invocation failed: {:?}", e)))?;

        let result = JsFuture::from(js_sys::Promise::from(promise))
            .await
            .map_err(|e| AbError::Rpc(format!("Callback promise rejected: {:?}", e)))?;

        let result_str = result
            .as_string()
            .ok_or_else(|| AbError::Rpc("Callback result is not a string".into()))?;

        serde_json::from_str(&result_str)
            .map_err(|e| AbError::Serialization(format!("Failed to parse callback result: {}", e)))
    }
}

#[async_trait(?Send)]
impl RpcClient for CallbackRpcClient {
    async fn call<T>(&self, method: &str, params: Value) -> AbResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let result = self.call_via_callback(method, params).await?;
        serde_json::from_value(result)
            .map_err(|e| AbError::Serialization(format!("Failed to deserialize result: {}", e)))
    }

    async fn get_height(&self) -> AbResult<u64> {
        let result = self
            .call_via_callback("get_height", serde_json::json!({}))
            .await?;

        result
            .get("height")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| AbError::Rpc("Invalid height response".into()))
    }

    async fn get_blocks(&self, start_height: u64, count: u64) -> AbResult<BlockResponse> {
        // Validate input parameters to prevent DoS attacks
        const MAX_BLOCK_COUNT: u64 = 100;
        if count == 0 {
            return Err(AbError::InvalidData("Block count must be greater than 0".into()));
        }
        if count > MAX_BLOCK_COUNT {
            return Err(AbError::InvalidData(format!(
                "Block count {} exceeds maximum allowed {} (prevents DoS)",
                count, MAX_BLOCK_COUNT
            )));
        }

        let params = serde_json::json!({
            "start_height": start_height,
            "count": count,
        });

        self.call("get_blocks", params).await
    }

    async fn get_outs(&self, params: &GetOutsParams) -> AbResult<OutsResponse> {
        let params_json = serde_json::to_value(params)
            .map_err(|e| AbError::Serialization(format!("Failed to serialize params: {}", e)))?;

        self.call("get_outs", params_json).await
    }

    async fn submit_transaction(&self, tx_blob: &str) -> AbResult<TxSubmitResponse> {
        let params = serde_json::json!({
            "tx_as_hex": tx_blob,
        });

        self.call("send_raw_transaction", params).await
    }

    async fn get_fee_estimate(&self) -> AbResult<u64> {
        let result = self
            .call_via_callback("get_fee_estimate", serde_json::json!({}))
            .await?;

        result
            .get("fee")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| AbError::Rpc("Invalid fee estimate response".into()))
    }
}

/// Encode bytes as base64 string using proper cryptographic base64
fn base64_encode(data: &[u8]) -> String {
    general_purpose::STANDARD.encode(data)
}

/// Decode base64 string to bytes using proper cryptographic base64
fn base64_decode(encoded: &str) -> AbResult<Vec<u8>> {
    general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| AbError::Storage(format!("Base64 decode failed: {}", e)))
}
