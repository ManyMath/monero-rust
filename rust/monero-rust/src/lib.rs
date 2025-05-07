//! Monero Wallet Core Library
//!
//! A portable, batteries-included wallet library for Monero.
//! Works on both native and WASM targets.

// Core modules
pub mod abstractions;
pub mod coin_selection;
pub mod encryption;
pub mod scan_coordinator;
pub mod scanner;
pub mod wallet_output;
pub mod wallet_state;

// Transaction building
pub mod tx_builder;
pub mod tx_prepare;
pub mod tx_proof;
pub use tx_builder::native;
pub use tx_prepare::{prepare_send_inputs, prepare_sweep_inputs, PreparedInputs};

// RPC
pub mod rpc_serai;

// -- Wallet state & output types --
pub use wallet_output::WalletOutput;
pub use wallet_state::{WalletState, Balance, is_spendable};

// -- Coin selection --
pub use coin_selection::{select_inputs, find_best_combination, CoinSelectionResult};

// -- Scan coordination --
pub use scan_coordinator::{
    compute_lookahead, filter_outputs_by_accounts, process_single_wallet_batch,
    sync_progress, BlockOutputSummary, ProcessedBatch, SyncProgress,
};

// -- Encryption --
pub use encryption::{encrypt, decrypt, EncryptionError};

// -- Scanning --
pub use scanner::{
    // Types
    BlockScanResult, DerivedKeys, Lookahead, MempoolScanResult,
    MultiWalletScanResult, WalletScanConfig, WalletScanData,
    // Constants
    DEFAULT_LOOKAHEAD,
    // Key derivation
    derive_address, derive_keys, derive_subaddress, generate_seed, seed_birthday, validate_seed,
    // Single-block scanning
    get_daemon_height,
    scan_block_for_outputs_with_url, scan_block_for_outputs_with_url_and_lookahead,
    scan_block_multi_wallet_with_url,
    // Mempool scanning
    scan_mempool_for_outputs, scan_mempool_for_outputs_with_lookahead,
    scan_mempool_for_outputs_with_account_lookahead,
    // Batch scanning
    scan_blocks_batch_with_url, scan_blocks_batch_multi_wallet_with_url,
    process_batch_response, process_batch_multi_wallet_response,
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
    MemoryStorage, OutEntry, OutsResponse, OutputIndex, RpcClient, TimeProvider,
    TransactionData, TxSubmitResponse, WalletStorage,
};

// -- Platform-specific implementations --
#[cfg(target_arch = "wasm32")]
pub mod wasm_impl;
#[cfg(target_arch = "wasm32")]
pub mod rpc_adapter;
#[cfg(target_arch = "wasm32")]
pub use wasm_impl::{BrowserStorage, CallbackRpcClient, JsTimeProvider, WasmRpcClient};

#[cfg(not(target_arch = "wasm32"))]
pub mod native_impl;
#[cfg(not(target_arch = "wasm32"))]
pub use native_impl::SystemTimeProvider;
