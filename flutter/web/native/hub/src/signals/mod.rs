use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipient {
    pub address: String,
    pub amount: u64,
}

#[derive(Deserialize)]
pub struct MoneroTestRequest {}

#[derive(Serialize)]
pub struct MoneroTestResponse {
    pub result: String,
}

#[derive(Deserialize)]
pub struct CreateWalletRequest {
    #[allow(dead_code)]
    pub password: String,
    pub network: String,
}

#[derive(Serialize)]
pub struct WalletCreatedResponse {
    pub address: String,
}

#[derive(Deserialize)]
pub struct StartSyncRequest {}

#[derive(Serialize)]
pub struct SyncProgressResponse {
    pub current_height: u64,
    pub daemon_height: u64,
    pub is_synced: bool,
    pub is_scanning: bool,
}

#[derive(Deserialize)]
pub struct GetBalanceRequest {}

#[derive(Serialize)]
pub struct BalanceResponse {
    pub confirmed: u64,
    pub unconfirmed: u64,
    pub pending_spend: u64,
}

#[derive(Deserialize)]
pub struct CreateTransactionRequest {
    pub node_url: String,
    pub seed: String,
    pub network: String,
    pub recipients: Vec<Recipient>,
    pub selected_outputs: Option<Vec<String>>, // "txHash:outputIndex" keys for coin control
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
    #[serde(default)]
    pub subtract_fee: bool,
}

#[derive(Deserialize)]
pub struct SweepAllRequest {
    pub node_url: String,
    pub seed: String,
    pub network: String,
    pub destination_address: String,
    pub selected_outputs: Option<Vec<String>>, // "txHash:outputIndex" keys to sweep (for account filtering)
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChangeOutput {
    pub tx_hash: String,
    pub output_index: u8,
    pub amount: u64,
    pub amount_xmr: String,
    pub key: String,
    pub key_offset: String,
    pub commitment_mask: String,
    pub subaddress_index: Option<(u32, u32)>,
    pub received_output_bytes: String,
    pub key_image: String,
}

#[derive(Serialize)]
pub struct TransactionCreatedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub tx_id: String,
    pub fee: u64,
    pub tx_blob: Option<String>,
    /// The private transaction key (r scalar), hex-encoded.
    /// Required to prove payments to recipients.
    pub tx_key: Option<String>,
    /// Additional private keys for subaddress outputs, hex-encoded.
    pub tx_key_additional: Vec<String>,
    pub spent_output_hashes: Vec<String>,
    pub change_outputs: Vec<ChangeOutput>,
}

#[derive(Deserialize)]
pub struct GenerateSeedRequest {
    pub seed_type: String,
}

#[derive(Serialize)]
pub struct SeedGeneratedResponse {
    pub seed: String,
    pub success: bool,
    pub error: Option<String>,
    pub restore_height: Option<u64>,
}

#[derive(Deserialize)]
pub struct GetSeedBirthdayRequest {
    pub seed: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Serialize)]
pub struct SeedBirthdayResponse {
    pub birthday: Option<u64>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct GetBlockHeightFromTimestampRequest {
    pub timestamp: u64,
    pub node_url: String,
}

#[derive(Serialize)]
pub struct BlockHeightFromTimestampResponse {
    pub block_height: u64,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct DeriveAddressRequest {
    pub seed: String,
    pub network: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Serialize)]
pub struct AddressDerivedResponse {
    pub address: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct DeriveSubaddressRequest {
    pub seed: String,
    pub network: String,
    pub account: u32,
    pub address_index: u32,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Serialize)]
pub struct SubaddressDerivedResponse {
    pub address: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct DeriveKeysRequest {
    pub seed: String,
    pub network: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Serialize)]
pub struct KeysDerivedResponse {
    pub address: String,
    pub secret_spend_key: String,
    pub secret_view_key: String,
    pub public_spend_key: String,
    pub public_view_key: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct ScanBlockRequest {
    pub node_url: String,
    pub block_height: u64,
    pub seed: String,
    pub network: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
    pub key_image: String,
    pub is_coinbase: bool,
    pub frozen: bool,
}

impl From<monero_rust::WalletOutput> for OwnedOutput {
    fn from(o: monero_rust::WalletOutput) -> Self {
        OwnedOutput {
            tx_hash: o.tx_hash,
            output_index: o.output_index,
            amount: o.amount,
            amount_xmr: o.amount_xmr,
            key: o.key,
            key_offset: o.key_offset,
            commitment_mask: o.commitment_mask,
            subaddress_index: o.subaddress_index,
            payment_id: o.payment_id,
            received_output_bytes: o.received_output_bytes,
            block_height: o.block_height,
            spent: o.spent,
            key_image: o.key_image,
            is_coinbase: o.is_coinbase,
            frozen: o.frozen,
        }
    }
}

impl From<&monero_rust::WalletOutput> for OwnedOutput {
    fn from(o: &monero_rust::WalletOutput) -> Self {
        OwnedOutput {
            tx_hash: o.tx_hash.clone(),
            output_index: o.output_index,
            amount: o.amount,
            amount_xmr: o.amount_xmr.clone(),
            key: o.key.clone(),
            key_offset: o.key_offset.clone(),
            commitment_mask: o.commitment_mask.clone(),
            subaddress_index: o.subaddress_index,
            payment_id: o.payment_id.clone(),
            received_output_bytes: o.received_output_bytes.clone(),
            block_height: o.block_height,
            spent: o.spent,
            key_image: o.key_image.clone(),
            is_coinbase: o.is_coinbase,
            frozen: o.frozen,
        }
    }
}

impl From<OwnedOutput> for monero_rust::WalletOutput {
    fn from(o: OwnedOutput) -> Self {
        monero_rust::WalletOutput {
            tx_hash: o.tx_hash,
            output_index: o.output_index,
            amount: o.amount,
            amount_xmr: o.amount_xmr,
            key: o.key,
            key_offset: o.key_offset,
            commitment_mask: o.commitment_mask,
            subaddress_index: o.subaddress_index,
            payment_id: o.payment_id,
            received_output_bytes: o.received_output_bytes,
            block_height: o.block_height,
            spent: o.spent,
            spent_height: None,
            key_image: o.key_image,
            is_coinbase: o.is_coinbase,
            frozen: o.frozen,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockScanResponse {
    pub success: bool,
    pub error: Option<String>,
    pub block_height: u64,
    pub block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: u32,
    pub outputs: Vec<OwnedOutput>,
    pub daemon_height: u64,
    pub spent_key_images: Vec<String>,
    pub spent_key_image_tx_hashes: Vec<String>,
}

#[derive(Deserialize)]
pub struct BroadcastTransactionRequest {
    pub node_url: String,
    pub tx_blob: String,
    pub spent_output_hashes: Vec<String>,
    #[serde(default)]
    pub tx_id: String,
    #[serde(default)]
    pub spent_key_images: Vec<String>,
}

#[derive(Serialize)]
pub struct TransactionBroadcastResponse {
    pub success: bool,
    pub error: Option<String>,
    pub tx_id: Option<String>,
    #[serde(default)]
    pub is_retryable: bool,
    #[serde(default)]
    pub is_double_spend: bool,
}

#[derive(Deserialize)]
pub struct QueryDaemonHeightRequest {
    pub node_url: String,
}

#[derive(Serialize)]
pub struct DaemonHeightResponse {
    pub success: bool,
    pub error: Option<String>,
    pub daemon_height: u64,
}

#[derive(Deserialize)]
pub struct StartContinuousScanRequest {
    pub node_url: String,
    pub start_height: u64,
    pub seed: String,
    pub network: String,
    pub account_lookahead: u32,
    #[serde(default)]
    pub subaddress_lookahead: u32,
    #[serde(default)]
    pub accounts_to_scan: Option<Vec<u32>>,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Deserialize)]
pub struct StopScanRequest {}

#[derive(Serialize)]
pub struct SpentStatusUpdatedResponse {
    pub spent_key_images: Vec<String>,
}

#[derive(Deserialize)]
pub struct MempoolScanRequest {
    pub node_url: String,
    pub seed: String,
    pub network: String,
    pub account_lookahead: u32,
    #[serde(default)]
    pub subaddress_lookahead: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub accounts_to_scan: Option<Vec<u32>>,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Serialize)]
pub struct MempoolScanResponse {
    pub success: bool,
    pub error: Option<String>,
    pub tx_count: u32,
    pub outputs: Vec<OwnedOutput>,
    pub spent_key_images: Vec<String>,
    pub spent_key_image_tx_hashes: Vec<String>,
}

#[derive(Deserialize)]
pub struct GenerateOutProofRequest {
    pub tx_id: String,
    pub tx_key: String,
    pub recipient_address: String,
    pub message: String,
    pub network: String,
}

#[derive(Serialize)]
pub struct OutProofGeneratedResponse {
    pub success: bool,
    pub error: Option<String>,
    /// The OutProofV2 signature string (e.g., "OutProofV2...")
    pub signature: Option<String>,
    /// Feather-style formatted proof with headers
    pub formatted: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveWalletDataRequest {
    pub password: String,
    pub wallet_data_json: String,
}

#[derive(Serialize)]
pub struct WalletDataSavedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub encrypted_data: Option<String>,
}

#[derive(Deserialize)]
pub struct LoadWalletDataRequest {
    pub password: String,
    pub encrypted_data: String,
}

#[derive(Serialize)]
pub struct WalletDataLoadedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub wallet_data_json: Option<String>,
}

// --- Derived-key encryption signals (for auto-save without caching raw password) ---

#[derive(Deserialize)]
pub struct DeriveEncryptionKeyRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct EncryptionKeyDerivedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub key_hex: Option<String>,
    pub salt_hex: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveWithDerivedKeyRequest {
    pub key_hex: String,
    pub salt_hex: String,
    pub wallet_data_json: String,
}

// Multi-wallet scanning signals

#[derive(Deserialize, Debug, Clone)]
pub struct WalletConfig {
    pub seed: String,
    pub network: String,
    pub account_lookahead: u32, // Keep for backwards compatibility
    #[serde(default)]
    pub subaddress_lookahead: u32,
    #[serde(default)]
    pub accounts_to_scan: Option<Vec<u32>>,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Deserialize)]
pub struct ScanBlockMultiWalletRequest {
    pub node_url: String,
    pub block_height: u64,
    pub wallets: Vec<WalletConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WalletScanResult {
    pub address: String,
    pub outputs: Vec<OwnedOutput>,
}

#[derive(Serialize)]
pub struct MultiWalletScanResponse {
    pub success: bool,
    pub error: Option<String>,
    pub block_height: u64,
    pub block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: u32,
    pub daemon_height: u64,
    pub spent_key_images: Vec<String>,
    pub spent_key_image_tx_hashes: Vec<String>,
    pub wallet_results: Vec<WalletScanResult>,
}

#[derive(Deserialize)]
pub struct StartMultiWalletScanRequest {
    pub node_url: String,
    pub start_height: u64,
    pub wallets: Vec<WalletConfig>,
}

#[derive(Deserialize)]
pub struct RestoreWalletDataRequest {
    pub seed: String,
    pub network: String,
    pub outputs: Vec<OwnedOutput>,
    pub daemon_height: u64,
    pub current_height: u64,
    #[serde(default)]
    pub block_hashes_json: Option<String>,
    #[serde(default)]
    pub pending_state_json: Option<String>,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Deserialize)]
pub struct GetBlockHashesRequest {}

#[derive(Serialize)]
pub struct BlockHashesResponse {
    pub success: bool,
    pub error: Option<String>,
    pub block_hashes_json: Option<String>,
}

#[derive(Deserialize)]
pub struct GetPendingStateRequest {}

#[derive(Serialize)]
pub struct PendingStateResponse {
    pub success: bool,
    pub error: Option<String>,
    pub pending_state_json: Option<String>,
}

#[derive(Serialize)]
pub struct ReorgDetectedResponse {
    pub split_height: u64,
    pub blocks_detached: u64,
    pub outputs_removed: u64,
    pub outputs_unspent: u64,
    pub removed_key_images: Vec<String>,
    pub unspent_key_images: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DoubleSpendConflict {
    pub key_image: String,
    pub previous_spent_height: u64, // 0 = unknown
    pub new_height: u64,            // 0 = mempool
}

#[derive(Serialize)]
pub struct DoubleSpendDetectedResponse {
    pub conflicts: Vec<DoubleSpendConflict>,
}

#[derive(Deserialize)]
pub struct ConvertBip39ToLegacyRequest {
    pub bip39_mnemonic: String,
    pub account_index: u32,
    #[serde(default)]
    pub passphrase: String,
}

#[derive(Serialize)]
pub struct Bip39LegacySeedResponse {
    pub legacy_seed: String,
    pub success: bool,
    pub error: Option<String>,
}

// --- Freeze/Thaw signals ---

#[derive(Deserialize)]
pub struct FreezeOutputRequest {
    pub key_image: String,
}

#[derive(Deserialize)]
pub struct ThawOutputRequest {
    pub key_image: String,
}

#[derive(Serialize)]
pub struct FreezeThawResponse {
    pub success: bool,
    pub key_image: String,
    pub frozen: bool,
}

#[derive(Serialize)]
pub struct TransactionStatusUpdate {
    pub tx_id: String,
    pub status: String,
    pub confirmed_height: Option<u64>,
}
