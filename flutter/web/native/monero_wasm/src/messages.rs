use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletState {
    pub address: String,
    pub current_height: u64,
    pub daemon_height: u64,
    pub confirmed_balance: u64,
    pub unconfirmed_balance: u64,
    pub seed: Option<String>,
    pub network: Option<String>,
    pub outputs: Vec<monero_rust::WalletOutput>,
}

#[derive(Debug, Clone)]
pub struct BuildTransaction {
    pub node_url: String,
    pub seed: String,
    pub network: String,
    pub recipients: Vec<(String, u64)>, // (address, amount) pairs
    pub selected_outputs: Option<Vec<String>>, // "txHash:outputIndex" keys for coin control
    pub subtract_fee: bool,
}

#[derive(Debug, Clone)]
pub struct SweepAll {
    pub node_url: String,
    pub seed: String,
    pub network: String,
    pub destination_address: String,
    pub selected_outputs: Option<Vec<String>>, // "txHash:outputIndex" keys to sweep (for account filtering)
}

#[derive(Debug, Clone)]
pub struct GetWalletData;

#[derive(Debug, Clone)]
pub struct WalletData {
    #[allow(dead_code)]
    pub seed: Option<String>,
    #[allow(dead_code)]
    pub network: Option<String>,
    pub outputs: Vec<monero_rust::WalletOutput>,
    pub pending_key_images: HashSet<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UpdateBalance {
    pub confirmed: u64,
    pub unconfirmed: u64,
}

#[derive(Debug, Clone)]
pub struct StoreOutputs {
    pub seed: String,
    pub network: String,
    pub outputs: Vec<monero_rust::WalletOutput>,
    pub daemon_height: u64,
    pub block_hashes: Vec<(u64, String)>,
}

#[derive(Debug, Clone)]
pub struct MarkOutputsSpent {
    pub output_keys: Vec<String>, // "txHash:outputIndex" format
}

#[derive(Debug, Clone)]
pub struct GetWalletHeight;

#[derive(Debug, Clone)]
pub struct WalletHeight {
    #[allow(dead_code)]
    pub current_height: u64,
    pub daemon_height: u64,
}

#[derive(Debug, Clone)]
pub struct BroadcastTransaction {
    pub node_url: String,
    pub tx_blob: String,
    pub spent_output_hashes: Vec<String>,
    pub tx_id: String,
    pub spent_key_images: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StartContinuousScan {
    pub node_url: String,
    pub start_height: u64,
    pub seed: String,
    pub passphrase: String,
    pub network: String,
    pub account_lookahead: u32,
    pub subaddress_lookahead: u32,
    pub accounts_to_scan: Option<Vec<u32>>,
}

#[derive(Debug, Clone)]
pub struct StopScan;

#[derive(Debug, Clone)]
pub struct ContinueScan;

#[derive(Debug, Clone)]
pub struct ContinueMultiWalletScan;

#[derive(Debug, Clone)]
pub struct UpdateScanState {
    pub is_scanning: bool,
    pub current_height: u64,
    pub target_height: u64,
    pub node_url: String,
    pub seed: String,
    pub passphrase: String,
    pub network: String,
    pub account_lookahead: u32,
    pub subaddress_lookahead: u32,
    pub accounts_to_scan: Option<Vec<u32>>,
}

#[derive(Debug, Clone)]
pub struct UpdateMultiWalletScanState {
    pub is_scanning: bool,
    pub current_height: u64,
    pub target_height: u64,
    pub node_url: String,
    pub wallets: Vec<crate::signals::WalletConfig>,
}

#[derive(Debug, Clone)]
pub struct UpdateSpentStatus {
    pub key_images: Vec<String>,
    pub tx_hashes: Vec<String>,
    pub height: u64,
}

#[derive(Debug, Clone)]
pub struct HandleReorg {
    pub batch_results: Vec<monero_rust::BlockScanResult>,
    pub accounts_to_scan: Option<Vec<u32>>,
    pub target_height: u64,
    pub batch_start_height: u64,
    pub node_url: String,
    pub seed: String,
    pub passphrase: String,
    pub network: String,
    pub account_lookahead: u32,
    pub subaddress_lookahead: u32,
}

#[derive(Debug, Clone)]
pub struct RecordBlockHashes {
    pub block_hashes: Vec<(u64, String)>,
    pub daemon_height: u64,
}

#[derive(Debug, Clone)]
pub struct SetDaemonHeight {
    pub height: u64,
}

#[derive(Debug, Clone)]
pub struct CheckMempoolConflicts {
    pub key_images: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PendingSpendInfo {
    pub key_image: String,
    pub output_key: String,
    pub amount: u64,
}

#[derive(Debug, Clone)]
pub struct AddPendingSpends {
    pub tx_id: String,
    pub spends: Vec<PendingSpendInfo>,
}

#[derive(Debug, Clone)]
pub struct TrackTransaction {
    pub tx_id: String,
    pub spent_key_images: Vec<String>,
    pub spent_output_keys: Vec<String>,
    pub change_outputs: Vec<monero_rust::ChangeOutputRef>,
    pub fee: u64,
}

#[derive(Debug, Clone)]
pub struct AddMempoolPendingSpends {
    pub key_images: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateUnsignedTx {
    pub node_url: String,
    pub view_key_hex: String,
    pub pub_spend_key_hex: String,
    pub network: String,
    pub recipients: Vec<(String, u64)>,
    pub selected_outputs: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct SignUnsignedTx {
    pub seed: String,
    pub unsigned_tx_hex: String,
    pub network: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExportKeyImages {
    pub seed: String,
    pub network: String,
}

#[derive(Debug, Clone)]
pub struct ImportKeyImages {
    pub data_hex: String,
    pub node_url: String,
}

