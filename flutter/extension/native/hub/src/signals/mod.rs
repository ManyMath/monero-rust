use rinf::{DartSignal, RustSignal};
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
    pub destination: String,
    pub amount: u64,
}

#[derive(Serialize, RustSignal)]
pub struct TransactionCreatedResponse {
    pub tx_id: String,
    pub fee: u64,
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
