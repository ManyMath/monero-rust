//! Wallet state with encryption support.

use crate::crypto::{decrypt_wallet_data, encrypt_wallet_data, generate_nonce};

// Magic number for FFI pointer validation
const WALLET_MAGIC: u64 = 0x4D4F4E45524F5758; // "MONEROX"
use crate::types::{KeyImage, SerializableOutput, Transaction, TxKey};
use crate::WalletError;
use curve25519_dalek::scalar::Scalar;
use monero_seed::Seed;
use monero_generators::biased_hash_to_point;
use monero_oxide::transaction::Input as TxInput;
use monero_wallet::{address::{Network, SubaddressIndex}, rpc::Rpc, Scanner, ViewPair};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

#[derive(Serialize)]
pub struct WalletState {
    #[serde(skip)]
    pub magic: u64,

    #[serde(default = "default_version")]
    pub version: u32,

    /// None for view-only wallets
    #[serde(serialize_with = "serialize_seed_option")]
    pub seed: Option<Zeroizing<Seed>>,

    /// Reconstructed from seed on deserialization
    #[serde(skip)]
    pub view_pair: ViewPair,

    /// For view-only wallets only
    pub view_only_spend_public: Option<[u8; 32]>,
    #[serde(
        serialize_with = "serialize_view_key_option",
        deserialize_with = "deserialize_view_key_option"
    )]
    pub view_only_view_private: Option<Zeroizing<[u8; 32]>>,

    /// None for view-only wallets
    #[serde(
        serialize_with = "serialize_spend_key",
        deserialize_with = "deserialize_spend_key"
    )]
    pub spend_key: Option<Zeroizing<Scalar>>,

    #[serde(
        serialize_with = "serialize_network",
        deserialize_with = "deserialize_network"
    )]
    pub network: Network,
    pub seed_language: String,

    pub outputs: HashMap<KeyImage, SerializableOutput>,
    /// Excluded from spending until thawed
    pub frozen_outputs: HashSet<KeyImage>,
    pub spent_outputs: HashSet<KeyImage>,

    pub transactions: HashMap<[u8; 32], Transaction>,
    /// For payment proofs
    pub tx_keys: HashMap<[u8; 32], TxKey>,

    pub refresh_from_height: u64,
    pub current_scanned_height: u64,
    pub daemon_height: u64,
    #[serde(skip)]
    pub is_syncing: bool,

    /// Recent block hashes for reorg detection (last ~100 blocks)
    #[serde(default)]
    pub block_hash_cache: HashMap<u64, [u8; 32]>,

    pub daemon_address: Option<String>,
    pub is_connected: bool,

    pub password_salt: [u8; 32],
    /// For password verification only, not encryption
    pub password_hash: [u8; 32],
    pub wallet_path: PathBuf,
    pub is_closed: bool,

    /// Keccak256 of public keys for integrity check
    #[serde(rename = "keys_checksum")]
    pub keys_checksum: [u8; 32],

    #[serde(skip)]
    pub auto_save_enabled: bool,

    /// RPC client (runtime only, re-establish after loading)
    #[serde(skip)]
    pub rpc_client: std::sync::Arc<tokio::sync::RwLock<Option<monero_simple_request_rpc::SimpleRequestRpc>>>,

    #[serde(skip)]
    pub health_check_handle: Option<tokio::task::JoinHandle<()>>,

    #[serde(skip)]
    pub reconnection_policy: crate::rpc::ReconnectionPolicy,

    #[serde(skip)]
    pub reconnection_attempts: u32,

    /// Stored for reconnection (includes credentials, not persisted for security)
    #[serde(skip)]
    pub connection_config: Option<crate::rpc::ConnectionConfig>,

    /// Scanner for detecting owned outputs in blocks. Reconstructed from ViewPair on load.
    #[serde(skip)]
    pub scanner: Scanner,

    /// Subaddresses registered for scanning. Primary address (0,0) is handled automatically.
    #[serde(
        serialize_with = "serialize_subaddress_vec",
        deserialize_with = "deserialize_subaddress_vec"
    )]
    pub registered_subaddresses: Vec<SubaddressIndex>,

    #[serde(skip)]
    pub sync_handle: Option<tokio::task::JoinHandle<()>>,

    #[serde(skip)]
    pub sync_interval: std::time::Duration,

    #[serde(skip)]
    pub sync_progress_callback: Option<std::sync::Arc<Box<dyn Fn(u64, u64) + Send + Sync>>>,
}

const WALLET_VERSION: u32 = 1;

fn default_version() -> u32 {
    WALLET_VERSION
}

fn compute_keys_checksum(view_pair: &ViewPair) -> [u8; 32] {
    use sha3::{Digest, Keccak256};

    let spend_bytes = view_pair.spend().compress().to_bytes();
    let view_bytes = view_pair.view().compress().to_bytes();

    let mut hasher = Keccak256::new();
    hasher.update(&spend_bytes);
    hasher.update(&view_bytes);
    hasher.finalize().into()
}

fn serialize_subaddress_vec<S>(
    vec: &Vec<SubaddressIndex>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize;
    let pairs: Vec<(u32, u32)> = vec.iter().map(|idx| (idx.account(), idx.address())).collect();
    pairs.serialize(serializer)
}

#[allow(dead_code)]
fn deserialize_subaddress_vec<'de, D>(deserializer: D) -> Result<Vec<SubaddressIndex>, D::Error>
where D: serde::Deserializer<'de> {
    let pairs: Vec<(u32, u32)> = Vec::deserialize(deserializer)?;
    pairs
        .into_iter()
        .map(|(account, address)| {
            SubaddressIndex::new(account, address)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid subaddress: ({}, {})", account, address)))
        })
        .collect()
}

fn serialize_seed_option<S>(
    seed: &Option<Zeroizing<Seed>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match seed {
        Some(s) => serializer.serialize_some(&*s.entropy() as &[u8]),
        None => serializer.serialize_none(),
    }
}

fn serialize_view_key_option<S>(
    key: &Option<Zeroizing<[u8; 32]>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match key {
        Some(k) => serializer.serialize_some(&**k),
        None => serializer.serialize_none(),
    }
}

fn deserialize_view_key_option<'de, D>(
    deserializer: D,
) -> Result<Option<Zeroizing<[u8; 32]>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<[u8; 32]> = Option::deserialize(deserializer)?;
    Ok(opt.map(Zeroizing::new))
}

mod serde_zeroizing_option {
    use super::*;
    use serde::Deserializer;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Zeroizing<[u8; 32]>>, D::Error>
    where D: Deserializer<'de> {
        super::deserialize_view_key_option(deserializer)
    }
}

fn serialize_network<S>(network: &Network, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let network_u8 = match network {
        Network::Mainnet => 0u8,
        Network::Testnet => 1u8,
        Network::Stagenet => 2u8,
    };
    serializer.serialize_u8(network_u8)
}

fn deserialize_network<'de, D>(deserializer: D) -> Result<Network, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let network_u8: u8 = Deserialize::deserialize(deserializer)?;
    match network_u8 {
        0 => Ok(Network::Mainnet),
        1 => Ok(Network::Testnet),
        2 => Ok(Network::Stagenet),
        _ => Err(serde::de::Error::custom("Invalid network type")),
    }
}

fn serialize_spend_key<S>(
    spend_key: &Option<Zeroizing<Scalar>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match spend_key {
        Some(key) => serializer.serialize_some(&*Zeroizing::new(key.to_bytes())),
        None => serializer.serialize_none(),
    }
}

fn deserialize_spend_key<'de, D>(
    deserializer: D,
) -> Result<Option<Zeroizing<Scalar>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let bytes: Option<[u8; 32]> = Deserialize::deserialize(deserializer)?;
    Ok(bytes.map(|b| Zeroizing::new(Scalar::from_bytes_mod_order(b))))
}

impl WalletState {
    pub fn hash_password(password: &str, salt: &[u8; 32]) -> Result<[u8; 32], String> {
        use argon2::{
            password_hash::{PasswordHasher, SaltString},
            Algorithm, Argon2, ParamsBuilder, Version,
        };

        let mut builder = ParamsBuilder::new();
        builder.m_cost(65536);
        builder.t_cost(3);
        builder.p_cost(4);
        builder.output_len(32);
        let params = builder.build().map_err(|e| e.to_string())?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let salt_string = SaltString::encode_b64(salt).map_err(|e| e.to_string())?;
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt_string)
            .map_err(|e| e.to_string())?;

        let hash_bytes = password_hash.hash.ok_or("missing hash")?;
        if hash_bytes.len() != 32 {
            return Err(format!("bad hash length: {}", hash_bytes.len()));
        }

        let mut result = [0u8; 32];
        result.copy_from_slice(hash_bytes.as_bytes());
        Ok(result)
    }

    pub fn generate_salt() -> [u8; 32] {
        use rand_core::RngCore;
        let mut salt = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut salt);
        salt
    }

    pub fn verify_password(
        password: &str,
        salt: &[u8; 32],
        expected_hash: &[u8; 32],
    ) -> Result<(), String> {
        let computed_hash = Self::hash_password(password, salt)?;
        use subtle::ConstantTimeEq;
        if computed_hash.ct_eq(expected_hash).into() {
            Ok(())
        } else {
            Err("invalid password".to_string())
        }
    }

    pub fn new(
        seed: Seed,
        seed_language: String,
        network: Network,
        password: &str,
        wallet_path: PathBuf,
        refresh_from_height: u64,
    ) -> Result<Self, String> {
        let password_salt = Self::generate_salt();
        let password_hash = Self::hash_password(password, &password_salt)?;
        use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint};
        use sha3::{Digest, Keccak256};

        let spend: [u8; 32] = *seed.entropy();
        let spend_scalar = Scalar::from_bytes_mod_order(spend);
        let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        let view_pair =
            ViewPair::new(spend_point, Zeroizing::new(view_scalar)).map_err(|e| e.to_string())?;
        let keys_checksum = compute_keys_checksum(&view_pair);
        let scanner = Scanner::new(view_pair.clone());

        Ok(Self {
            magic: WALLET_MAGIC,
            version: WALLET_VERSION,
            seed: Some(Zeroizing::new(seed)),
            view_pair,
            view_only_spend_public: None, // Not a view-only wallet
            view_only_view_private: None, // Not a view-only wallet
            spend_key: Some(Zeroizing::new(spend_scalar)),
            network,
            seed_language,
            outputs: HashMap::new(),
            frozen_outputs: HashSet::new(),
            spent_outputs: HashSet::new(),
            transactions: HashMap::new(),
            tx_keys: HashMap::new(),
            refresh_from_height,
            current_scanned_height: refresh_from_height,
            daemon_height: 0,
            is_syncing: false,
            block_hash_cache: HashMap::new(),
            daemon_address: None,
            is_connected: false,
            password_salt,
            password_hash,
            wallet_path,
            is_closed: false,
            keys_checksum,
            auto_save_enabled: false,
            rpc_client: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            health_check_handle: None,
            reconnection_policy: crate::rpc::ReconnectionPolicy::default(),
            reconnection_attempts: 0,
            connection_config: None,
            scanner,
            registered_subaddresses: Vec::new(),
            sync_handle: None,
            sync_interval: std::time::Duration::from_secs(1),
            sync_progress_callback: None,
        })
    }

    pub fn new_view_only(
        spend_public_key: [u8; 32],
        view_private_key: [u8; 32],
        network: Network,
        password: &str,
        wallet_path: PathBuf,
        refresh_from_height: u64,
    ) -> Result<Self, String> {
        let password_salt = Self::generate_salt();
        let password_hash = Self::hash_password(password, &password_salt)?;
        use curve25519_dalek::edwards::CompressedEdwardsY;

        let spend_point = CompressedEdwardsY(spend_public_key)
            .decompress()
            .ok_or("invalid spend public key")?;
        let view_scalar = Scalar::from_bytes_mod_order(view_private_key);
        let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar))
            .map_err(|e| e.to_string())?;
        let keys_checksum = compute_keys_checksum(&view_pair);
        let scanner = Scanner::new(view_pair.clone());

        Ok(Self {
            magic: WALLET_MAGIC,
            version: WALLET_VERSION,
            seed: None,
            view_pair,
            view_only_spend_public: Some(spend_public_key),
            view_only_view_private: Some(Zeroizing::new(view_private_key)),
            spend_key: None,
            network,
            seed_language: String::from("N/A"),
            outputs: HashMap::new(),
            frozen_outputs: HashSet::new(),
            spent_outputs: HashSet::new(),
            transactions: HashMap::new(),
            tx_keys: HashMap::new(),
            refresh_from_height,
            current_scanned_height: refresh_from_height,
            daemon_height: 0,
            is_syncing: false,
            block_hash_cache: HashMap::new(),
            daemon_address: None,
            is_connected: false,
            password_salt,
            password_hash,
            wallet_path,
            is_closed: false,
            keys_checksum,
            auto_save_enabled: false,
            rpc_client: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            health_check_handle: None,
            reconnection_policy: crate::rpc::ReconnectionPolicy::default(),
            reconnection_attempts: 0,
            connection_config: None,
            scanner,
            registered_subaddresses: Vec::new(),
            sync_handle: None,
            sync_interval: std::time::Duration::from_secs(1),
            sync_progress_callback: None,
        })
    }

    /// Validates that a WalletState pointer looks legit.
    /// Not foolproof but catches obvious issues like dangling or garbage pointers.
    pub unsafe fn validate_ptr(ptr: *const WalletState) -> bool {
        if ptr.is_null() {
            return false;
        }
        // Check magic number - won't catch everything but helps
        unsafe { (*ptr).magic == WALLET_MAGIC }
    }

    pub fn is_view_only(&self) -> bool {
        self.spend_key.is_none()
    }

    pub fn get_balance(&self) -> u64 {
        self.outputs
            .iter()
            .filter(|(ki, _)| !self.spent_outputs.contains(*ki))
            .map(|(_, output)| output.amount)
            .fold(0u64, |acc, amt| acc.saturating_add(amt))
    }

    /// Spendable balance (10+ confirmations, not frozen)
    pub fn get_unlocked_balance(&self) -> u64 {
        const LOCK_BLOCKS: u64 = 10;
        self.outputs
            .iter()
            .filter(|(ki, output)| {
                !self.spent_outputs.contains(*ki)
                    && !self.frozen_outputs.contains(*ki)
                    && self.daemon_height >= output.height.saturating_add(LOCK_BLOCKS)
            })
            .map(|(_, output)| output.amount)
            .fold(0u64, |acc, amt| acc.saturating_add(amt))
    }

    pub fn is_synced(&self) -> bool {
        self.is_connected && self.current_scanned_height >= self.daemon_height
    }

    pub fn get_outputs_count(&self) -> usize {
        self.outputs.len()
    }

    pub fn get_spent_outputs_count(&self) -> usize {
        self.spent_outputs.len()
    }

    pub fn is_output_spent(&self, key_image: &KeyImage) -> bool {
        self.spent_outputs.contains(key_image)
    }

    pub fn is_output_frozen(&self, key_image: &KeyImage) -> bool {
        self.frozen_outputs.contains(key_image)
    }

    pub fn freeze_output(&mut self, key_image: &KeyImage) -> Result<(), WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }
        if !self.outputs.contains_key(key_image) {
            return Err(WalletError::Other(format!(
                "output {} not found",
                hex::encode(key_image)
            )));
        }
        self.frozen_outputs.insert(*key_image);
        Ok(())
    }

    pub fn thaw_output(&mut self, key_image: &KeyImage) -> Result<(), WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }
        if !self.outputs.contains_key(key_image) {
            return Err(WalletError::Other(format!(
                "output {} not found",
                hex::encode(key_image)
            )));
        }
        self.frozen_outputs.remove(key_image);
        Ok(())
    }

    /// Returns outputs, optionally including spent ones.
    pub fn get_outputs(&self, include_spent: bool) -> Result<Vec<SerializableOutput>, WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }

        let mut result = Vec::new();
        for (key_image, output) in &self.outputs {
            let is_spent = self.spent_outputs.contains(key_image);
            if !include_spent && is_spent {
                continue;
            }

            let mut out = output.clone();
            out.spent = is_spent;
            out.frozen = self.frozen_outputs.contains(key_image);
            result.push(out);
        }
        Ok(result)
    }

    pub fn get_transaction_count(&self) -> usize {
        self.transactions.len()
    }

    /// Returns the mnemonic seed, or None for view-only wallets.
    pub fn get_seed(&self) -> Option<String> {
        self.seed.as_ref().map(|seed| (*seed.to_string()).clone())
    }

    pub fn get_seed_language(&self) -> &str {
        &self.seed_language
    }

    /// Returns the private spend key as hex, or None for view-only wallets.
    pub fn get_private_spend_key(&self) -> Option<String> {
        self.spend_key.as_ref().map(|key| {
            let bytes = Zeroizing::new(key.to_bytes());
            hex::encode(&*bytes)
        })
    }

    /// Returns the private view key as hex.
    pub fn get_private_view_key(&self) -> String {
        use sha3::{Digest, Keccak256};

        if let Some(seed) = &self.seed {
            let view_bytes: [u8; 32] = Keccak256::digest(seed.entropy()).into();
            let view = Zeroizing::new(view_bytes);
            hex::encode(&*view)
        } else if let Some(view_private) = &self.view_only_view_private {
            hex::encode(&**view_private)
        } else {
            panic!("WalletState has neither seed nor view key")
        }
    }

    pub fn get_public_spend_key(&self) -> String {
        hex::encode(self.view_pair.spend().compress().to_bytes())
    }

    pub fn get_public_view_key(&self) -> String {
        hex::encode(self.view_pair.view().compress().to_bytes())
    }

    pub fn get_path(&self) -> &std::path::Path {
        &self.wallet_path
    }

    // ========================================================================
    // RPC CONNECTION MANAGEMENT
    // ========================================================================

    /// Connect to a Monero daemon. Disconnects any existing connection first.
    pub async fn connect(&mut self, config: crate::rpc::ConnectionConfig) -> Result<(), WalletError> {
        use monero_simple_request_rpc::SimpleRequestRpc;

        // Already connected to this daemon
        if self.is_connected && self.daemon_address.as_ref() == Some(&config.daemon_address) {
            return Ok(());
        }

        if self.is_connected {
            self.disconnect().await;
        }

        let url = config.build_url();
        let rpc = SimpleRequestRpc::with_custom_timeout(url.as_str().to_string(), config.timeout)
            .await
            .map_err(WalletError::RpcError)?;

        let daemon_height = rpc.get_height().await.map_err(WalletError::RpcError)?;

        self.daemon_address = Some(config.daemon_address.clone());
        self.daemon_height = daemon_height as u64;
        self.is_connected = true;
        self.reconnection_attempts = 0;
        self.connection_config = Some(config);

        {
            let mut client = self.rpc_client.write().await;
            *client = Some(rpc);
        }

        self.start_health_check().await;
        Ok(())
    }

    pub async fn disconnect(&mut self) {
        self.stop_health_check().await;

        {
            let mut client = self.rpc_client.write().await;
            *client = None;
        }

        self.connection_config = None;
        self.is_connected = false;
        self.reconnection_attempts = 0;
    }

    pub fn is_connected_to_daemon(&self) -> bool {
        self.is_connected
    }

    async fn get_rpc(&self) -> Result<monero_simple_request_rpc::SimpleRequestRpc, WalletError> {
        let client = self.rpc_client.read().await;
        client.clone().ok_or(WalletError::NotConnected)
    }

    pub async fn check_connection(&mut self) -> Result<(), WalletError> {
        let rpc = self.get_rpc().await?;

        match rpc.get_height().await {
            Ok(height) => {
                self.daemon_height = height as u64;
                self.is_connected = true;
                self.reconnection_attempts = 0;
                Ok(())
            }
            Err(e) => {
                self.is_connected = false;
                Err(WalletError::RpcError(e))
            }
        }
    }

    /// Refreshes output unlock status from daemon.
    pub async fn refresh_outputs(&mut self) -> Result<(), WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }
        if !self.is_connected {
            return Err(WalletError::NotConnected);
        }

        let rpc = self.get_rpc().await?;
        let daemon_height = rpc.get_height().await.map_err(WalletError::RpcError)? as u64;
        self.daemon_height = daemon_height;

        const LOCK_BLOCKS: u64 = 10;
        for output in self.outputs.values_mut() {
            output.unlocked = daemon_height >= output.height.saturating_add(LOCK_BLOCKS);
        }

        Ok(())
    }

    /// Refreshes transaction confirmation counts from daemon.
    pub async fn refresh_transactions(&mut self) -> Result<(), WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }

        let rpc = self.get_rpc().await?;
        let daemon_height = match rpc.get_height().await {
            Ok(height) => height as u64,
            Err(e) => {
                self.is_connected = false;
                return Err(WalletError::RpcError(e));
            }
        };

        self.daemon_height = daemon_height;

        for tx in self.transactions.values_mut() {
            if let Some(tx_height) = tx.height {
                tx.confirmations = daemon_height.saturating_sub(tx_height).saturating_add(1);
                tx.is_pending = false;
            } else {
                tx.confirmations = 0;
                tx.is_pending = true;
            }
        }

        Ok(())
    }

    async fn start_health_check(&mut self) {
        self.stop_health_check().await;

        let rpc_client = self.rpc_client.clone();
        let interval = self.reconnection_policy.health_check_interval;
        let policy = self.reconnection_policy.clone();
        let config = self.connection_config.clone();

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                ticker.tick().await;

                let rpc = {
                    let lock = rpc_client.read().await;
                    lock.clone()
                };

                let Some(rpc) = rpc else {
                    break;
                };

                if rpc.get_height().await.is_err() {
                    Self::attempt_reconnect(rpc_client.clone(), config.clone(), policy.clone()).await;
                }
            }
        });

        self.health_check_handle = Some(handle);
    }

    async fn stop_health_check(&mut self) {
        if let Some(handle) = self.health_check_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
    }

    async fn attempt_reconnect(
        rpc_client: std::sync::Arc<tokio::sync::RwLock<Option<monero_simple_request_rpc::SimpleRequestRpc>>>,
        config: Option<crate::rpc::ConnectionConfig>,
        policy: crate::rpc::ReconnectionPolicy,
    ) {
        use monero_simple_request_rpc::SimpleRequestRpc;

        let Some(config) = config else { return };

        for attempt in 0..policy.max_attempts {
            tokio::time::sleep(policy.delay_for_attempt(attempt)).await;

            let url = config.build_url();
            let Ok(new_rpc) = SimpleRequestRpc::with_custom_timeout(
                url.as_str().to_string(),
                config.timeout,
            ).await else {
                continue;
            };

            if new_rpc.get_height().await.is_ok() {
                let mut lock = rpc_client.write().await;
                *lock = Some(new_rpc);
                return;
            }
        }
    }

    // ========================================================================
    // BLOCKCHAIN SCANNING
    // ========================================================================

    /// Scans transaction inputs to find which of our outputs were spent.
    fn detect_spent_outputs_in_block(
        &self,
        block: &monero_rpc::ScannableBlock,
    ) -> HashSet<KeyImage> {
        let mut spent = HashSet::new();

        macro_rules! check_inputs {
            ($tx:expr) => {
                for input in $tx.prefix().inputs.iter() {
                    if let TxInput::ToKey { key_image, .. } = input {
                        let ki = key_image.to_bytes();
                        if self.outputs.contains_key(&ki) {
                            spent.insert(ki);
                        }
                    }
                }
            };
        }

        check_inputs!(block.block.miner_transaction());
        for tx in &block.transactions {
            check_inputs!(tx);
        }

        spent
    }

    /// Creates transaction records for any txs involving this wallet.
    fn create_transaction_records(
        &self,
        discovered_outputs: &[SerializableOutput],
        spent_in_block: &HashSet<KeyImage>,
        block: &monero_rpc::ScannableBlock,
        block_height: u64,
    ) -> HashMap<[u8; 32], Transaction> {
        use crate::types::TransactionDirection;
        let mut transactions = HashMap::new();

        // Group outputs by tx
        let mut outputs_by_tx: HashMap<[u8; 32], Vec<&SerializableOutput>> = HashMap::new();
        for output in discovered_outputs {
            outputs_by_tx.entry(output.tx_hash).or_default().push(output);
        }

        // Build map of tx_hash -> key images we own that were spent by that tx
        let mut tx_spent_kis: HashMap<[u8; 32], Vec<KeyImage>> = HashMap::new();

        macro_rules! collect_spent {
            ($tx:expr, $hash:expr) => {
                for input in $tx.prefix().inputs.iter() {
                    if let TxInput::ToKey { key_image, .. } = input {
                        let ki = key_image.to_bytes();
                        if spent_in_block.contains(&ki) {
                            tx_spent_kis.entry($hash).or_default().push(ki);
                        }
                    }
                }
            };
        }

        let miner_hash = block.block.miner_transaction().hash();
        collect_spent!(block.block.miner_transaction(), miner_hash);

        for (i, tx) in block.transactions.iter().enumerate() {
            let hash = block.block.transactions[i];
            collect_spent!(tx, hash);
        }

        for (tx_hash, outputs) in outputs_by_tx {
            let timestamp = block.block.header.timestamp;
            let spends_ours = tx_spent_kis.contains_key(&tx_hash);

            // Calculate total received (includes previously known outputs from the same tx)
            let mut received: u64 = outputs.iter().map(|o| o.amount).fold(0u64, |a, b| a.saturating_add(b));
            for (_, output) in self.outputs.iter() {
                if output.tx_hash == tx_hash {
                    let is_new = outputs.iter().any(|o| o.key_image == output.key_image);
                    if !is_new {
                        received = received.saturating_add(output.amount);
                    }
                }
            }

            let spent_amt: u64 = if spends_ours {
                tx_spent_kis.get(&tx_hash)
                    .map(|kis| kis.iter().filter_map(|ki| self.outputs.get(ki)).map(|o| o.amount).fold(0u64, |a, b| a.saturating_add(b)))
                    .unwrap_or(0)
            } else {
                0
            };

            let (direction, amount) = if spends_ours {
                // Outgoing: amount = spent - received (net loss to wallet)
                (TransactionDirection::Outgoing, spent_amt.saturating_sub(received))
            } else {
                (TransactionDirection::Incoming, received)
            };

            let payment_id = outputs.first().and_then(|o| o.payment_id.clone());

            // Only insert if transaction doesn't already exist (prevents duplicates during rescans)
            transactions.entry(tx_hash).or_insert(Transaction {
                txid: tx_hash,
                height: Some(block_height),
                timestamp,
                amount,
                fee: None,
                destinations: vec![],
                payment_id,
                direction,
                confirmations: 0,
                is_pending: false,
            });
        }

        transactions
    }

    /// Scans a block for owned outputs. Returns the number of outputs found.
    pub async fn scan_block(
        &mut self,
        block: monero_rpc::ScannableBlock,
        block_height: u64,
    ) -> Result<usize, WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }

        // Clone before scanning since scan() consumes the block
        let block_for_analysis = block.clone();

        let scan_result = self
            .scanner
            .scan(block)
            .map_err(|e| WalletError::Other(format!("failed to scan block {}: {}", block_height, e)))?;

        let outputs = scan_result.not_additionally_locked();
        let count = outputs.len();

        // Convert outputs without mutating state yet
        let mut converted = Vec::with_capacity(count);
        for wallet_output in outputs {
            converted.push(self.convert_to_serializable(wallet_output, block_height)?);
        }

        // Detect spent outputs and create tx records
        let spent_in_block = self.detect_spent_outputs_in_block(&block_for_analysis);
        let new_txs = self.create_transaction_records(&converted, &spent_in_block, &block_for_analysis, block_height);

        // Apply all changes atomically
        for ki in spent_in_block {
            self.spent_outputs.insert(ki);
        }

        for (hash, tx) in new_txs {
            // Only insert if not already present (prevents duplicates during rescans)
            self.transactions.entry(hash).or_insert(tx);
        }

        for output in converted {
            let key_image = output.key_image;
            if let Some(existing) = self.outputs.get(&key_image) {
                if existing.tx_hash == output.tx_hash && existing.output_index == output.output_index {
                    self.outputs.insert(key_image, output);
                    continue;
                }
                // Collision with different output - keep existing, skip new
                eprintln!(
                    "WARNING: key image collision at block {}: {} already maps to {}:{}, skipping {}:{}",
                    block_height,
                    hex::encode(key_image),
                    hex::encode(existing.tx_hash), existing.output_index,
                    hex::encode(output.tx_hash), output.output_index
                );
                continue;
            }
            self.outputs.insert(key_image, output);
        }

        self.current_scanned_height = block_height;
        Ok(count)
    }

    /// Fetches and scans a block by height.
    pub async fn scan_block_by_height(&mut self, height: u64) -> Result<usize, WalletError> {
        let rpc = self.get_rpc().await?;

        let height_usize: usize = height.try_into()
            .map_err(|_| WalletError::Other(format!("height {} exceeds platform limit", height)))?;

        // Cache block hash for reorg detection
        let block_hash = rpc.get_block_hash(height_usize).await.map_err(WalletError::RpcError)?;
        self.block_hash_cache.insert(height, block_hash);
        // Keep only last ~100 blocks
        if self.block_hash_cache.len() > 100 {
            if let Some(&oldest) = self.block_hash_cache.keys().min() {
                self.block_hash_cache.remove(&oldest);
            }
        }

        let block = rpc
            .get_scannable_block_by_number(height_usize)
            .await
            .map_err(WalletError::RpcError)?;

        self.scan_block(block, height).await
    }

    fn convert_to_serializable(
        &self,
        wallet_output: monero_wallet::WalletOutput,
        block_height: u64,
    ) -> Result<SerializableOutput, WalletError> {
        let key_image = self.compute_key_image(&wallet_output)?;

        let subaddress_indices = wallet_output
            .subaddress()
            .map(|idx| (idx.account(), idx.address()))
            .unwrap_or((0, 0));

        // payment_id extraction would require changes to monero-wallet (PaymentId is pub(crate))
        let payment_id = None;

        Ok(SerializableOutput {
            tx_hash: wallet_output.transaction(),
            output_index: wallet_output.index_in_transaction(),
            amount: wallet_output.commitment().amount,
            key_image,
            subaddress_indices,
            height: block_height,
            unlocked: false,
            spent: false,
            frozen: false,
            payment_id,
        })
    }

    /// Computes the key image for an output.
    /// Full wallets use the proper formula: (spend_key + key_offset) * H_p(output_key).
    /// View-only wallets use a deterministic placeholder since they lack the spend key.
    fn compute_key_image(&self, wallet_output: &monero_wallet::WalletOutput) -> Result<KeyImage, WalletError> {
        match &self.spend_key {
            Some(spend_key) => {
                let output_key = wallet_output.key();
                let key_offset = wallet_output.key_offset();
                let effective_spend_key = spend_key.deref() + key_offset;
                let hash_point = biased_hash_to_point(output_key.compress().to_bytes());
                let key_image_point = effective_spend_key * hash_point;
                Ok(key_image_point.compress().to_bytes())
            }
            None => {
                use sha3::{Digest, Keccak256};
                let mut hasher = Keccak256::new();
                hasher.update(wallet_output.transaction());
                hasher.update(&wallet_output.index_in_transaction().to_le_bytes());
                Ok(hasher.finalize().into())
            }
        }
    }

    // ========================================================================
    // REORGANIZATION HANDLING
    // ========================================================================

    /// Handles a blockchain reorganization by removing outputs at or after fork_height.
    /// Returns the number of outputs removed.
    pub fn handle_reorganization(&mut self, fork_height: u64) -> usize {
        let mut removed_count = 0;
        let mut removed_keys = Vec::new();

        self.outputs.retain(|key_image, output| {
            if output.height >= fork_height {
                removed_keys.push(*key_image);
                removed_count += 1;
                false
            } else {
                true
            }
        });

        self.transactions.retain(|_, tx| {
            tx.height.map_or(true, |h| h < fork_height)
        });

        for key_image in removed_keys {
            self.spent_outputs.remove(&key_image);
            self.frozen_outputs.remove(&key_image);
        }

        self.current_scanned_height = fork_height.saturating_sub(1);

        eprintln!(
            "reorg at height {}: removed {} outputs, rewound to {}",
            fork_height, removed_count, self.current_scanned_height
        );

        removed_count
    }

    /// Checks if a reorganization occurred by comparing heights and block hashes.
    pub async fn detect_reorganization(&mut self) -> Result<Option<u64>, WalletError> {
        let rpc = self.get_rpc().await?;
        let daemon_height = rpc.get_height().await.map_err(WalletError::RpcError)? as u64;
        self.daemon_height = daemon_height;

        // Obvious reorg: daemon is behind us
        if daemon_height < self.current_scanned_height {
            let fork_height = daemon_height.saturating_sub(10);
            eprintln!("REORG: daemon height {} < wallet height {}, rewinding to {}",
                daemon_height, self.current_scanned_height, fork_height);
            return Ok(Some(fork_height));
        }

        // Check cached block hashes against daemon
        let check_start = self.current_scanned_height.saturating_sub(100);
        for height in (check_start..=self.current_scanned_height).rev() {
            if let Some(cached_hash) = self.block_hash_cache.get(&height) {
                let h: usize = match height.try_into() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match rpc.get_block_hash(h).await {
                    Ok(daemon_hash) => {
                        if &daemon_hash != cached_hash {
                            let fork_height = height.saturating_sub(10);
                            eprintln!("REORG: block hash mismatch at {}, rewinding to {}",
                                height, fork_height);
                            return Ok(Some(fork_height));
                        }
                    }
                    Err(_) => continue,
                }
            }
        }

        Ok(None)
    }

    // ========================================================================
    // SYNC LOOP
    // ========================================================================

    pub fn set_sync_progress_callback(
        &mut self,
        callback: Option<std::sync::Arc<Box<dyn Fn(u64, u64) + Send + Sync>>>,
    ) {
        self.sync_progress_callback = callback;
    }

    /// Marks the wallet as syncing and validates preconditions.
    /// Call this before `sync_once()`.
    pub async fn start_syncing(&mut self) -> Result<(), WalletError> {
        if self.is_syncing {
            return Ok(());
        }
        if !self.is_connected {
            return Err(WalletError::NotConnected);
        }
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }
        self.is_syncing = true;
        Ok(())
    }

    /// Scans one block if available. Returns true if a block was scanned.
    pub async fn sync_once(&mut self) -> Result<bool, WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }
        if !self.is_connected {
            return Err(WalletError::NotConnected);
        }
        if !self.is_syncing {
            return Err(WalletError::Other("Call start_syncing() first".to_string()));
        }

        let rpc = self.get_rpc().await?;
        let daemon_height = rpc.get_height().await.map_err(WalletError::RpcError)? as u64;
        self.daemon_height = daemon_height;

        if self.current_scanned_height >= daemon_height {
            if let Some(ref cb) = self.sync_progress_callback {
                cb(self.current_scanned_height, daemon_height);
            }
            return Ok(false);
        }

        if let Some(fork_height) = self.detect_reorganization().await? {
            self.handle_reorganization(fork_height);
        }

        let next_height = self.current_scanned_height + 1;

        const MAX_RETRIES: u32 = 3;
        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            match self.scan_block_by_height(next_height).await {
                Ok(_) => {
                    if let Some(ref cb) = self.sync_progress_callback {
                        cb(self.current_scanned_height, daemon_height);
                    }
                    return Ok(true);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < MAX_RETRIES {
                        let delay = std::time::Duration::from_secs(2u64.pow(attempt));
                        eprintln!(
                            "Block {} scan failed (attempt {}/{}), retrying in {:?}",
                            next_height, attempt + 1, MAX_RETRIES, delay
                        );
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    /// Clears the syncing flag.
    pub async fn stop_syncing(&mut self) {
        self.is_syncing = false;
        if let Some(handle) = self.sync_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
    }

    // ========================================================================
    // HEIGHT MANAGEMENT
    // ========================================================================

    pub fn get_refresh_from_height(&self) -> u64 {
        self.refresh_from_height
    }

    pub fn get_current_syncing_height(&self) -> u64 {
        self.current_scanned_height
    }

    pub fn get_daemon_height(&self) -> u64 {
        self.daemon_height
    }

    pub fn set_refresh_from_height(&mut self, height: u64) {
        self.refresh_from_height = height;
        self.current_scanned_height = height;
    }

    /// Clears all cached data and resets to refresh height.
    pub fn rescan_blockchain(&mut self) {
        self.outputs.clear();
        self.spent_outputs.clear();
        self.frozen_outputs.clear();
        self.transactions.clear();
        self.tx_keys.clear();
        self.current_scanned_height = self.refresh_from_height;
    }

    // ========================================================================
    // SUBADDRESS MANAGEMENT
    // ========================================================================

    /// Registers a subaddress for scanning. Primary address (0,0) is automatic.
    pub fn register_subaddress(&mut self, account: u32, address: u32) -> Result<(), WalletError> {
        let idx = SubaddressIndex::new(account, address)
            .ok_or_else(|| WalletError::Other("invalid subaddress index".to_string()))?;

        if !self.registered_subaddresses.iter().any(|i| i == &idx) {
            self.scanner.register_subaddress(idx);
            self.registered_subaddresses.push(idx);
        }

        Ok(())
    }

    /// Registers a range of subaddresses. Returns count registered.
    pub fn register_subaddress_range(
        &mut self,
        account: u32,
        start: u32,
        end: u32,
    ) -> Result<usize, WalletError> {
        let mut count = 0;
        for addr in start..=end {
            self.register_subaddress(account, addr)?;
            count += 1;
        }
        Ok(count)
    }

    /// Returns all registered subaddresses as (account, address) pairs.
    pub fn get_registered_subaddresses(&self) -> Vec<(u32, u32)> {
        self.registered_subaddresses
            .iter()
            .map(|idx| (idx.account(), idx.address()))
            .collect()
    }

    // ========================================================================
    // FILE I/O - Encrypted wallet persistence
    // ========================================================================

    /// Magic bytes identifying a monero-rust wallet file: "MNRS"
    const MAGIC_BYTES: &'static [u8; 4] = b"MNRS";

    /// Size of the fixed header in bytes (magic + version + salt + nonce)
    const HEADER_SIZE: usize = 4 + 4 + 32 + 12; // 52 bytes

    /// Saves wallet to configured path
    pub fn save(&self, password: &str) -> Result<(), WalletError> {
        let path = self.wallet_path.clone();
        self.save_to_file(&path, password)
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P, password: &str) -> Result<(), WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }

        Self::verify_password(password, &self.password_salt, &self.password_hash)
            .map_err(|_| WalletError::InvalidPassword)?;

        let path = path.as_ref();
        let serialized_data = bincode::serialize(self)
            .map_err(|e| WalletError::SerializationError(format!("Failed to serialize wallet: {}", e)))?;

        let nonce = generate_nonce();
        let encrypted_data = encrypt_wallet_data(
            &serialized_data,
            password,
            &self.password_salt,
            &nonce,
        )
        .map_err(WalletError::EncryptionError)?;

        let mut file_contents = Vec::with_capacity(Self::HEADER_SIZE + encrypted_data.len());
        file_contents.extend_from_slice(Self::MAGIC_BYTES);
        file_contents.extend_from_slice(&self.version.to_le_bytes());
        file_contents.extend_from_slice(&self.password_salt);
        file_contents.extend_from_slice(&nonce);
        file_contents.extend_from_slice(&encrypted_data);

        let temp_path = path.with_extension("tmp");

        #[cfg(unix)]
        let mut temp_file = {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp_path)?
        };

        #[cfg(not(unix))]
        let mut temp_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;

        temp_file.write_all(&file_contents)?;
        temp_file.sync_all()?;
        drop(temp_file);

        match fs::rename(&temp_path, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                Err(WalletError::IoError(e))
            }
        }
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P, password: &str) -> Result<Self, WalletError> {
        let path = path.as_ref();

        let mut file = fs::File::open(path)?;
        let mut file_contents = Vec::new();
        file.read_to_end(&mut file_contents)?;

        if file_contents.len() < Self::HEADER_SIZE {
            return Err(WalletError::CorruptedFile(format!(
                "File too small: expected at least {} bytes, got {}",
                Self::HEADER_SIZE,
                file_contents.len()
            )));
        }

        let magic = &file_contents[0..4];
        if magic != Self::MAGIC_BYTES {
            return Err(WalletError::CorruptedFile(format!(
                "Invalid magic bytes: expected {:?}, got {:?}",
                Self::MAGIC_BYTES,
                magic
            )));
        }

        let version_bytes: [u8; 4] = file_contents[4..8]
            .try_into()
            .map_err(|_| WalletError::CorruptedFile("Failed to read version".to_string()))?;
        let version = u32::from_le_bytes(version_bytes);

        if version > WALLET_VERSION {
            return Err(WalletError::UnsupportedVersion(version));
        }

        let salt: [u8; 32] = file_contents[8..40]
            .try_into()
            .map_err(|_| WalletError::CorruptedFile("Failed to read salt".to_string()))?;

        let nonce: [u8; 12] = file_contents[40..52]
            .try_into()
            .map_err(|_| WalletError::CorruptedFile("Failed to read nonce".to_string()))?;

        let encrypted_data = &file_contents[52..];

        let decrypted_data = Zeroizing::new(
            decrypt_wallet_data(encrypted_data, password, &salt, &nonce).map_err(|e| {
                if e.contains("invalid password") || e.contains("corrupted") {
                    WalletError::InvalidPassword
                } else {
                    WalletError::EncryptionError(e)
                }
            })?,
        );

        let wallet: WalletState = bincode::deserialize(&*decrypted_data)
            .map_err(|e| WalletError::SerializationError(format!("Failed to deserialize wallet: {}", e)))?;

        Ok(wallet)
    }
}

impl<'de> Deserialize<'de> for WalletState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WalletStateHelper {
            #[serde(default = "default_version")]
            version: u32,
            seed: Option<Vec<u8>>,
            #[serde(default)]
            view_only_spend_public: Option<[u8; 32]>,
            #[serde(default, with = "serde_zeroizing_option")]
            view_only_view_private: Option<Zeroizing<[u8; 32]>>,
            #[serde(deserialize_with = "deserialize_spend_key")]
            spend_key: Option<Zeroizing<Scalar>>,
            #[serde(deserialize_with = "deserialize_network")]
            network: Network,
            seed_language: String,
            outputs: HashMap<KeyImage, SerializableOutput>,
            frozen_outputs: HashSet<KeyImage>,
            spent_outputs: HashSet<KeyImage>,
            transactions: HashMap<[u8; 32], Transaction>,
            tx_keys: HashMap<[u8; 32], TxKey>,
            refresh_from_height: u64,
            current_scanned_height: u64,
            daemon_height: u64,
            #[serde(default)]
            block_hash_cache: HashMap<u64, [u8; 32]>,
            daemon_address: Option<String>,
            is_connected: bool,
            password_salt: [u8; 32],
            password_hash: [u8; 32],
            wallet_path: PathBuf,
            #[allow(dead_code)]
            is_closed: bool,
            #[serde(rename = "keys_checksum")]
            keys_checksum: [u8; 32],
            #[serde(default)]
            registered_subaddresses: Vec<(u32, u32)>,
        }

        let helper = WalletStateHelper::deserialize(deserializer)?;

        if helper.version > WALLET_VERSION {
            return Err(serde::de::Error::custom(format!(
                "wallet version {} > supported {}",
                helper.version, WALLET_VERSION
            )));
        }

        use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint};
        use sha3::{Digest, Keccak256};

        let (seed_opt, view_pair) = if let Some(seed_bytes) = helper.seed {
            if seed_bytes.len() != 32 {
                return Err(serde::de::Error::custom("bad seed length"));
            }

            let mut entropy = Zeroizing::new([0u8; 32]);
            entropy.copy_from_slice(&seed_bytes);

            use monero_seed::Language;
            let language = match helper.seed_language.as_str() {
                "Chinese" => Language::Chinese,
                "English" => Language::English,
                "Dutch" => Language::Dutch,
                "French" => Language::French,
                "Spanish" => Language::Spanish,
                "German" => Language::German,
                "Italian" => Language::Italian,
                "Portuguese" => Language::Portuguese,
                "Japanese" => Language::Japanese,
                "Russian" => Language::Russian,
                "Esperanto" => Language::Esperanto,
                "Lojban" => Language::Lojban,
                "DeprecatedEnglish" => Language::DeprecatedEnglish,
                _ => Language::English,
            };

            let seed = Seed::from_entropy(language, entropy)
                .ok_or_else(|| serde::de::Error::custom("invalid seed entropy"))?;

            let spend: [u8; 32] = *seed.entropy();
            let spend_scalar = Scalar::from_bytes_mod_order(spend);
            let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
            let view: [u8; 32] = Keccak256::digest(&spend).into();
            let view_scalar = Scalar::from_bytes_mod_order(view);
            let vp = ViewPair::new(spend_point, Zeroizing::new(view_scalar))
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;

            let reconstructed_checksum = compute_keys_checksum(&vp);
            if reconstructed_checksum != helper.keys_checksum {
                return Err(serde::de::Error::custom("keys checksum mismatch - possible corruption"));
            }

            (Some(Zeroizing::new(seed)), vp)
        } else {
            let spend_public = helper.view_only_spend_public
                .as_ref()
                .ok_or_else(|| serde::de::Error::custom("missing spend public key"))?;
            let view_private = helper.view_only_view_private
                .as_ref()
                .ok_or_else(|| serde::de::Error::custom("missing view private key"))?;

            let spend_point = curve25519_dalek::edwards::CompressedEdwardsY(*spend_public)
                .decompress()
                .ok_or_else(|| serde::de::Error::custom("invalid spend public key"))?;
            let view_scalar = Scalar::from_bytes_mod_order(**view_private);

            let vp = ViewPair::new(spend_point, Zeroizing::new(view_scalar))
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;

            let reconstructed_checksum = compute_keys_checksum(&vp);
            if reconstructed_checksum != helper.keys_checksum {
                return Err(serde::de::Error::custom("keys checksum mismatch - possible corruption"));
            }

            (None, vp)
        };

        // Reconstruct scanner and re-register subaddresses
        let mut scanner = Scanner::new(view_pair.clone());
        let mut registered_subaddresses = Vec::new();
        for (account, address) in helper.registered_subaddresses {
            if account == 0 && address == 0 {
                continue; // primary address is automatic
            }
            if let Some(idx) = SubaddressIndex::new(account, address) {
                scanner.register_subaddress(idx);
                registered_subaddresses.push(idx);
            } else {
                return Err(serde::de::Error::custom(format!(
                    "invalid subaddress: ({}, {})", account, address
                )));
            }
        }

        Ok(WalletState {
            magic: WALLET_MAGIC,
            version: helper.version,
            seed: seed_opt,
            view_pair,
            view_only_spend_public: helper.view_only_spend_public,
            view_only_view_private: helper.view_only_view_private,
            spend_key: helper.spend_key,
            network: helper.network,
            seed_language: helper.seed_language,
            outputs: helper.outputs,
            frozen_outputs: helper.frozen_outputs,
            spent_outputs: helper.spent_outputs,
            transactions: helper.transactions,
            tx_keys: helper.tx_keys,
            refresh_from_height: helper.refresh_from_height,
            current_scanned_height: helper.current_scanned_height,
            daemon_height: helper.daemon_height,
            is_syncing: false,
            block_hash_cache: helper.block_hash_cache,
            daemon_address: helper.daemon_address,
            is_connected: helper.is_connected,
            password_salt: helper.password_salt,
            password_hash: helper.password_hash,
            wallet_path: helper.wallet_path,
            is_closed: false,
            keys_checksum: helper.keys_checksum,
            auto_save_enabled: false,
            rpc_client: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
            health_check_handle: None,
            reconnection_policy: crate::rpc::ReconnectionPolicy::default(),
            reconnection_attempts: 0,
            connection_config: None,
            scanner,
            registered_subaddresses,
            sync_handle: None,
            sync_interval: std::time::Duration::from_secs(1),
            sync_progress_callback: None,
        })
    }
}

impl Default for WalletState {
    fn default() -> Self {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        Self::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "default_password",
            PathBuf::from("wallet.bin"),
            0,
        )
        .expect("Failed to create default WalletState")
    }
}
