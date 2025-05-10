use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Clone)]
pub struct StartContinuousScan {
    pub node_url: String,
    pub start_height: u64,
    pub seed: String,
    pub network: String,
    pub account_lookahead: u32,
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
    pub network: String,
    pub account_lookahead: u32,
    pub accounts_to_scan: Option<Vec<u32>>, // New: specific accounts to scan
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
}

#[derive(Debug, Clone)]
pub struct SetDaemonHeight {
    pub height: u64,
}

