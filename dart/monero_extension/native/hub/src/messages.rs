use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletState {
    pub address: String,
    pub current_height: u64,
    pub daemon_height: u64,
    pub confirmed_balance: u64,
    pub unconfirmed_balance: u64,
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
    pub destination: String,
    pub amount: u64,
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
