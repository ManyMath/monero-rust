use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOutput {
    pub tx_hash: String,
    pub output_index: u8,
    pub amount: u64,
    pub key: String,
    pub key_offset: String,
    pub commitment_mask: String,
    pub subaddress: Option<(u32, u32)>,
    pub payment_id: Option<String>,
    pub received_output_bytes: String,
    pub block_height: u64,
    pub spent: bool,
    pub key_image: String, // Hex-encoded key image for spent detection
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletState {
    pub address: String,
    pub current_height: u64,
    pub daemon_height: u64,
    pub confirmed_balance: u64,
    pub unconfirmed_balance: u64,
    pub seed: Option<String>,
    pub network: Option<String>,
    pub outputs: Vec<StoredOutput>,
}

#[derive(Debug, Clone)]
pub struct ScanBlock {
    pub height: u64,
}

#[derive(Debug, Clone)]
pub struct BlockScanned {
    pub height: u64,
    pub new_outputs: usize,
}

#[derive(Debug, Clone)]
pub struct QueryHeight;

#[derive(Debug, Clone)]
pub struct HeightResponse {
    pub height: u64,
}

#[derive(Debug, Clone)]
pub struct FetchBlock {
    pub height: u64,
}

#[derive(Debug, Clone)]
pub struct BlockData {
    pub height: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BuildTransaction {
    pub node_url: String,
    pub seed: String,
    pub network: String,
    pub destination: String,
    pub amount: u64,
}

#[derive(Debug, Clone)]
pub struct GetWalletData;

#[derive(Debug, Clone)]
pub struct WalletData {
    pub seed: Option<String>,
    pub network: Option<String>,
    pub outputs: Vec<StoredOutput>,
}

#[derive(Debug, Clone)]
pub struct TransactionBuilt {
    pub tx_id: String,
    pub fee: u64,
}

#[derive(Debug, Clone)]
pub struct UpdateBalance {
    pub confirmed: u64,
    pub unconfirmed: u64,
}

#[derive(Debug, Clone)]
pub struct GetWalletState;

#[derive(Debug, Clone)]
pub struct StoreOutputs {
    pub seed: String,
    pub network: String,
    pub outputs: Vec<StoredOutput>,
    pub daemon_height: u64,
}

#[derive(Debug, Clone)]
pub struct MarkOutputsSpent {
    pub tx_hashes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GetWalletHeight;

#[derive(Debug, Clone)]
pub struct WalletHeight {
    pub current_height: u64,
    pub daemon_height: u64,
}

#[derive(Debug, Clone)]
pub struct BroadcastSuccess {
    pub spent_outputs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BroadcastTransaction {
    pub node_url: String,
    pub tx_blob: String,
    pub spent_output_hashes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StartContinuousScan {
    pub node_url: String,
    pub start_height: u64,
    pub seed: String,
    pub network: String,
}

#[derive(Debug, Clone)]
pub struct StopScan;

#[derive(Debug, Clone)]
pub struct ContinueScan;

#[derive(Debug, Clone)]
pub struct UpdateScanState {
    pub is_scanning: bool,
    pub current_height: u64,
    pub target_height: u64,
    pub node_url: String,
    pub seed: String,
    pub network: String,
}

#[derive(Debug, Clone)]
pub struct UpdateSpentStatus {
    pub key_images: Vec<String>,
}
