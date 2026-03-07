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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    #[serde(default)]
    pub spent_height: Option<u64>,
    pub key_image: String,
    pub is_coinbase: bool,
    #[serde(default)]
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
            spent_height: o.spent_height,
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
            spent_height: o.spent_height,
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
            spent_height: o.spent_height,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    #[serde(default)]
    pub do_not_relay: bool,
}

#[derive(Serialize)]
pub struct TransactionBroadcastResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    #[serde(default)]
    pub allow_insecure_http: bool,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub block_hashes_json: Option<String>,
}

#[derive(Deserialize)]
pub struct GetPendingStateRequest {}

#[derive(Serialize)]
pub struct PendingStateResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
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

// --- Offline signing signals ---

#[derive(Deserialize)]
pub struct CreateUnsignedTransactionRequest {
    pub node_url: String,
    pub view_key_hex: String,
    pub pub_spend_key_hex: String,
    pub network: String,
    pub recipients: Vec<Recipient>,
    pub selected_outputs: Option<Vec<String>>,
    #[serde(default)]
    pub max_fee_per_weight: Option<u64>,
}

#[derive(Deserialize)]
pub struct SignUnsignedTransactionRequest {
    pub seed: String,
    pub unsigned_tx_hex: String,
    pub network: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
    #[serde(default)]
    pub spend_secret_key_hex: Option<String>,
    #[serde(default)]
    pub view_secret_key_hex: Option<String>,
}

#[derive(Deserialize)]
pub struct ExtractSignedTxSetRequest {
    pub data_hex: String,
    pub view_key_hex: String,
}

#[derive(Deserialize)]
pub struct InspectUnsignedTxSetRequest {
    pub data_hex: String,
    pub view_key_hex: String,
}

#[derive(Deserialize)]
pub struct BuildSignedTxSetRequest {
    pub unsigned_txset_hex: String,
    pub view_key_hex: String,
    pub tx_blob_hex: String,
    pub key_images: Vec<String>,
    pub tx_key_images: Vec<SignedTxSetKeyImageEntry>,
}

#[derive(Deserialize)]
pub struct ExportKeyImagesRequest {
    pub seed: String,
    pub network: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Deserialize)]
pub struct ImportKeyImagesRequest {
    pub data_hex: String,
    pub node_url: String,
    pub seed: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub bip39_account_index: u32,
}

#[derive(Serialize)]
pub struct UnsignedTransactionCreatedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub unsigned_tx_hex: Option<String>,
    pub fee: u64,
    pub recipients: Vec<Recipient>,
}

#[derive(Serialize)]
pub struct TransactionSignedOfflineResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub tx_id: Option<String>,
    pub fee: u64,
    pub tx_blob: Option<String>,
    pub signed_txset_hex: Option<String>,
    pub tx_key: Option<String>,
    pub tx_key_additional: Vec<String>,
    pub change_outputs: Vec<ChangeOutput>,
    pub spent_key_images: Vec<String>,
}

#[derive(Serialize)]
pub struct ExtractedSignedTransaction {
    pub tx_id: String,
    pub tx_blob: String,
    pub tx_version: u64,
    pub tx_unlock_time: u64,
    pub tx_input_count: u64,
    pub tx_input_ring_sizes: Vec<u64>,
    pub tx_output_count: u64,
    pub tx_extra_len: usize,
    pub rct_type: Option<u8>,
    pub rct_fee: Option<u64>,
    pub dust: u64,
    pub fee: u64,
    pub dust_added_to_fee: bool,
    pub change_amount: u64,
    pub selected_transfer_count: u64,
    pub selected_transfer_indices: Vec<u64>,
    pub key_images_len: usize,
    pub key_images_blob_hex: String,
    pub tx_key_is_zero: bool,
    pub tx_key: Option<String>,
    pub additional_tx_key_count: u64,
    pub tx_key_additional: Vec<String>,
    pub destination_count: u64,
    pub destination_total_amount: u64,
    pub multisig_sig_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTxSetKeyImageEntry {
    pub public_key: String,
    pub key_image: String,
}

#[derive(Serialize)]
pub struct SignedTxSetExtractedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub transactions: Vec<ExtractedSignedTransaction>,
    pub key_images: Vec<String>,
    pub tx_key_images: Vec<SignedTxSetKeyImageEntry>,
}

#[derive(Serialize)]
pub struct UnsignedTxSetSourceRingEntry {
    pub global_output_index: u64,
    pub output_public_key: String,
    pub commitment: String,
}

#[derive(Serialize)]
pub struct UnsignedTxSetSourceSummary {
    pub ring_size: u64,
    pub ring: Vec<UnsignedTxSetSourceRingEntry>,
    pub real_output: u64,
    pub real_global_output_index: u64,
    pub real_output_public_key: String,
    pub real_tx_public_key: String,
    pub real_out_additional_tx_keys: Vec<String>,
    pub real_output_in_tx_index: u64,
    pub amount: u64,
    pub rct: bool,
    pub mask: String,
}

#[derive(Serialize)]
pub struct UnsignedTxSetDestinationSummary {
    pub original_address_hex: String,
    pub amount: u64,
    pub spend_public_key: String,
    pub view_public_key: String,
    pub is_subaddress: bool,
    pub is_integrated: bool,
}

#[derive(Serialize)]
pub struct UnsignedTxSetConstructionSummary {
    pub source_count: u64,
    pub source_ring_sizes: Vec<u64>,
    pub sources: Vec<UnsignedTxSetSourceSummary>,
    pub change_amount: u64,
    pub change: UnsignedTxSetDestinationSummary,
    pub split_destination_count: u64,
    pub split_destination_total_amount: u64,
    pub split_destinations: Vec<UnsignedTxSetDestinationSummary>,
    pub selected_transfer_indices: Vec<u64>,
    pub extra_hex: String,
    pub unlock_time: u64,
    pub construction_flags: u8,
    pub use_rct: bool,
    pub use_view_tags: bool,
    pub rct_range_proof_type: u64,
    pub rct_bp_version: u64,
    pub destination_count: u64,
    pub destination_total_amount: u64,
    pub destinations: Vec<UnsignedTxSetDestinationSummary>,
    pub subaddr_account: u32,
    pub subaddr_indices: Vec<u32>,
}

#[derive(Serialize)]
pub struct UnsignedTxSetTransferSummary {
    pub output_public_key: String,
    pub internal_output_index: u64,
    pub global_output_index: u64,
    pub tx_public_key: String,
    pub flags_raw: u8,
    pub spent: bool,
    pub frozen: bool,
    pub rct: bool,
    pub key_image_known: bool,
    pub key_image_request: bool,
    pub key_image_partial: bool,
    pub amount: u64,
    pub additional_tx_keys: Vec<String>,
    pub subaddress_major: u64,
    pub subaddress_minor: u64,
}

#[derive(Serialize)]
pub struct UnsignedTxSetInspectedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub archive_version: u64,
    pub transaction_count: u64,
    pub new_transfer_first: u64,
    pub new_transfer_second: u64,
    pub new_transfer_count: u64,
    pub new_transfers: Vec<UnsignedTxSetTransferSummary>,
    pub constructions: Vec<UnsignedTxSetConstructionSummary>,
}

#[derive(Serialize)]
pub struct SignedTxSetBuiltResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub signed_txset_hex: Option<String>,
}

#[derive(Serialize)]
pub struct KeyImagesExportedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub key_images_hex: Option<String>,
    pub count: u64,
}

#[derive(Serialize)]
pub struct KeyImagesImportedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub imported_count: u64,
    pub spent_count: u64,
    pub key_images: Vec<String>,
    pub spent_key_images: Vec<String>,
}

// --- .keys file import signals ---

#[derive(Deserialize)]
pub struct ImportKeysFileRequest {
    pub file_bytes_hex: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct ImportKeysFileResponse {
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<u32>,
    pub error_hint: Option<String>,
    pub error_transient: Option<bool>,
    pub spend_secret_key: Option<String>,
    pub view_secret_key: Option<String>,
    pub spend_public_key: Option<String>,
    pub view_public_key: Option<String>,
    pub creation_timestamp: u64,
    pub watch_only: bool,
    pub network: Option<String>,
    pub seed_language: Option<String>,
    pub mnemonic: Option<String>,
}

#[derive(Deserialize)]
pub struct ExportKeysFileRequest {
    pub seed: String,
    pub network: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct ExportKeysFileResponse {
    pub success: bool,
    pub error: Option<String>,
    pub file_bytes_hex: Option<String>,
}

#[derive(Deserialize)]
pub struct StartUrEncoderRequest {
    pub data_hex: String,
    pub ur_type: String,
    pub max_fragment_len: u32,
}

#[derive(Deserialize)]
pub struct StopUrEncoderRequest {}

#[derive(Serialize)]
pub struct QrFrameResponse {
    pub modules: Vec<bool>,
    pub size: u32,
    pub seq_num: u32,
    pub seq_len: u32,
    pub uri: String,
}

#[derive(Deserialize)]
pub struct UrDecodeFrameRequest {
    pub uri: String,
}

#[derive(Deserialize)]
pub struct ResetUrDecoderRequest {}

#[derive(Serialize)]
pub struct UrDecodeProgressResponse {
    pub progress: f64,
    pub is_complete: bool,
}

#[derive(Serialize)]
pub struct UrDecodeCompleteResponse {
    pub data_hex: String,
    pub ur_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(value: &T) -> serde_json::Value
    where
        T: Serialize,
    {
        let json_str = serde_json::to_string(value).expect("serialize");
        serde_json::from_str(&json_str).expect("deserialize")
    }

    #[test]
    fn owned_output_round_trip_no_optionals() {
        let output = OwnedOutput {
            tx_hash: "aabb".repeat(16),
            output_index: 0,
            amount: 10_000_000_000_000,
            amount_xmr: "10.000000000000".into(),
            key: "key_hex".into(),
            key_offset: "offset_hex".into(),
            commitment_mask: "mask_hex".into(),
            subaddress_index: None,
            payment_id: None,
            received_output_bytes: "received_bytes".into(),
            block_height: 1_384_526,
            spent: false,
            spent_height: None,
            key_image: "ki_hex".into(),
            is_coinbase: false,
            frozen: false,
        };

        let json_str = serde_json::to_string(&output).expect("serialize");
        let restored: OwnedOutput = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.tx_hash, output.tx_hash);
        assert_eq!(restored.output_index, 0);
        assert_eq!(restored.amount, 10_000_000_000_000);
        assert_eq!(restored.subaddress_index, None);
        assert_eq!(restored.payment_id, None);
        assert_eq!(restored.block_height, 1_384_526);
        assert!(!restored.spent);
        assert_eq!(restored.spent_height, None);
        assert!(!restored.is_coinbase);
        assert!(!restored.frozen);
    }

    #[test]
    fn owned_output_round_trip_with_subaddress_and_payment_id() {
        let output = OwnedOutput {
            tx_hash: "tx_with_sub".into(),
            output_index: 1,
            amount: 500,
            amount_xmr: "0.000000000500".into(),
            key: "k".into(),
            key_offset: "o".into(),
            commitment_mask: "m".into(),
            subaddress_index: Some((2, 5)),
            payment_id: Some("deadbeef12345678".into()),
            received_output_bytes: "b".into(),
            block_height: 999,
            spent: true,
            spent_height: Some(1_000),
            key_image: "ki".into(),
            is_coinbase: true,
            frozen: true,
        };

        let json_str = serde_json::to_string(&output).expect("serialize");
        let restored: OwnedOutput = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.subaddress_index, Some((2, 5)));
        assert_eq!(restored.payment_id.as_deref(), Some("deadbeef12345678"));
        assert!(restored.spent);
        assert!(restored.is_coinbase);
        assert!(restored.frozen);
    }

    #[test]
    fn change_output_round_trip() {
        let change = ChangeOutput {
            tx_hash: "change_tx".into(),
            output_index: 1,
            amount: 999_956_000_000,
            amount_xmr: "0.999956000000".into(),
            key: "ck".into(),
            key_offset: "co".into(),
            commitment_mask: "cm".into(),
            subaddress_index: Some((0, 1)),
            received_output_bytes: "cb".into(),
            key_image: "cki".into(),
        };

        let json_str = serde_json::to_string(&change).expect("serialize");
        let restored: ChangeOutput = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.tx_hash, "change_tx");
        assert_eq!(restored.subaddress_index, Some((0, 1)));
        assert_eq!(restored.amount, 999_956_000_000);
    }

    #[test]
    fn change_output_no_subaddress() {
        let change = ChangeOutput {
            tx_hash: "c2".into(),
            output_index: 0,
            amount: 100,
            amount_xmr: "0.000000000100".into(),
            key: "k".into(),
            key_offset: "o".into(),
            commitment_mask: "m".into(),
            subaddress_index: None,
            received_output_bytes: "b".into(),
            key_image: "ki".into(),
        };

        let json_str = serde_json::to_string(&change).expect("serialize");
        let restored: ChangeOutput = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.subaddress_index, None);
    }

    #[test]
    fn block_scan_response_round_trip() {
        let output = OwnedOutput {
            tx_hash: "scan_tx".into(),
            output_index: 0,
            amount: 7_777,
            amount_xmr: "0.000000007777".into(),
            key: "k".into(),
            key_offset: "o".into(),
            commitment_mask: "m".into(),
            subaddress_index: Some((0, 0)),
            payment_id: None,
            received_output_bytes: "b".into(),
            block_height: 1_384_526,
            spent: false,
            spent_height: None,
            key_image: "ki".into(),
            is_coinbase: false,
            frozen: false,
        };

        let response = BlockScanResponse {
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
            block_height: 1_384_526,
            block_hash: "a5918cf3".into(),
            block_timestamp: 1_688_074_142,
            tx_count: 2,
            outputs: vec![output],
            daemon_height: 2_037_532,
            spent_key_images: vec!["spent_ki_1".into(), "spent_ki_2".into()],
            spent_key_image_tx_hashes: vec!["spent_tx_1".into()],
        };

        let json_str = serde_json::to_string(&response).expect("serialize");
        let restored: BlockScanResponse = serde_json::from_str(&json_str).expect("deserialize");

        assert!(restored.success);
        assert_eq!(restored.error, None);
        assert_eq!(restored.block_height, 1_384_526);
        assert_eq!(restored.tx_count, 2);
        assert_eq!(restored.outputs.len(), 1);
        assert_eq!(restored.outputs[0].tx_hash, "scan_tx");
        assert_eq!(restored.outputs[0].subaddress_index, Some((0, 0)));
        assert_eq!(restored.spent_key_images.len(), 2);
        assert_eq!(restored.daemon_height, 2_037_532);
    }

    #[test]
    fn block_scan_response_empty_outputs() {
        let response = BlockScanResponse {
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
            block_height: 100,
            block_hash: "hash".into(),
            block_timestamp: 0,
            tx_count: 0,
            outputs: vec![],
            daemon_height: 200,
            spent_key_images: vec![],
            spent_key_image_tx_hashes: vec![],
        };

        let json_str = serde_json::to_string(&response).expect("serialize");
        let restored: BlockScanResponse = serde_json::from_str(&json_str).expect("deserialize");

        assert!(restored.outputs.is_empty());
        assert!(restored.spent_key_images.is_empty());
    }

    #[test]
    fn block_scan_response_with_error() {
        let response = BlockScanResponse {
            success: false,
            error: Some("Connection refused".into()),
            error_code: None,
            error_hint: None,
            error_transient: None,
            block_height: 0,
            block_hash: String::new(),
            block_timestamp: 0,
            tx_count: 0,
            outputs: vec![],
            daemon_height: 0,
            spent_key_images: vec![],
            spent_key_image_tx_hashes: vec![],
        };

        let v = round_trip(&response);
        assert_eq!(v["success"], false);
        assert_eq!(v["error"], "Connection refused");
    }

    #[test]
    fn transaction_created_response_round_trip() {
        let change = ChangeOutput {
            tx_hash: "change_hash".into(),
            output_index: 1,
            amount: 800_000,
            amount_xmr: "0.000000800000".into(),
            key: "ck".into(),
            key_offset: "co".into(),
            commitment_mask: "cm".into(),
            subaddress_index: Some((0, 0)),
            received_output_bytes: "cb".into(),
            key_image: "cki".into(),
        };

        let response = TransactionCreatedResponse {
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
            tx_id: "txid_hex".into(),
            fee: 44_000_000,
            tx_blob: Some("blob_hex".into()),
            tx_key: Some("txkey_hex".into()),
            tx_key_additional: vec!["extra1".into()],
            spent_output_hashes: vec!["spent1:0".into(), "spent2:1".into()],
            change_outputs: vec![change],
        };

        let v = round_trip(&response);
        assert_eq!(v["success"], true);
        assert_eq!(v["tx_id"], "txid_hex");
        assert_eq!(v["fee"], 44_000_000);
        assert_eq!(v["tx_blob"], "blob_hex");
        assert_eq!(v["tx_key"], "txkey_hex");
        assert_eq!(v["tx_key_additional"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(
            v["spent_output_hashes"].as_array().map(|a| a.len()),
            Some(2)
        );
        assert_eq!(v["change_outputs"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(v["change_outputs"][0]["subaddress_index"][0], 0);
        assert_eq!(v["change_outputs"][0]["subaddress_index"][1], 0);
    }

    #[test]
    fn multi_wallet_scan_response_round_trip() {
        let output = OwnedOutput {
            tx_hash: "multi_tx".into(),
            output_index: 0,
            amount: 500,
            amount_xmr: "0.000000000500".into(),
            key: "k".into(),
            key_offset: "o".into(),
            commitment_mask: "m".into(),
            subaddress_index: None,
            payment_id: None,
            received_output_bytes: "b".into(),
            block_height: 5_000,
            spent: false,
            spent_height: None,
            key_image: "ki".into(),
            is_coinbase: false,
            frozen: false,
        };

        let response = MultiWalletScanResponse {
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
            block_height: 5_000,
            block_hash: "blockhash".into(),
            block_timestamp: 1_700_000_000,
            tx_count: 3,
            daemon_height: 5_100,
            spent_key_images: vec!["ki1".into()],
            spent_key_image_tx_hashes: vec!["txh1".into()],
            wallet_results: vec![
                WalletScanResult {
                    address: "5wallet1addr".into(),
                    outputs: vec![],
                },
                WalletScanResult {
                    address: "5wallet2addr".into(),
                    outputs: vec![output],
                },
            ],
        };

        let v = round_trip(&response);
        assert_eq!(v["wallet_results"].as_array().map(|a| a.len()), Some(2));
        assert_eq!(
            v["wallet_results"][0]["outputs"]
                .as_array()
                .map(|a| a.len()),
            Some(0)
        );
        assert_eq!(
            v["wallet_results"][1]["outputs"]
                .as_array()
                .map(|a| a.len()),
            Some(1)
        );
        assert_eq!(v["wallet_results"][1]["address"], "5wallet2addr");
    }

    #[test]
    fn recipient_round_trip() {
        let r = Recipient {
            address: "5addr1".into(),
            amount: 1_000_000_000_000,
        };

        let json_str = serde_json::to_string(&r).expect("serialize");
        let restored: Recipient = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.address, "5addr1");
        assert_eq!(restored.amount, 1_000_000_000_000);
    }

    #[test]
    fn wallet_scan_result_round_trip() {
        let result = WalletScanResult {
            address: "5scanaddr".into(),
            outputs: vec![],
        };

        let json_str = serde_json::to_string(&result).expect("serialize");
        let restored: WalletScanResult = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.address, "5scanaddr");
        assert!(restored.outputs.is_empty());
    }

    #[test]
    fn double_spend_conflict_round_trip() {
        let conflict = DoubleSpendConflict {
            key_image: "ki_conflict".into(),
            previous_spent_height: 1000,
            new_height: 1005,
        };

        let json_str = serde_json::to_string(&conflict).expect("serialize");
        let restored: DoubleSpendConflict = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.key_image, "ki_conflict");
        assert_eq!(restored.previous_spent_height, 1000);
        assert_eq!(restored.new_height, 1005);
    }

    #[test]
    fn sync_progress_response_serialize() {
        let response = SyncProgressResponse {
            current_height: 1500,
            daemon_height: 2000,
            is_synced: false,
            is_scanning: true,
        };

        let v = round_trip(&response);
        assert_eq!(v["current_height"], 1500);
        assert_eq!(v["daemon_height"], 2000);
        assert_eq!(v["is_synced"], false);
        assert_eq!(v["is_scanning"], true);
    }

    #[test]
    fn keys_derived_response_serialize() {
        let response = KeysDerivedResponse {
            address: "5addr".into(),
            secret_spend_key: "ssk".into(),
            secret_view_key: "svk".into(),
            public_spend_key: "psk".into(),
            public_view_key: "pvk".into(),
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
        };

        let v = round_trip(&response);
        assert_eq!(v["address"], "5addr");
        assert_eq!(v["secret_spend_key"], "ssk");
        assert_eq!(v["error"], serde_json::Value::Null);
    }

    #[test]
    fn transaction_broadcast_response_serialize() {
        let response = TransactionBroadcastResponse {
            success: false,
            error: Some("Double spend".into()),
            error_code: None,
            error_hint: None,
            error_transient: None,
            tx_id: None,
            is_retryable: false,
            is_double_spend: true,
        };

        let v = round_trip(&response);
        assert_eq!(v["success"], false);
        assert_eq!(v["error"], "Double spend");
        assert_eq!(v["tx_id"], serde_json::Value::Null);
        assert_eq!(v["is_double_spend"], true);
        assert_eq!(v["is_retryable"], false);
    }

    #[test]
    fn reorg_detected_response_serialize() {
        let response = ReorgDetectedResponse {
            split_height: 1000,
            blocks_detached: 5,
            outputs_removed: 2,
            outputs_unspent: 1,
            removed_key_images: vec!["rki1".into(), "rki2".into()],
            unspent_key_images: vec!["uki1".into()],
        };

        let v = round_trip(&response);
        assert_eq!(v["split_height"], 1000);
        assert_eq!(v["blocks_detached"], 5);
        assert_eq!(v["removed_key_images"].as_array().map(|a| a.len()), Some(2));
        assert_eq!(v["unspent_key_images"].as_array().map(|a| a.len()), Some(1));
    }

    #[test]
    fn double_spend_detected_response_serialize() {
        let response = DoubleSpendDetectedResponse {
            conflicts: vec![
                DoubleSpendConflict {
                    key_image: "ki_a".into(),
                    previous_spent_height: 100,
                    new_height: 105,
                },
                DoubleSpendConflict {
                    key_image: "ki_b".into(),
                    previous_spent_height: 0,
                    new_height: 0,
                },
            ],
        };

        let v = round_trip(&response);
        let conflicts = v["conflicts"].as_array().expect("conflicts array");
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0]["key_image"], "ki_a");
        assert_eq!(conflicts[1]["previous_spent_height"], 0);
    }

    #[test]
    fn unsigned_transaction_created_response_serialize() {
        let response = UnsignedTransactionCreatedResponse {
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
            unsigned_tx_hex: Some("aabbccdd".into()),
            fee: 50_000_000,
            recipients: vec![Recipient {
                address: "5dest1".into(),
                amount: 1_000_000_000_000,
            }],
        };

        let v = round_trip(&response);
        assert_eq!(v["unsigned_tx_hex"], "aabbccdd");
        assert_eq!(v["fee"], 50_000_000);
        assert_eq!(v["recipients"][0]["address"], "5dest1");
    }

    #[test]
    fn transaction_signed_offline_response_serialize() {
        let change = ChangeOutput {
            tx_hash: "signed_change".into(),
            output_index: 1,
            amount: 800_000,
            amount_xmr: "0.000000800000".into(),
            key: "ck".into(),
            key_offset: "co".into(),
            commitment_mask: "cm".into(),
            subaddress_index: None,
            received_output_bytes: "cb".into(),
            key_image: "cki".into(),
        };

        let response = TransactionSignedOfflineResponse {
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
            tx_id: Some("signed_txid".into()),
            fee: 44_000_000,
            tx_blob: Some("signed_blob".into()),
            signed_txset_hex: Some("signed_txset".into()),
            tx_key: Some("signed_key".into()),
            tx_key_additional: vec![],
            change_outputs: vec![change],
            spent_key_images: vec!["spent_ki".into()],
        };

        let v = round_trip(&response);
        assert_eq!(v["tx_id"], "signed_txid");
        assert_eq!(v["signed_txset_hex"], "signed_txset");
        assert_eq!(
            v["change_outputs"][0]["subaddress_index"],
            serde_json::Value::Null
        );
        assert_eq!(v["spent_key_images"][0], "spent_ki");
    }

    #[test]
    fn signed_txset_extracted_response_serialize() {
        let response = SignedTxSetExtractedResponse {
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
            transactions: vec![ExtractedSignedTransaction {
                tx_id: "signed_txid".into(),
                tx_blob: "signed_blob".into(),
                tx_version: 2,
                tx_unlock_time: 0,
                tx_input_count: 2,
                tx_input_ring_sizes: vec![16, 16],
                tx_output_count: 2,
                tx_extra_len: 44,
                rct_type: Some(6),
                rct_fee: Some(123),
                dust: 0,
                fee: 123,
                dust_added_to_fee: false,
                change_amount: 456,
                selected_transfer_count: 2,
                selected_transfer_indices: vec![0, 20],
                key_images_len: 64,
                key_images_blob_hex: "abcd".into(),
                tx_key_is_zero: false,
                tx_key: Some("txkey".into()),
                additional_tx_key_count: 1,
                tx_key_additional: vec!["txkey2".into()],
                destination_count: 1,
                destination_total_amount: 789,
                multisig_sig_count: 0,
            }],
            key_images: vec!["ki1".into(), "ki2".into()],
            tx_key_images: vec![SignedTxSetKeyImageEntry {
                public_key: "pubkey".into(),
                key_image: "ki1".into(),
            }],
        };

        let v = round_trip(&response);
        assert_eq!(v["success"], true);
        assert_eq!(v["transactions"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(v["transactions"][0]["tx_id"], "signed_txid");
        assert_eq!(v["transactions"][0]["tx_blob"], "signed_blob");
        assert_eq!(v["transactions"][0]["tx_version"], 2);
        assert_eq!(v["transactions"][0]["tx_unlock_time"], 0);
        assert_eq!(v["transactions"][0]["tx_input_count"], 2);
        assert_eq!(
            v["transactions"][0]["tx_input_ring_sizes"],
            serde_json::json!([16, 16])
        );
        assert_eq!(v["transactions"][0]["tx_output_count"], 2);
        assert_eq!(v["transactions"][0]["tx_extra_len"], 44);
        assert_eq!(v["transactions"][0]["rct_type"], 6);
        assert_eq!(v["transactions"][0]["rct_fee"], 123);
        assert_eq!(v["transactions"][0]["dust"], 0);
        assert_eq!(v["transactions"][0]["fee"], 123);
        assert_eq!(v["transactions"][0]["dust_added_to_fee"], false);
        assert_eq!(v["transactions"][0]["change_amount"], 456);
        assert_eq!(v["transactions"][0]["selected_transfer_count"], 2);
        assert_eq!(
            v["transactions"][0]["selected_transfer_indices"],
            serde_json::json!([0, 20])
        );
        assert_eq!(v["transactions"][0]["key_images_len"], 64);
        assert_eq!(v["transactions"][0]["key_images_blob_hex"], "abcd");
        assert_eq!(v["transactions"][0]["tx_key_is_zero"], false);
        assert_eq!(v["transactions"][0]["tx_key"], "txkey");
        assert_eq!(v["transactions"][0]["additional_tx_key_count"], 1);
        assert_eq!(v["transactions"][0]["tx_key_additional"][0], "txkey2");
        assert_eq!(v["transactions"][0]["destination_count"], 1);
        assert_eq!(v["transactions"][0]["destination_total_amount"], 789);
        assert_eq!(v["transactions"][0]["multisig_sig_count"], 0);
        assert_eq!(v["key_images"].as_array().map(|a| a.len()), Some(2));
        assert_eq!(v["tx_key_images"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(v["tx_key_images"][0]["public_key"], "pubkey");
        assert_eq!(v["tx_key_images"][0]["key_image"], "ki1");
    }

    #[test]
    fn unsigned_txset_inspected_response_serialize() {
        let response = UnsignedTxSetInspectedResponse {
            success: true,
            error: None,
            error_code: None,
            error_hint: None,
            error_transient: None,
            archive_version: 2,
            transaction_count: 1,
            new_transfer_first: 80,
            new_transfer_second: 81,
            new_transfer_count: 1,
            new_transfers: vec![UnsignedTxSetTransferSummary {
                output_public_key: "transfer_opk".into(),
                internal_output_index: 2,
                global_output_index: 1234,
                tx_public_key: "transfer_tpk".into(),
                flags_raw: 0x0c,
                spent: false,
                frozen: false,
                rct: true,
                key_image_known: true,
                key_image_request: false,
                key_image_partial: false,
                amount: 42,
                additional_tx_keys: vec!["transfer_extra_tpk".into()],
                subaddress_major: 0,
                subaddress_minor: 1,
            }],
            constructions: vec![UnsignedTxSetConstructionSummary {
                source_count: 2,
                source_ring_sizes: vec![16, 16],
                sources: vec![UnsignedTxSetSourceSummary {
                    ring_size: 16,
                    ring: vec![
                        UnsignedTxSetSourceRingEntry {
                            global_output_index: 122,
                            output_public_key: "ring_opk_0".into(),
                            commitment: "ring_commitment_0".into(),
                        },
                        UnsignedTxSetSourceRingEntry {
                            global_output_index: 123,
                            output_public_key: "opk".into(),
                            commitment: "ring_commitment_1".into(),
                        },
                    ],
                    real_output: 4,
                    real_global_output_index: 123,
                    real_output_public_key: "opk".into(),
                    real_tx_public_key: "tpk".into(),
                    real_out_additional_tx_keys: vec!["extra_tpk".into()],
                    real_output_in_tx_index: 1,
                    amount: 0,
                    rct: true,
                    mask: "mask".into(),
                }],
                change_amount: 10,
                change: UnsignedTxSetDestinationSummary {
                    original_address_hex: String::new(),
                    amount: 10,
                    spend_public_key: "change_spend".into(),
                    view_public_key: "change_view".into(),
                    is_subaddress: false,
                    is_integrated: false,
                },
                split_destination_count: 2,
                split_destination_total_amount: 50,
                split_destinations: vec![
                    UnsignedTxSetDestinationSummary {
                        original_address_hex: String::new(),
                        amount: 10,
                        spend_public_key: "change_spend".into(),
                        view_public_key: "change_view".into(),
                        is_subaddress: false,
                        is_integrated: false,
                    },
                    UnsignedTxSetDestinationSummary {
                        original_address_hex: "0011".into(),
                        amount: 40,
                        spend_public_key: "spend".into(),
                        view_public_key: "view".into(),
                        is_subaddress: false,
                        is_integrated: false,
                    },
                ],
                selected_transfer_indices: vec![0, 20],
                extra_hex: "abcd".into(),
                unlock_time: 0,
                construction_flags: 3,
                use_rct: true,
                use_view_tags: true,
                rct_range_proof_type: 3,
                rct_bp_version: 4,
                destination_count: 1,
                destination_total_amount: 40,
                destinations: vec![UnsignedTxSetDestinationSummary {
                    original_address_hex: "0011".into(),
                    amount: 40,
                    spend_public_key: "spend".into(),
                    view_public_key: "view".into(),
                    is_subaddress: false,
                    is_integrated: false,
                }],
                subaddr_account: 0,
                subaddr_indices: vec![0],
            }],
        };

        let v = round_trip(&response);
        assert_eq!(v["success"], true);
        assert_eq!(v["archive_version"], 2);
        assert_eq!(v["transaction_count"], 1);
        assert_eq!(v["new_transfer_first"], 80);
        assert_eq!(v["new_transfer_second"], 81);
        assert_eq!(v["new_transfer_count"], 1);
        assert_eq!(v["new_transfers"][0]["flags_raw"], 12);
        assert_eq!(
            v["new_transfers"][0]["additional_tx_keys"][0],
            "transfer_extra_tpk"
        );
        assert_eq!(v["constructions"][0]["source_ring_sizes"][0], 16);
        assert_eq!(
            v["constructions"][0]["sources"][0]["ring"]
                .as_array()
                .map(|a| a.len()),
            Some(2)
        );
        assert_eq!(
            v["constructions"][0]["sources"][0]["ring"][1]["commitment"],
            "ring_commitment_1"
        );
        assert_eq!(
            v["constructions"][0]["sources"][0]["real_out_additional_tx_keys"][0],
            "extra_tpk"
        );
        assert_eq!(v["constructions"][0]["sources"][0]["mask"], "mask");
        assert_eq!(v["constructions"][0]["change"]["amount"], 10);
        assert_eq!(v["constructions"][0]["construction_flags"], 3);
        assert_eq!(
            v["constructions"][0]["split_destinations"]
                .as_array()
                .map(|a| a.len()),
            Some(2)
        );
        assert_eq!(v["constructions"][0]["selected_transfer_indices"][1], 20);
        assert_eq!(v["constructions"][0]["destinations"][0]["amount"], 40);
    }

    #[test]
    fn create_transaction_request_deserialize() {
        let json = r#"{
            "node_url": "http://localhost:38081",
            "seed": "test seed words",
            "network": "stagenet",
            "recipients": [{"address": "5addr", "amount": 1000000000000}],
            "selected_outputs": ["txhash1:0"],
            "passphrase": "mypass",
            "bip39_account_index": 2,
            "subtract_fee": true
        }"#;

        let req: CreateTransactionRequest = serde_json::from_str(json).expect("deserialize");

        assert_eq!(req.node_url, "http://localhost:38081");
        assert_eq!(req.recipients.len(), 1);
        assert_eq!(req.recipients[0].amount, 1_000_000_000_000);
        assert_eq!(req.selected_outputs, Some(vec!["txhash1:0".into()]));
        assert_eq!(req.passphrase, "mypass");
        assert_eq!(req.bip39_account_index, 2);
        assert!(req.subtract_fee);
    }

    #[test]
    fn create_transaction_request_defaults() {
        let json = r#"{
            "node_url": "http://node:38081",
            "seed": "seed",
            "network": "mainnet",
            "recipients": [{"address": "a", "amount": 1}]
        }"#;

        let req: CreateTransactionRequest = serde_json::from_str(json).expect("deserialize");

        assert_eq!(req.selected_outputs, None);
        assert_eq!(req.passphrase, "");
        assert_eq!(req.bip39_account_index, 0);
        assert!(!req.subtract_fee);
    }

    #[test]
    fn create_unsigned_transaction_request_fee_override_deserialize() {
        let json = r#"{
            "node_url": "http://localhost:18081",
            "view_key_hex": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "pub_spend_key_hex": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "network": "mainnet",
            "recipients": [{"address": "4dest", "amount": 1000000000000}],
            "selected_outputs": ["txhash1:0"],
            "max_fee_per_weight": 2000000
        }"#;

        let req: CreateUnsignedTransactionRequest =
            serde_json::from_str(json).expect("deserialize");

        assert_eq!(req.node_url, "http://localhost:18081");
        assert_eq!(req.recipients.len(), 1);
        assert_eq!(req.selected_outputs, Some(vec!["txhash1:0".into()]));
        assert_eq!(req.max_fee_per_weight, Some(2_000_000));
    }

    #[test]
    fn start_continuous_scan_request_deserialize() {
        let json = r#"{
            "node_url": "http://localhost:38081",
            "start_height": 1000,
            "seed": "seed words",
            "network": "stagenet",
            "account_lookahead": 5,
            "subaddress_lookahead": 10,
            "accounts_to_scan": [0, 1, 2]
        }"#;

        let req: StartContinuousScanRequest = serde_json::from_str(json).expect("deserialize");

        assert_eq!(req.start_height, 1000);
        assert_eq!(req.account_lookahead, 5);
        assert_eq!(req.subaddress_lookahead, 10);
        assert_eq!(req.accounts_to_scan, Some(vec![0, 1, 2]));
    }

    #[test]
    fn restore_wallet_data_request_deserialize() {
        let json = r#"{
            "seed": "test seed",
            "network": "stagenet",
            "outputs": [{
                "tx_hash": "tx1",
                "output_index": 0,
                "amount": 100,
                "amount_xmr": "0.000000000100",
                "key": "k",
                "key_offset": "o",
                "commitment_mask": "m",
                "subaddress_index": [1, 3],
                "received_output_bytes": "b",
                "block_height": 50,
                "spent": true,
                "spent_height": 45,
                "key_image": "ki",
                "is_coinbase": false,
                "frozen": false
            }],
            "daemon_height": 2000,
            "current_height": 1500,
            "block_hashes_json": "{\"hashes\":[\"abc\"]}",
            "pending_state_json": "{\"pending\":[]}"
        }"#;

        let req: RestoreWalletDataRequest = serde_json::from_str(json).expect("deserialize");

        assert_eq!(req.outputs.len(), 1);
        assert_eq!(req.outputs[0].subaddress_index, Some((1, 3)));
        assert_eq!(req.outputs[0].spent_height, Some(45));
        assert_eq!(req.daemon_height, 2000);
        assert_eq!(req.current_height, 1500);
        assert!(req.block_hashes_json.is_some());
        assert!(req.pending_state_json.is_some());
    }

    #[test]
    fn owned_output_zero_values() {
        let output = OwnedOutput {
            tx_hash: String::new(),
            output_index: 0,
            amount: 0,
            amount_xmr: "0.000000000000".into(),
            key: String::new(),
            key_offset: String::new(),
            commitment_mask: String::new(),
            subaddress_index: None,
            payment_id: None,
            received_output_bytes: String::new(),
            block_height: 0,
            spent: false,
            spent_height: None,
            key_image: String::new(),
            is_coinbase: false,
            frozen: false,
        };

        let json_str = serde_json::to_string(&output).expect("serialize");
        let restored: OwnedOutput = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.amount, 0);
        assert_eq!(restored.block_height, 0);
        assert_eq!(restored.tx_hash, "");
    }

    #[test]
    fn owned_output_large_u64_amount() {
        let output = OwnedOutput {
            tx_hash: "large".into(),
            output_index: 255, // u8 max
            amount: u64::MAX,
            amount_xmr: "18446744073.709551615".into(),
            key: "k".into(),
            key_offset: "o".into(),
            commitment_mask: "m".into(),
            subaddress_index: Some((u32::MAX, u32::MAX)),
            payment_id: None,
            received_output_bytes: "b".into(),
            block_height: u64::MAX,
            spent: false,
            spent_height: Some(u64::MAX),
            key_image: "ki".into(),
            is_coinbase: false,
            frozen: false,
        };

        let json_str = serde_json::to_string(&output).expect("serialize");
        let restored: OwnedOutput = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(restored.amount, u64::MAX);
        assert_eq!(restored.output_index, 255);
        assert_eq!(restored.block_height, u64::MAX);
        assert_eq!(restored.spent_height, Some(u64::MAX));
        assert_eq!(restored.subaddress_index, Some((u32::MAX, u32::MAX)));
    }
}
