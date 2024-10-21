//! Monero WASM Wallet Library

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub mod abstractions;
#[cfg(target_arch = "wasm32")]
pub mod wasm_impl;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    pub address: String,
    pub balance: u64,
    pub network: String,
}

impl WalletInfo {
    pub fn new(address: String, balance: u64, network: String) -> Self {
        Self {
            address,
            balance,
            network,
        }
    }
}
pub fn generate_demo_address() -> String {
    use getrandom::getrandom;

    let mut bytes = [0u8; 32];
    if getrandom(&mut bytes).is_ok() {
        let address = format!("4{}", hex_encode(&bytes[..20]));
        // Zeroize sensitive data after use
        bytes.zeroize();
        address
    } else {
        "4Demo_Address_Generation_Failed".to_string()
    }
}

pub fn validate_address(address: &str) -> bool {
    address.starts_with('4') && address.len() > 90 && address.len() < 110
}

pub fn calculate_fee(amount: u64, priority: u8) -> u64 {
    let base_fee = amount / 1000;
    match priority {
        0 => base_fee,
        1 => base_fee * 2,
        2 => base_fee * 5,
        _ => base_fee,
    }
}

pub fn format_atomic_to_xmr(atomic: u64) -> String {
    let xmr = atomic as f64 / 1_000_000_000_000.0;
    format!("{:.12} XMR", xmr)
}

pub fn create_demo_wallet(network: &str) -> WalletInfo {
    WalletInfo::new(
        generate_demo_address(),
        5_000_000_000_000,
        network.trim().to_string(),
    )
}
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to generate test addresses with specified prefix and length
    fn generate_test_address(prefix: &str, fill_char: char, total_length: usize) -> String {
        format!("{}{}", prefix, fill_char.to_string().repeat(total_length.saturating_sub(prefix.len())))
    }

    #[test]
    fn test_validate_address() {
        let valid = generate_test_address("4", 'a', 96);
        assert!(validate_address(&valid));

        let invalid = generate_test_address("3", 'a', 96);
        assert!(!validate_address(&invalid));
    }

    #[test]
    fn test_calculate_fee() {
        assert_eq!(calculate_fee(1000, 0), 1);
        assert_eq!(calculate_fee(1000, 1), 2);
        assert_eq!(calculate_fee(1000, 2), 5);
    }

    #[test]
    fn test_format_atomic_to_xmr() {
        let result = format_atomic_to_xmr(1_000_000_000_000);
        assert!(result.contains("1.000000000000 XMR"));
    }

    #[test]
    fn test_create_demo_wallet() {
        let wallet = create_demo_wallet("mainnet");
        assert_eq!(wallet.network, "mainnet");
        assert_eq!(wallet.balance, 5_000_000_000_000);
        assert!(wallet.address.starts_with('4'));
    }
}
