use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, DartSignal)]
pub struct MoneroTestRequest {}

#[derive(Serialize, RustSignal)]
pub struct MoneroTestResponse {
    pub result: String,
}

#[derive(Deserialize, DartSignal)]
pub struct CreateWalletRequest {
    pub password: String,
    pub network: String,
}

#[derive(Serialize, RustSignal)]
pub struct WalletCreatedResponse {
    pub address: String,
}

#[derive(Deserialize, DartSignal)]
pub struct StartSyncRequest {}

#[derive(Serialize, RustSignal)]
pub struct SyncProgressResponse {
    pub current_height: u64,
    pub daemon_height: u64,
    pub is_synced: bool,
    pub is_scanning: bool,
}

#[derive(Deserialize, DartSignal)]
pub struct GetBalanceRequest {}

#[derive(Serialize, RustSignal)]
pub struct BalanceResponse {
    pub confirmed: u64,
    pub unconfirmed: u64,
}

#[derive(Deserialize, DartSignal)]
pub struct CreateTransactionRequest {
    pub node_url: String,
    pub seed: String,
    pub network: String,
    pub destination: String,
    pub amount: u64,
}

#[derive(Serialize, RustSignal)]
pub struct TransactionCreatedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub tx_id: String,
    pub fee: u64,
    pub tx_blob: Option<String>,
    pub spent_output_hashes: Vec<String>,
}

#[derive(Deserialize, DartSignal)]
pub struct GenerateSeedRequest {}

#[derive(Serialize, RustSignal)]
pub struct SeedGeneratedResponse {
    pub seed: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize, DartSignal)]
pub struct DeriveAddressRequest {
    pub seed: String,
    pub network: String,
}

#[derive(Serialize, RustSignal)]
pub struct AddressDerivedResponse {
    pub address: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize, DartSignal)]
pub struct DeriveKeysRequest {
    pub seed: String,
    pub network: String,
}

#[derive(Serialize, RustSignal)]
pub struct KeysDerivedResponse {
    pub address: String,
    pub secret_spend_key: String,
    pub secret_view_key: String,
    pub public_spend_key: String,
    pub public_view_key: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize, DartSignal)]
pub struct ScanBlockRequest {
    pub node_url: String,
    pub block_height: u64,
    pub seed: String,
    pub network: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, SignalPiece)]
pub struct OwnedOutput {
    pub tx_hash: String,
    pub output_index: u8,
    pub amount: u64,
    pub amount_xmr: String,
    pub key: String,
    pub key_offset: String,
    pub commitment_mask: String,
    pub subaddress_index: Option<(u32, u32)>,
    pub payment_id: Option<String>,
    pub received_output_bytes: String,
    pub block_height: u64,
    pub spent: bool,
    pub key_image: String, // Hex-encoded key image for spent detection
}

#[derive(Serialize, Deserialize, Debug, Clone, RustSignal)]
pub struct BlockScanResponse {
    pub success: bool,
    pub error: Option<String>,
    pub block_height: u64,
    pub block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: u32,
    pub outputs: Vec<OwnedOutput>,
    pub daemon_height: u64,
}

#[derive(Deserialize, DartSignal)]
pub struct BroadcastTransactionRequest {
    pub node_url: String,
    pub tx_blob: String,
    pub spent_output_hashes: Vec<String>,
}

#[derive(Serialize, RustSignal)]
pub struct TransactionBroadcastResponse {
    pub success: bool,
    pub error: Option<String>,
    pub tx_id: Option<String>,
}

#[derive(Deserialize, DartSignal)]
pub struct QueryDaemonHeightRequest {
    pub node_url: String,
}

#[derive(Serialize, RustSignal)]
pub struct DaemonHeightResponse {
    pub success: bool,
    pub error: Option<String>,
    pub daemon_height: u64,
}

#[derive(Deserialize, DartSignal)]
pub struct StartContinuousScanRequest {
    pub node_url: String,
    pub start_height: u64,
    pub seed: String,
    pub network: String,
}

#[derive(Deserialize, DartSignal)]
pub struct StopScanRequest {}
