//! Monero Wallet Core Library
//!
//! A portable, batteries-included wallet library for Monero.
//! Works on both native and WASM targets.

// Core modules
pub mod abstractions;
pub mod bip39_conv;
pub mod chain_config;
pub mod coin_selection;
pub mod encryption;
pub mod error_codes;
pub mod key_image_import;
pub mod monero_backend;
#[cfg(feature = "oxide-adapter-spike")]
pub mod oxide_adapter;
pub mod scan_coordinator;
pub mod scanner;
pub mod wallet_output;
pub mod wallet_state;

// Transaction building
pub mod epee_compat;
pub mod key_image_signing;
pub mod tx_builder;
pub mod tx_prepare;
pub mod tx_proof;
pub mod tx_utils;
pub mod ur_codec;
pub use tx_builder::native;
pub use tx_prepare::{prepare_send_inputs, prepare_sweep_inputs, PreparedInputs};
pub use tx_utils::{adjust_recipients_for_fee, classify_broadcast_error};

// RPC
pub mod rpc_serai;

// -- Wallet state & output types --
pub use wallet_output::WalletOutput;
pub use wallet_state::{
    is_spendable, Balance, BlockHashChain, ChangeOutputRef, PendingSpend, RollbackResult,
    SpentConflict, TrackedTransaction, TxStatus, WalletState, MAX_REORG_DEPTH,
    PENDING_SPEND_TTL_SECS,
};

// -- Coin selection --
pub use coin_selection::{
    estimate_fee, find_best_combination, select_inputs, CoinSelectionResult, DUST_THRESHOLD,
};

// -- Scan coordination --
pub use scan_coordinator::{
    compute_lookahead, filter_outputs_by_accounts, process_batch_with_reorg_detection,
    process_single_wallet_batch, sync_progress, BlockOutputSummary, ProcessedBatch, ReorgInfo,
    ScanBatchOutcome, SyncProgress,
};

// -- BIP39 conversion --
pub use bip39_conv::{bip39_to_legacy_mnemonic, generate_bip39, validate_bip39};

// -- Encryption --
pub use encryption::{decrypt, derive_key_fresh, encrypt, encrypt_with_key, EncryptionError};

// -- Key image import --
pub use key_image_import::{
    extract_key_image_hex, parse_rpc_export, verify_and_extract, verify_key_image_signature,
    KeyImageImportResult, SignedKeyImage,
};

// -- Scanning --
pub use scanner::{
    // Key derivation
    derive_address,
    derive_address_from_view_only,
    derive_keys,
    derive_keys_from_view_only,
    derive_subaddress,
    fetch_blocks_batch_with_history_url,
    fetch_blocks_batch_with_url,
    generate_seed,
    // Single-block scanning
    get_daemon_height,
    is_key_image_spent,
    // View-only wallet
    parse_view_only_keys,
    process_batch_multi_wallet_response,
    process_batch_response,
    process_fetched_batch,
    process_fetched_batch_cached,
    process_fetched_batch_multi_wallet,
    process_fetched_batch_multi_wallet_cached,
    resolve_seed,
    resolve_seed_bip39,
    scan_block_for_outputs_with_url,
    scan_block_for_outputs_with_url_and_lookahead,
    scan_block_multi_wallet_with_url,
    scan_blocks_batch_multi_wallet_with_url,
    // History-aware scanning
    scan_blocks_batch_with_history_url,
    // Batch scanning
    scan_blocks_batch_with_url,
    // Mempool scanning
    scan_mempool_for_outputs,
    scan_mempool_for_outputs_with_account_lookahead,
    scan_mempool_for_outputs_with_lookahead,
    seed_birthday,
    // Fingerprint computation
    spend_key_fingerprint,
    validate_seed,
    // Types
    BlockScanResult,
    CachedScanner,
    CachedScanners,
    DerivedKeys,
    // Double-buffered pipelining
    FetchedBlocks,
    Lookahead,
    MempoolScanResult,
    MultiWalletScanResult,
    WalletScanConfig,
    WalletScanData,
    // Constants
    DEFAULT_LOOKAHEAD,
};

#[cfg(not(target_arch = "wasm32"))]
pub use scanner::scan_block_multi_wallet;

#[cfg(target_arch = "wasm32")]
pub use scanner::scan_block_multi_wallet_wasm;

/// Simple integration test function
pub fn test_integration() -> String {
    "monero-rust works".to_string()
}

// -- Abstractions (RPC, storage, time) --
pub use abstractions::{
    AbError, AbResult, BlockData, BlockHeader, BlockResponse, GetOutsParams, HeightResponse,
    MemoryStorage, OutEntry, OutputIndex, OutsResponse, RpcClient, TimeProvider, TransactionData,
    TxSubmitResponse, WalletStorage,
};

// -- Platform-specific implementations --
#[cfg(target_arch = "wasm32")]
pub mod rpc_adapter;
#[cfg(target_arch = "wasm32")]
pub mod wasm_fetch;
#[cfg(target_arch = "wasm32")]
pub mod wasm_impl;
#[cfg(target_arch = "wasm32")]
pub use wasm_impl::{BrowserStorage, CallbackRpcClient, JsTimeProvider, WasmRpcClient};

#[cfg(not(target_arch = "wasm32"))]
pub mod native_impl;
#[cfg(not(target_arch = "wasm32"))]
pub use native_impl::SystemTimeProvider;

// -- .keys file import --
pub mod wallet_keys_file;
pub use wallet_keys_file::{
    decrypt_keys_data, encrypt_keys_data, parse_decrypted_keys, ImportedKeysFile,
};
#[cfg(not(target_arch = "wasm32"))]
pub use wallet_keys_file::{decrypt_keys_file, read_keys_file, write_keys_file};
