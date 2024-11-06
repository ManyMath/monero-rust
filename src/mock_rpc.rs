//! Mock RPC client for deterministic testing.
//!
//! This module provides a mock RPC implementation that replays pre-recorded responses.

use base64::Engine;
use monero_rpc::{Rpc, RpcError};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// A recorded RPC call and its response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedCall {
    pub route: String,
    pub body: String,
    pub response: String,
    /// True if the response is binary data (stored as base64)
    #[serde(default)]
    pub is_binary: bool,
}

/// Mock RPC client that replays recorded responses.
#[derive(Clone)]
pub struct MockRpc {
    recordings: Arc<Vec<RecordedCall>>,
    replay_index: Arc<Mutex<usize>>,
}

impl MockRpc {
    /// Create a new mock RPC from a recording file.
    pub fn from_file(file_path: impl AsRef<Path>) -> Result<Self, String> {
        let content = std::fs::read_to_string(file_path.as_ref())
            .map_err(|e| format!("Failed to read recording file: {}", e))?;

        let recordings: Vec<RecordedCall> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse recording file: {}", e))?;

        Ok(Self {
            recordings: Arc::new(recordings),
            replay_index: Arc::new(Mutex::new(0)),
        })
    }

    /// Create a new mock RPC from recordings.
    pub fn from_recordings(recordings: Vec<RecordedCall>) -> Self {
        Self {
            recordings: Arc::new(recordings),
            replay_index: Arc::new(Mutex::new(0)),
        }
    }

    /// Reset the replay index to start from the beginning.
    pub fn reset(&self) {
        *self.replay_index.lock().unwrap() = 0;
    }

    /// Get the number of recorded calls.
    pub fn recording_count(&self) -> usize {
        self.recordings.len()
    }
}

impl Rpc for MockRpc {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        let mut index = self.replay_index.lock().unwrap();

        if *index >= self.recordings.len() {
            return Err(RpcError::InternalError(format!(
                "Replay exhausted: requested call #{} but only {} calls recorded",
                *index + 1,
                self.recordings.len()
            )));
        }

        let recorded = &self.recordings[*index];
        *index += 1;

        // Verify the route matches (optional, but helpful for debugging)
        if recorded.is_binary {
            // For binary routes, compare base64-encoded bodies
            let body_base64 = base64::engine::general_purpose::STANDARD.encode(&body);
            if recorded.body != body_base64 {
                eprintln!(
                    "WARNING: Binary body mismatch in replay at index {}",
                    *index - 1
                );
            }
        } else {
            let body_str = String::from_utf8_lossy(&body);
            if recorded.route != route {
                eprintln!(
                    "WARNING: Route mismatch in replay at index {} - expected '{}', got '{}'",
                    *index - 1,
                    recorded.route,
                    route
                );
            }
            if recorded.body != body_str {
                eprintln!(
                    "WARNING: Body mismatch in replay at index {}:\nExpected: {}\nGot: {}",
                    *index - 1,
                    recorded.body,
                    body_str
                );
            }
        }

        // Decode response based on whether it's binary
        let response = if recorded.is_binary {
            base64::engine::general_purpose::STANDARD.decode(&recorded.response)
                .map_err(|e| RpcError::InternalError(format!("Failed to decode base64 response: {}", e)))?
        } else {
            recorded.response.as_bytes().to_vec()
        };

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_rpc_basic() {
        let recordings = vec![
            RecordedCall {
                route: "get_height".to_string(),
                body: "".to_string(),
                response: r#"{"height": 12345, "status": "OK"}"#.to_string(),
                is_binary: false,
            },
            RecordedCall {
                route: "json_rpc".to_string(),
                body: r#"{"method":"test"}"#.to_string(),
                response: r#"{"result": "ok"}"#.to_string(),
                is_binary: false,
            },
        ];

        let mock_rpc = MockRpc::from_recordings(recordings);
        assert_eq!(mock_rpc.recording_count(), 2);

        // First call
        let response = mock_rpc.post("get_height", vec![]).await.unwrap();
        assert!(String::from_utf8_lossy(&response).contains("12345"));

        // Second call
        let response = mock_rpc.post("json_rpc", br#"{"method":"test"}"#.to_vec()).await.unwrap();
        assert!(String::from_utf8_lossy(&response).contains("ok"));

        // Third call should fail
        let result = mock_rpc.post("another", vec![]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_rpc_reset() {
        let recordings = vec![
            RecordedCall {
                route: "test".to_string(),
                body: "".to_string(),
                response: "response1".to_string(),
                is_binary: false,
            },
        ];

        let mock_rpc = MockRpc::from_recordings(recordings);

        // Use once
        mock_rpc.post("test", vec![]).await.unwrap();

        // Should fail on second call
        assert!(mock_rpc.post("test", vec![]).await.is_err());

        // Reset and try again
        mock_rpc.reset();
        let result = mock_rpc.post("test", vec![]).await;
        assert!(result.is_ok());
    }
}
