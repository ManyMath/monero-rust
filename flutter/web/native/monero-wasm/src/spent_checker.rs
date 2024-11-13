use monero_serai::rpc::RpcConnection;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[cfg(not(target_arch = "wasm32"))]
use monero_serai::rpc::HttpRpc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyImageSpentStatus {
    pub key_image: String,
    pub spent: bool,
}

pub async fn check_key_images_spent<R: RpcConnection>(
    rpc_connection: &R,
    key_images_hex: &[String],
) -> Result<Vec<KeyImageSpentStatus>, String> {
    if key_images_hex.is_empty() {
        return Ok(Vec::new());
    }

    let request_body = json!({
        "jsonrpc": "2.0",
        "id": "0",
        "method": "is_key_image_spent",
        "params": {
            "key_images": key_images_hex
        }
    });

    let request_bytes = serde_json::to_vec(&request_body)
        .map_err(|e| format!("Failed to serialize request: {}", e))?;

    let response_bytes = rpc_connection
        .post("json_rpc", request_bytes)
        .await
        .map_err(|e| format!("RPC error: {:?}", e))?;

    #[derive(Deserialize)]
    struct RpcResponse {
        result: IsKeyImageSpentResult,
    }

    #[derive(Debug, Deserialize)]
    struct IsKeyImageSpentResult {
        spent_status: Vec<u32>,
    }

    let response: RpcResponse = serde_json::from_slice(&response_bytes)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if response.result.spent_status.len() != key_images_hex.len() {
        return Err(format!(
            "Response length mismatch: expected {}, got {}",
            key_images_hex.len(),
            response.result.spent_status.len()
        ));
    }

    let results = key_images_hex
        .iter()
        .zip(response.result.spent_status.iter())
        .map(|(key_image, &status)| KeyImageSpentStatus {
            key_image: key_image.clone(),
            spent: status > 0,
        })
        .collect();

    Ok(results)
}

pub fn batch_key_images(key_images: &[String], batch_size: usize) -> Vec<Vec<String>> {
    key_images
        .chunks(batch_size)
        .map(|chunk| chunk.to_vec())
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn check_key_images_spent_native(
    node_url: &str,
    key_images: &[String],
) -> Result<Vec<KeyImageSpentStatus>, String> {
    if key_images.is_empty() {
        return Ok(Vec::new());
    }

    let rpc = HttpRpc::new(node_url.to_string())
        .map_err(|e| format!("Failed to create HTTP RPC: {:?}", e))?;

    #[derive(Debug, Deserialize)]
    struct IsKeyImageSpentResult {
        spent_status: Vec<u32>,
    }

    let params = json!({
        "key_images": key_images
    });

    let result: IsKeyImageSpentResult = rpc
        .json_rpc_call("is_key_image_spent", Some(params))
        .await
        .map_err(|e| format!("RPC error: {:?}", e))?;

    if result.spent_status.len() != key_images.len() {
        return Err(format!(
            "Response length mismatch: expected {}, got {}",
            key_images.len(),
            result.spent_status.len()
        ));
    }

    let results = key_images
        .iter()
        .zip(result.spent_status.iter())
        .map(|(key_image, &status)| KeyImageSpentStatus {
            key_image: key_image.clone(),
            spent: status > 0,
        })
        .collect();

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_key_images_empty() {
        let key_images: Vec<String> = vec![];
        let batches = batch_key_images(&key_images, 100);
        assert_eq!(batches.len(), 0);
    }

    #[test]
    fn test_batch_key_images_single_batch() {
        let key_images: Vec<String> = (0..50).map(|i| format!("key_{}", i)).collect();
        let batches = batch_key_images(&key_images, 100);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 50);
    }

    #[test]
    fn test_batch_key_images_multiple_batches() {
        let key_images: Vec<String> = (0..250).map(|i| format!("key_{}", i)).collect();
        let batches = batch_key_images(&key_images, 100);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 100);
        assert_eq!(batches[1].len(), 100);
        assert_eq!(batches[2].len(), 50);
    }

    #[test]
    fn test_batch_key_images_exact_multiple() {
        let key_images: Vec<String> = (0..200).map(|i| format!("key_{}", i)).collect();
        let batches = batch_key_images(&key_images, 100);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 100);
        assert_eq!(batches[1].len(), 100);
    }
}
