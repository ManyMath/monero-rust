//! Monero WASM Wallet Library

pub mod abstractions;
pub mod coin_selection;
pub mod encryption;
pub mod scanner;
pub mod rpc_serai;
pub mod tx_proof;
pub mod wallet_output;
pub mod wallet_state;

pub mod tx_builder;
pub use tx_builder::native;

pub use coin_selection::{select_inputs, find_best_combination, CoinSelectionResult};
pub use encryption::{encrypt, decrypt, EncryptionError};
pub use wallet_output::WalletOutput;
pub use wallet_state::{WalletState, Balance};

pub use scanner::{
    BlockScanResult, DerivedKeys, Lookahead, MempoolScanResult, OwnedOutputInfo,
    DEFAULT_LOOKAHEAD, derive_address, derive_keys, derive_subaddress, generate_seed, seed_birthday, validate_seed,
    get_daemon_height, scan_block_for_outputs_with_url, scan_block_for_outputs_with_url_and_lookahead,
    scan_mempool_for_outputs, scan_mempool_for_outputs_with_lookahead, scan_mempool_for_outputs_with_account_lookahead,
    // Batch scanning exports
    scan_blocks_batch_with_url,
    scan_blocks_batch_multi_wallet_with_url,
    process_batch_response,
    process_batch_multi_wallet_response,
    // Multi-wallet scanning exports
    MultiWalletScanResult, WalletScanConfig, WalletScanData,
    scan_block_multi_wallet_with_url,
};

#[cfg(not(target_arch = "wasm32"))]
pub use scanner::scan_block_multi_wallet;

#[cfg(target_arch = "wasm32")]
pub use scanner::scan_block_multi_wallet_wasm;

/// Simple integration test function
pub fn test_integration() -> String {
    "monero-rust works".to_string()
}

#[cfg(target_arch = "wasm32")]
pub mod wasm_impl;

#[cfg(target_arch = "wasm32")]
pub mod rpc_adapter;

#[cfg(not(target_arch = "wasm32"))]
pub mod native_impl;
pub use abstractions::{
    AbError, AbResult, BlockData, BlockHeader, BlockResponse, GetOutsParams, HeightResponse,
    OutEntry, OutsResponse, OutputIndex, RpcClient, TimeProvider, TransactionData,
    TxSubmitResponse, WalletStorage,
};
#[cfg(target_arch = "wasm32")]
pub use wasm_impl::{BrowserStorage, CallbackRpcClient, JsTimeProvider, WasmRpcClient};

#[cfg(not(target_arch = "wasm32"))]
pub use native_impl::SystemTimeProvider;

pub use abstractions::MemoryStorage;
