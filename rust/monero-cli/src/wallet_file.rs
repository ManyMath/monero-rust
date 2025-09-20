use monero_rust::WalletOutput;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

/// Persistent wallet data — the full view returned to callers.
#[derive(Clone, Serialize, Deserialize)]
pub struct WalletData {
    /// The seed phrase (plaintext within the encrypted wallet file)
    #[serde(alias = "encrypted_seed")]
    pub mnemonic: String,
    /// Network: "mainnet", "stagenet", or "testnet"
    pub network: String,
    /// Last synced block height
    pub last_sync_height: u64,
    /// Owned outputs discovered during scanning
    pub outputs: Vec<WalletOutput>,
}

/// Sidecar `.cache` file contents: sync state that the `.keys` format doesn't carry.
#[derive(Serialize, Deserialize)]
struct WalletCache {
    last_sync_height: u64,
    outputs: Vec<WalletOutput>,
}

/// Return the default wallet file path: ~/.monero-cli/wallet.keys
pub fn default_wallet_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".monero-cli").join("wallet.keys"))
}

/// Resolve the wallet path from an optional user-provided path, falling back to the default.
pub fn resolve_wallet_path(user_path: Option<&str>) -> Result<PathBuf, String> {
    match user_path {
        Some(p) => Ok(PathBuf::from(p)),
        None => default_wallet_path(),
    }
}

fn network_to_nettype(network: &str) -> Result<u8, String> {
    match network {
        "mainnet" => Ok(0),
        "stagenet" => Ok(1),
        "testnet" => Ok(2),
        _ => Err(format!("Unknown network: {}", network)),
    }
}

fn nettype_to_network(nettype: u8) -> Result<String, String> {
    match nettype {
        0 => Ok("mainnet".to_string()),
        1 => Ok("stagenet".to_string()),
        2 => Ok("testnet".to_string()),
        _ => Err(format!("Unknown nettype: {}", nettype)),
    }
}

/// Save wallet data to disk as a `.keys` file + sidecar `.cache` file.
///
/// The `.keys` file stores identity (keys, mnemonic) in Monero's native format.
/// The `.cache` file stores sync state (height, outputs) encrypted with ChaCha20-Poly1305+Argon2.
pub fn save_wallet(path: &Path, data: &WalletData, password: &str) -> Result<(), String> {
    // --- 1. Build and write the .keys file ---
    let keys = monero_rust::derive_keys(&data.mnemonic, &data.network, "")?;

    let spend_secret = hex_to_key(&keys.secret_spend_key, "secret_spend_key")?;
    let view_secret = hex_to_key(&keys.secret_view_key, "secret_view_key")?;
    let spend_public = hex_to_key(&keys.public_spend_key, "public_spend_key")?;
    let view_public = hex_to_key(&keys.public_view_key, "public_view_key")?;
    let nettype = network_to_nettype(&data.network)?;

    let mut rng = rand::thread_rng();
    let encryption_iv: [u8; 8] = rng.gen();
    let outer_iv: [u8; 8] = rng.gen();

    let imported = monero_rust::ImportedKeysFile {
        spend_secret_key: spend_secret,
        view_secret_key: view_secret,
        spend_public_key: spend_public,
        view_public_key: view_public,
        creation_timestamp: 0,
        watch_only: false,
        seed_language: Some("English".to_string()),
        mnemonic: Some(data.mnemonic.clone()),
        encryption_iv,
        outer_iv,
        nettype,
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create wallet directory: {}", e))?;
    }

    monero_rust::write_keys_file(path, password, &imported)?;

    // --- 2. Write the sidecar .cache file ---
    let cache = WalletCache {
        last_sync_height: data.last_sync_height,
        outputs: data.outputs.clone(),
    };
    let mut cache_json = serde_json::to_string(&cache)
        .map_err(|e| format!("Failed to serialize cache: {}", e))?;

    let encrypted_cache = monero_rust::encrypt(cache_json.as_bytes(), password)
        .map_err(|e| format!("Failed to encrypt cache: {}", e))?;

    cache_json.zeroize();

    let cache_path = path.with_extension("cache");
    let cache_tmp = cache_path.with_extension("cache.tmp");
    std::fs::write(&cache_tmp, &encrypted_cache)
        .map_err(|e| format!("Failed to write cache file: {}", e))?;
    std::fs::rename(&cache_tmp, &cache_path)
        .map_err(|e| format!("Failed to finalize cache file: {}", e))?;

    Ok(())
}

/// Load and decrypt wallet data from a `.keys` file + optional sidecar `.cache`.
pub fn load_wallet(path: &Path, password: &str) -> Result<WalletData, String> {
    // --- 1. Read the .keys file ---
    let imported = monero_rust::read_keys_file(path, password)?;

    let mnemonic = imported
        .mnemonic
        .ok_or("No mnemonic in .keys file (watch-only wallets are not supported)")?;

    let network = nettype_to_network(imported.nettype)?;

    // --- 2. Read the sidecar .cache file if it exists ---
    let cache_path = path.with_extension("cache");
    let (last_sync_height, outputs) = if cache_path.exists() {
        let encrypted = std::fs::read(&cache_path)
            .map_err(|e| format!("Failed to read cache file: {}", e))?;

        let mut plaintext = monero_rust::decrypt(&encrypted, password)
            .map_err(|_| "Wrong password or corrupted cache file".to_string())?;

        let cache: WalletCache = serde_json::from_slice(&plaintext)
            .map_err(|e| format!("Failed to parse cache data: {}", e))?;

        plaintext.zeroize();

        (cache.last_sync_height, cache.outputs)
    } else {
        (0, Vec::new())
    };

    Ok(WalletData {
        mnemonic,
        network,
        last_sync_height,
        outputs,
    })
}

fn hex_to_key(hex_str: &str, name: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_str)
        .map_err(|e| format!("Failed to decode {}: {}", name, e))?;
    bytes
        .try_into()
        .map_err(|_| format!("{}: expected 32 bytes", name))
}
