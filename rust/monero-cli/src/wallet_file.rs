use monero_rust::WalletOutput;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

/// Persistent wallet data, encrypted at rest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletData {
    /// The seed phrase, stored encrypted within the wallet file
    pub encrypted_seed: String,
    /// Network: "mainnet", "stagenet", or "testnet"
    pub network: String,
    /// Last synced block height
    pub last_sync_height: u64,
    /// Owned outputs discovered during scanning
    pub outputs: Vec<WalletOutput>,
}

/// Return the default wallet file path: ~/.monero-cli/wallet.json.enc
pub fn default_wallet_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".monero-cli").join("wallet.json.enc"))
}

/// Resolve the wallet path from an optional user-provided path, falling back to the default.
pub fn resolve_wallet_path(user_path: Option<&str>) -> Result<PathBuf, String> {
    match user_path {
        Some(p) => Ok(PathBuf::from(p)),
        None => default_wallet_path(),
    }
}

/// Save wallet data to disk, encrypted with the given password.
///
/// The plaintext JSON is encrypted using ChaCha20-Poly1305 with Argon2id key derivation
/// (via `monero_rust::encrypt`). The file contains raw encrypted bytes.
pub fn save_wallet(path: &Path, data: &WalletData, password: &str) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Failed to serialize wallet data: {}", e))?;

    let encrypted = monero_rust::encrypt(json.as_bytes(), password)
        .map_err(|e| format!("Failed to encrypt wallet data: {}", e))?;

    json.zeroize();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create wallet directory: {}", e))?;
    }

    // Write to a temporary file then atomically rename to prevent corruption
    // if the process crashes mid-write.
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &encrypted)
        .map_err(|e| format!("Failed to write wallet file: {}", e))?;
    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("Failed to finalize wallet file: {}", e))?;

    Ok(())
}

/// Load and decrypt wallet data from disk.
pub fn load_wallet(path: &Path, password: &str) -> Result<WalletData, String> {
    let encrypted = std::fs::read(path)
        .map_err(|e| format!("Failed to read wallet file: {}", e))?;

    let mut plaintext = monero_rust::decrypt(&encrypted, password)
        .map_err(|_| "Wrong password or corrupted wallet file".to_string())?;

    let data: WalletData = serde_json::from_slice(&plaintext)
        .map_err(|e| format!("Failed to parse wallet data: {}", e))?;

    plaintext.zeroize();

    Ok(data)
}
