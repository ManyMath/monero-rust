use monero_rust::WalletOutput;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

/// Persistent wallet data, the full view returned to callers.
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

impl WalletData {
    pub fn is_view_only(&self) -> bool {
        self.mnemonic.starts_with("viewonly:")
    }
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

    let spend_secret = if data.is_view_only() {
        [0u8; 32]
    } else {
        hex_to_key(&keys.secret_spend_key, "secret_spend_key")?
    };
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
        watch_only: data.is_view_only(),
        seed_language: Some("English".to_string()),
        mnemonic: if data.is_view_only() {
            None
        } else {
            Some(data.mnemonic.clone())
        },
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
    let mut cache_json =
        serde_json::to_string(&cache).map_err(|e| format!("Failed to serialize cache: {}", e))?;

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

    let mnemonic = match imported.mnemonic {
        Some(mnemonic) => mnemonic,
        None if imported.watch_only => format!(
            "viewonly:{}:{}",
            hex::encode(imported.view_secret_key),
            hex::encode(imported.spend_public_key)
        ),
        None => return Err("No mnemonic in full-access .keys file".to_string()),
    };

    let network = nettype_to_network(imported.nettype)?;

    // --- 2. Read the sidecar .cache file if it exists ---
    let cache_path = path.with_extension("cache");
    let (last_sync_height, outputs) = if cache_path.exists() {
        let encrypted =
            std::fs::read(&cache_path).map_err(|e| format!("Failed to read cache file: {}", e))?;

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
    let bytes = hex::decode(hex_str).map_err(|e| format!("Failed to decode {}: {}", name, e))?;
    bytes
        .try_into()
        .map_err(|_| format!("{}: expected 32 bytes", name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLD_SIGNING_VIEW_KEY: &str =
        "49774391fa5e8d249fc2c5b45dadef13534bf2483dede880dac88f061e809100";
    const COLD_SIGNING_PUBLIC_SPEND_KEY: &str =
        "1b3bd040020d3712ab84992b773d0a965134eb2df0392fb84af95de8a17be2ab";
    const COLD_SIGNING_ADDRESS: &str =
        "42ey1afDFnn4886T7196doS9GPMzexD9gXpsZJDwVjeRVdFCSoHnv7KPbBeGpzJBzHRCAs9UxqeoyFQMYbqSWYTfJJQAWDm";

    fn vector_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../monero-rust/tests/vectors/cold_signing_regtest_v0_18_5_0")
            .join(name)
    }

    fn temp_wallet_path(name: &str) -> PathBuf {
        let nonce: u64 = rand::random();
        std::env::temp_dir().join(format!("monero-cli-{name}-{nonce}.keys"))
    }

    #[test]
    fn loads_watch_only_keys_file_as_viewonly_sentinel() {
        let data = load_wallet(&vector_path("hot_view_only.keys"), "")
            .expect("watch-only keys fixture should load");

        assert_eq!(data.network, "mainnet");
        assert!(data.is_view_only());
        assert_eq!(
            data.mnemonic,
            format!("viewonly:{COLD_SIGNING_VIEW_KEY}:{COLD_SIGNING_PUBLIC_SPEND_KEY}")
        );
        assert_eq!(
            monero_rust::derive_address(&data.mnemonic, &data.network, "")
                .expect("view-only sentinel should derive address"),
            COLD_SIGNING_ADDRESS
        );
    }

    #[test]
    fn saves_watch_only_wallet_without_requiring_mnemonic() {
        let path = temp_wallet_path("watch-only-roundtrip");
        let cache_path = path.with_extension("cache");
        let data = WalletData {
            mnemonic: format!("viewonly:{COLD_SIGNING_VIEW_KEY}:{COLD_SIGNING_PUBLIC_SPEND_KEY}"),
            network: "mainnet".to_string(),
            last_sync_height: 42,
            outputs: Vec::new(),
        };

        save_wallet(&path, &data, "").expect("watch-only wallet should save");

        let imported =
            monero_rust::read_keys_file(&path, "").expect("saved watch-only keys should reload");
        assert!(imported.watch_only);
        assert!(imported.mnemonic.is_none());
        assert_eq!(hex::encode(imported.view_secret_key), COLD_SIGNING_VIEW_KEY);
        assert_eq!(
            hex::encode(imported.spend_public_key),
            COLD_SIGNING_PUBLIC_SPEND_KEY
        );

        let loaded = load_wallet(&path, "").expect("saved wallet should load");
        assert_eq!(loaded.mnemonic, data.mnemonic);
        assert_eq!(loaded.last_sync_height, 42);

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(cache_path);
    }
}
