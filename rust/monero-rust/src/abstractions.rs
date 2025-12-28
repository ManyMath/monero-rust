//! Platform abstraction traits for WASM compatibility

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub type AbResult<T> = Result<T, AbError>;
#[derive(Debug, thiserror::Error)]
pub enum AbError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// Trait for persistent key-value storage
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait WalletStorage {
    async fn save(&self, key: &str, data: &[u8]) -> AbResult<()>;
    async fn load(&self, key: &str) -> AbResult<Vec<u8>>;
    async fn delete(&self, key: &str) -> AbResult<()>;
    async fn list_keys(&self) -> AbResult<Vec<String>>;
    async fn exists(&self, key: &str) -> AbResult<bool>;
}

pub trait TimeProvider {
    fn now(&self) -> u64;
    fn now_ms(&self) -> u64;
}

/// Response from get_height RPC call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeightResponse {
    pub height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockResponse {
    pub blocks: Vec<BlockData>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockData {
    pub block_header: BlockHeader,
    pub txs: Vec<TransactionData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub height: u64,
    pub timestamp: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionData {
    pub tx_hash: String,
    pub tx_blob: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutsResponse {
    pub outs: Vec<OutEntry>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutEntry {
    pub height: u64,
    pub key: String,
    pub mask: String,
    pub txid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxSubmitResponse {
    pub status: String,
    pub tx_hash: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOutsParams {
    pub outputs: Vec<OutputIndex>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputIndex {
    pub amount: u64,
    pub index: u64,
}

/// Trait for async RPC client operations
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait RpcClient {
    async fn call<T>(&self, method: &str, params: Value) -> AbResult<T>
    where
        T: for<'de> Deserialize<'de>;

    async fn get_height(&self) -> AbResult<u64>;
    async fn get_blocks(&self, start_height: u64, count: u64) -> AbResult<BlockResponse>;
    async fn get_outs(&self, params: &GetOutsParams) -> AbResult<OutsResponse>;
    async fn submit_transaction(&self, tx_blob: &str) -> AbResult<TxSubmitResponse>;
    async fn get_fee_estimate(&self) -> AbResult<u64>;
}

/// In-memory storage for testing
#[cfg(not(target_arch = "wasm32"))]
pub struct MemoryStorage {
    data: std::sync::Arc<std::sync::Mutex<HashMap<String, Vec<u8>>>>,
}

#[cfg(target_arch = "wasm32")]
pub struct MemoryStorage {
    data: std::rc::Rc<std::cell::RefCell<HashMap<String, Vec<u8>>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            data: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            data: std::rc::Rc::new(std::cell::RefCell::new(HashMap::new())),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl WalletStorage for MemoryStorage {
    async fn save(&self, key: &str, data: &[u8]) -> AbResult<()> {
        let mut storage = self
            .data
            .lock()
            .map_err(|e| AbError::Storage(format!("Mutex lock poisoned: {}", e)))?;
        storage.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn load(&self, key: &str) -> AbResult<Vec<u8>> {
        let storage = self
            .data
            .lock()
            .map_err(|e| AbError::Storage(format!("Mutex lock poisoned: {}", e)))?;
        storage
            .get(key)
            .cloned()
            .ok_or_else(|| AbError::NotFound(format!("Key '{}' not found", key)))
    }

    async fn delete(&self, key: &str) -> AbResult<()> {
        let mut storage = self
            .data
            .lock()
            .map_err(|e| AbError::Storage(format!("Mutex lock poisoned: {}", e)))?;
        storage.remove(key);
        Ok(())
    }

    async fn list_keys(&self) -> AbResult<Vec<String>> {
        let storage = self
            .data
            .lock()
            .map_err(|e| AbError::Storage(format!("Mutex lock poisoned: {}", e)))?;
        Ok(storage.keys().cloned().collect())
    }

    async fn exists(&self, key: &str) -> AbResult<bool> {
        let storage = self
            .data
            .lock()
            .map_err(|e| AbError::Storage(format!("Mutex lock poisoned: {}", e)))?;
        Ok(storage.contains_key(key))
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl WalletStorage for MemoryStorage {
    async fn save(&self, key: &str, data: &[u8]) -> AbResult<()> {
        let mut storage = self.data.borrow_mut();
        storage.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn load(&self, key: &str) -> AbResult<Vec<u8>> {
        let storage = self.data.borrow();
        storage
            .get(key)
            .cloned()
            .ok_or_else(|| AbError::NotFound(format!("Key '{}' not found", key)))
    }

    async fn delete(&self, key: &str) -> AbResult<()> {
        let mut storage = self.data.borrow_mut();
        storage.remove(key);
        Ok(())
    }

    async fn list_keys(&self) -> AbResult<Vec<String>> {
        let storage = self.data.borrow();
        Ok(storage.keys().cloned().collect())
    }

    async fn exists(&self, key: &str) -> AbResult<bool> {
        let storage = self.data.borrow();
        Ok(storage.contains_key(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_storage() {
        let storage = MemoryStorage::new();

        let data = b"test data";
        storage.save("key1", data).await.unwrap();
        let loaded = storage.load("key1").await.unwrap();
        assert_eq!(loaded, data);

        assert!(storage.exists("key1").await.unwrap());
        assert!(!storage.exists("key2").await.unwrap());

        storage.save("key2", b"more data").await.unwrap();
        let keys = storage.list_keys().await.unwrap();
        assert_eq!(keys.len(), 2);

        storage.delete("key1").await.unwrap();
        assert!(!storage.exists("key1").await.unwrap());
        assert!(storage.load("key1").await.is_err());
    }

    #[tokio::test]
    async fn test_memory_storage_overwrite() {
        let storage = MemoryStorage::new();

        storage.save("key", b"original").await.unwrap();
        let loaded1 = storage.load("key").await.unwrap();
        assert_eq!(loaded1, b"original");

        // Overwrite with new data
        storage.save("key", b"updated").await.unwrap();
        let loaded2 = storage.load("key").await.unwrap();
        assert_eq!(loaded2, b"updated");
    }

    #[tokio::test]
    async fn test_memory_storage_load_nonexistent() {
        let storage = MemoryStorage::new();

        let result = storage.load("nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AbError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_memory_storage_delete_nonexistent() {
        let storage = MemoryStorage::new();

        // Delete should not error on nonexistent keys
        storage.delete("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_memory_storage_empty_list() {
        let storage = MemoryStorage::new();

        let keys = storage.list_keys().await.unwrap();
        assert_eq!(keys.len(), 0);
    }

    #[tokio::test]
    async fn test_memory_storage_binary_data() {
        let storage = MemoryStorage::new();

        // Test with binary data containing nulls
        let binary_data = vec![0x00, 0x01, 0xFF, 0xDE, 0xAD, 0xBE, 0xEF];
        storage.save("binary", &binary_data).await.unwrap();
        let loaded = storage.load("binary").await.unwrap();
        assert_eq!(loaded, binary_data);
    }

    #[tokio::test]
    async fn test_memory_storage_large_data() {
        let storage = MemoryStorage::new();

        // Test with 1MB of data
        let large_data = vec![42u8; 1024 * 1024];
        storage.save("large", &large_data).await.unwrap();
        let loaded = storage.load("large").await.unwrap();
        assert_eq!(loaded.len(), large_data.len());
        assert_eq!(loaded, large_data);
    }

    #[tokio::test]
    async fn test_memory_storage_special_keys() {
        let storage = MemoryStorage::new();

        // Test with special characters in keys
        let keys = vec![
            "key/with/slashes",
            "key.with.dots",
            "key-with-dashes",
            "key_with_underscores",
        ];

        for key in keys {
            storage.save(key, b"data").await.unwrap();
            assert!(storage.exists(key).await.unwrap());
            let loaded = storage.load(key).await.unwrap();
            assert_eq!(loaded, b"data");
        }
    }

    #[test]
    fn test_aberror_display() {
        let storage_err = AbError::Storage("test error".to_string());
        assert!(storage_err.to_string().contains("Storage error"));

        let rpc_err = AbError::Rpc("rpc failed".to_string());
        assert!(rpc_err.to_string().contains("RPC error"));

        let not_found_err = AbError::NotFound("key".to_string());
        assert!(not_found_err.to_string().contains("Not found"));
    }

    #[test]
    fn test_height_response_serialization() {
        let response = HeightResponse { height: 12345 };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: HeightResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.height, deserialized.height);
    }

    #[test]
    fn test_block_header_serialization() {
        let header = BlockHeader {
            height: 100,
            timestamp: 1234567890,
            hash: "abc123".to_string(),
        };
        let json = serde_json::to_string(&header).unwrap();
        let deserialized: BlockHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(header.height, deserialized.height);
        assert_eq!(header.timestamp, deserialized.timestamp);
        assert_eq!(header.hash, deserialized.hash);
    }

    #[test]
    fn test_transaction_data_serialization() {
        let tx = TransactionData {
            tx_hash: "deadbeef".to_string(),
            tx_blob: "blobdata".to_string(),
        };
        let json = serde_json::to_string(&tx).unwrap();
        let deserialized: TransactionData = serde_json::from_str(&json).unwrap();
        assert_eq!(tx.tx_hash, deserialized.tx_hash);
        assert_eq!(tx.tx_blob, deserialized.tx_blob);
    }

    #[test]
    fn test_output_index_serialization() {
        let output = OutputIndex {
            amount: 1000000,
            index: 5,
        };
        let json = serde_json::to_string(&output).unwrap();
        let deserialized: OutputIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(output.amount, deserialized.amount);
        assert_eq!(output.index, deserialized.index);
    }

    #[test]
    fn test_tx_submit_response_success() {
        let response = TxSubmitResponse {
            status: "OK".to_string(),
            tx_hash: Some("abc123".to_string()),
            reason: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: TxSubmitResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.status, deserialized.status);
        assert_eq!(response.tx_hash, deserialized.tx_hash);
    }

    #[test]
    fn test_tx_submit_response_failure() {
        let response = TxSubmitResponse {
            status: "FAILED".to_string(),
            tx_hash: None,
            reason: Some("double spend".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: TxSubmitResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response.status, deserialized.status);
        assert_eq!(response.reason, deserialized.reason);
    }
}
