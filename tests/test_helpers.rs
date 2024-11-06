//! Test helpers for deterministic testing with mock RPC.

use monero_rust::{mock_rpc::MockRpc, WalletState};
use monero_rpc::Rpc;
use monero_seed::{Language, Seed};
use monero_wallet::address::Network;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Helper to create a wallet with a mock RPC client.
pub struct MockWalletHelper {
    pub wallet: WalletState,
    pub mock_rpc: MockRpc,
    pub temp_dir: TempDir,
}

impl MockWalletHelper {
    /// Create a new wallet with mock RPC from a recording file.
    pub fn from_mnemonic_and_recording(
        mnemonic: &str,
        network: Network,
        recording_path: impl AsRef<Path>,
        refresh_from_height: u64,
    ) -> Result<Self, String> {
        let seed = Seed::from_string(
            Language::English,
            zeroize::Zeroizing::new(mnemonic.to_string()),
        )
        .map_err(|e| format!("Invalid mnemonic: {:?}", e))?;

        let mock_rpc = MockRpc::from_file(recording_path)?;

        let temp_dir = TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;
        let wallet_path = temp_dir.path().join("test_wallet.mw");

        let wallet = WalletState::new(
            seed,
            "English".to_string(),
            network,
            "test_password",
            wallet_path,
            refresh_from_height,
        )
        .map_err(|e| format!("Failed to create wallet: {}", e))?;

        Ok(Self {
            wallet,
            mock_rpc,
            temp_dir,
        })
    }

    /// Create a view-only wallet with mock RPC from a recording file.
    pub fn view_only_from_keys_and_recording(
        spend_public: [u8; 32],
        view_private: [u8; 32],
        network: Network,
        recording_path: impl AsRef<Path>,
        refresh_from_height: u64,
    ) -> Result<Self, String> {
        let mock_rpc = MockRpc::from_file(recording_path)?;

        let temp_dir = TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;
        let wallet_path = temp_dir.path().join("test_wallet.mw");

        let wallet = WalletState::new_view_only(
            spend_public,
            view_private,
            network,
            "test_password",
            wallet_path,
            refresh_from_height,
        )
        .map_err(|e| format!("Failed to create wallet: {}", e))?;

        Ok(Self {
            wallet,
            mock_rpc,
            temp_dir,
        })
    }

    /// Scan a block using the mock RPC.
    ///
    /// Note: This manually calls RPC methods since WalletState.scan_block_by_height
    /// uses its internal RPC client. For deterministic tests, we need to mock the
    /// responses directly.
    pub async fn scan_block_deterministic(
        &mut self,
        block_height: u64,
    ) -> Result<usize, String> {
        let block_height_usize: usize = block_height
            .try_into()
            .map_err(|_| format!("Height {} too large", block_height))?;

        // Get scannable block directly (recordings should include get_height first)
        // First call should be get_height, then get the block
        let _height = self.mock_rpc
            .get_height()
            .await
            .map_err(|e| format!("Failed to get height: {}", e))?;

        let block = self.mock_rpc
            .get_scannable_block_by_number(block_height_usize)
            .await
            .map_err(|e| format!("Failed to get scannable block: {}", e))?;

        // Scan the block
        self.wallet
            .scan_block(block, block_height)
            .await
            .map_err(|e| format!("Failed to scan block: {}", e))
    }
}

/// Get the path to a test vector file.
pub fn test_vector_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors")
        .join(filename)
}
