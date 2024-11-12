use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct MockBlock {
    pub height: u64,
    pub hash: String,
    pub timestamp: u64,
    pub tx_count: usize,
    pub has_output: bool,
}

pub struct MockRpc {
    pub daemon_height: u64,
    pub blocks: HashMap<u64, MockBlock>,
    pub should_fail: bool,
    pub fail_at_height: Option<u64>,
}

impl MockRpc {
    pub fn new(daemon_height: u64) -> Self {
        let mut blocks = HashMap::new();

        // Populate with mock blocks
        for height in 0..=daemon_height {
            blocks.insert(height, MockBlock {
                height,
                hash: format!("blockhash_{}", height),
                timestamp: 1600000000 + (height * 120), // 2 min per block
                tx_count: if height % 10 == 0 { 5 } else { 2 },
                has_output: height % 15 == 0, // Output every 15 blocks
            });
        }

        Self {
            daemon_height,
            blocks,
            should_fail: false,
            fail_at_height: None,
        }
    }

    pub fn with_failure(mut self, fail_at_height: u64) -> Self {
        self.should_fail = true;
        self.fail_at_height = Some(fail_at_height);
        self
    }

    pub fn get_height(&self) -> Result<u64, String> {
        if self.should_fail {
            return Err("Mock RPC: Failed to get height".to_string());
        }
        Ok(self.daemon_height)
    }

    pub fn get_block(&self, height: u64) -> Result<MockBlock, String> {
        if let Some(fail_height) = self.fail_at_height {
            if height == fail_height {
                return Err(format!("Mock RPC: Failed to get block at height {}", height));
            }
        }

        self.blocks
            .get(&height)
            .cloned()
            .ok_or_else(|| format!("Block not found at height {}", height))
    }

    pub fn scan_block(&self, height: u64, _seed: &str) -> Result<MockScanResult, String> {
        let block = self.get_block(height)?;

        let outputs = if block.has_output {
            vec![MockOutput {
                tx_hash: format!("tx_{}", height),
                output_index: 0,
                amount: 1000000000000, // 1 XMR
                block_height: height,
            }]
        } else {
            vec![]
        };

        Ok(MockScanResult {
            block_height: height,
            block_hash: block.hash,
            block_timestamp: block.timestamp,
            tx_count: block.tx_count,
            outputs,
            daemon_height: self.daemon_height,
        })
    }
}

#[derive(Clone, Debug)]
pub struct MockOutput {
    pub tx_hash: String,
    pub output_index: u64,
    pub amount: u64,
    pub block_height: u64,
}

#[derive(Clone, Debug)]
pub struct MockScanResult {
    pub block_height: u64,
    pub block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: usize,
    pub outputs: Vec<MockOutput>,
    pub daemon_height: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_rpc_creation() {
        let rpc = MockRpc::new(1000);
        assert_eq!(rpc.daemon_height, 1000);
        assert_eq!(rpc.blocks.len(), 1001); // 0 to 1000 inclusive
    }

    #[test]
    fn test_mock_rpc_get_height() {
        let rpc = MockRpc::new(500);
        assert_eq!(rpc.get_height().unwrap(), 500);
    }

    #[test]
    fn test_mock_rpc_get_block() {
        let rpc = MockRpc::new(100);
        let block = rpc.get_block(50).unwrap();
        assert_eq!(block.height, 50);
        assert_eq!(block.hash, "blockhash_50");
    }

    #[test]
    fn test_mock_scan_with_outputs() {
        let rpc = MockRpc::new(100);

        // Block 0, 15, 30, 45, etc. have outputs
        let result = rpc.scan_block(15, "test_seed").unwrap();
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.outputs[0].amount, 1000000000000);
    }

    #[test]
    fn test_mock_scan_without_outputs() {
        let rpc = MockRpc::new(100);

        // Block 1 should have no outputs
        let result = rpc.scan_block(1, "test_seed").unwrap();
        assert_eq!(result.outputs.len(), 0);
    }

    #[test]
    fn test_mock_rpc_failure() {
        let rpc = MockRpc::new(100).with_failure(50);

        // Should succeed before failure height
        assert!(rpc.scan_block(49, "test_seed").is_ok());

        // Should fail at failure height
        assert!(rpc.scan_block(50, "test_seed").is_err());

        // Should succeed after failure height
        assert!(rpc.scan_block(51, "test_seed").is_ok());
    }

    #[test]
    fn test_continuous_scan_simulation() {
        let rpc = MockRpc::new(100);
        let start_height = 10;
        let end_height = 20;

        let mut total_outputs = 0;
        for height in start_height..=end_height {
            let result = rpc.scan_block(height, "test_seed").unwrap();
            total_outputs += result.outputs.len();
        }

        // In range 10-20, only height 15 is divisible by 15
        assert_eq!(total_outputs, 1); // Only height 15 has an output
    }
}
