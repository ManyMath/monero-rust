//! Test API for WASM
//!
//! Bindings to test Monero functions from JS.

use wasm_bindgen::prelude::*;
use serde_wasm_bindgen::to_value;
use monero_rust::{generate_seed, derive_address, derive_keys};

#[wasm_bindgen]
pub struct TestApi;

#[wasm_bindgen]
impl TestApi {
    /// Generate seed
    #[wasm_bindgen]
    pub fn generate_seed() -> Result<String, JsValue> {
        generate_seed()
            .map_err(|e| JsValue::from_str(&format!("seed gen failed: {}", e)))
    }

    // Derive address from seed
    #[wasm_bindgen]
    pub fn derive_address(seed: &str, network: &str) -> Result<String, JsValue> {
        derive_address(seed, network)
            .map_err(|e| JsValue::from_str(&format!("addr failed: {}", e)))
    }

    #[wasm_bindgen]
    pub fn derive_keys(seed: &str, network: &str) -> Result<JsValue, JsValue> {
        let keys = derive_keys(seed, network)
            .map_err(|e| JsValue::from_str(&format!("keys failed: {}", e)))?;

        // TODO: optimize serialization
        to_value(&keys)
            .map_err(|e| JsValue::from_str(&format!("serialize failed: {}", e)))
    }

    #[wasm_bindgen]
    pub fn test_wasm() -> String {
        "WASM OK".to_string()
    }
}
