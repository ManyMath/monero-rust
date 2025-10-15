//! Wallet state with encryption support.

use crate::crypto::{decrypt_wallet_data, encrypt_wallet_data, generate_nonce};
use crate::types::{KeyImage, SerializableOutput, Transaction, TxKey};
use crate::WalletError;
use curve25519_dalek::scalar::Scalar;
use monero_seed::Seed;
use monero_generators::biased_hash_to_point;
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

fn deserialize_subaddress_vec<'de, D>(deserializer: D) -> Result<Vec<SubaddressIndex>, D::Error>
where
    D: serde::Deserializer<'de>,
{
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
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        value: &Option<Zeroizing<[u8; 32]>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        super::serialize_view_key_option(value, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Zeroizing<[u8; 32]>>, D::Error>
    where
        D: Deserializer<'de>,
    {
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
        })
    }

    pub fn is_view_only(&self) -> bool {
        self.spend_key.is_none()
    }

    pub fn get_balance(&self) -> u64 {
        self.outputs
            .iter()
            .filter(|(ki, _)| !self.spent_outputs.contains(*ki))
            .map(|(_, output)| output.amount)
            .sum()
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
            .sum()
    }

    pub fn is_synced(&self) -> bool {
        self.is_connected && self.current_scanned_height >= self.daemon_height
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

    /// Scans a block for owned outputs. Returns the number of outputs found.
    pub async fn scan_block(
        &mut self,
        block: monero_rpc::ScannableBlock,
        block_height: u64,
    ) -> Result<usize, WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }

        let scan_result = self
            .scanner
            .scan(block)
            .map_err(|e| WalletError::Other(format!("scan error: {:?}", e)))?;

        let outputs = scan_result.not_additionally_locked();
        let count = outputs.len();

        for wallet_output in outputs {
            self.process_discovered_output(wallet_output, block_height)?;
        }

        self.current_scanned_height = block_height;
        Ok(count)
    }

    /// Fetches and scans a block by height.
    pub async fn scan_block_by_height(&mut self, height: u64) -> Result<usize, WalletError> {
        let rpc = self.get_rpc().await?;

        let height_usize: usize = height.try_into()
            .map_err(|_| WalletError::Other(format!("height {} exceeds platform limit", height)))?;

        let block = rpc
            .get_scannable_block_by_number(height_usize)
            .await
            .map_err(WalletError::RpcError)?;

        self.scan_block(block, height).await
    }

    fn process_discovered_output(
        &mut self,
        wallet_output: monero_wallet::WalletOutput,
        block_height: u64,
    ) -> Result<(), WalletError> {
        let key_image = self.compute_key_image(&wallet_output)?;
        let tx_hash = wallet_output.transaction();

        let subaddress_indices = wallet_output
            .subaddress()
            .map(|idx| (idx.account(), idx.address()))
            .unwrap_or((0, 0));

        let output = SerializableOutput {
            tx_hash,
            output_index: wallet_output.index_in_transaction(),
            amount: wallet_output.commitment().amount,
            key_image,
            subaddress_indices,
            height: block_height,
            unlocked: false,
            spent: false,
            frozen: false,
        };

        if let Some(existing) = self.outputs.get(&key_image) {
            if existing.tx_hash == tx_hash && existing.output_index == output.output_index {
                self.outputs.insert(key_image, output);
                return Ok(());
            }
            return Err(WalletError::Other(format!(
                "key image collision: {} already maps to {}:{}, cannot insert {}:{}",
                hex::encode(key_image),
                hex::encode(existing.tx_hash), existing.output_index,
                hex::encode(tx_hash), output.output_index
            )));
        }

        self.outputs.insert(key_image, output);
        Ok(())
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

    /// Checks if a reorganization occurred by comparing scanned height with daemon.
    pub async fn detect_reorganization(&mut self) -> Result<Option<u64>, WalletError> {
        let rpc = self.get_rpc().await?;
        let daemon_height = rpc.get_height().await.map_err(WalletError::RpcError)? as u64;

        self.daemon_height = daemon_height;

        if daemon_height < self.current_scanned_height {
            let fork_height = daemon_height.saturating_sub(10);
            return Ok(Some(fork_height));
        }

        Ok(None)
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
            daemon_address: Option<String>,
            is_connected: bool,
            password_salt: [u8; 32],
            password_hash: [u8; 32],
            wallet_path: PathBuf,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_state_creation() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let wallet_state = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test_password",
            PathBuf::from("test_wallet.bin"),
            0,
        )
        .unwrap();

        assert!(!wallet_state.is_view_only());
        assert!(!wallet_state.is_closed);
        assert_eq!(wallet_state.network, Network::Mainnet);
        assert_eq!(wallet_state.get_balance(), 0);
    }

    #[test]
    fn test_view_only_wallet() {
        use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint};

        let spend_scalar = Scalar::from_bytes_mod_order([1u8; 32]);
        let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
        let spend_public_key = spend_point.compress().to_bytes();
        let view_private_key = [2u8; 32];

        let wallet_state = WalletState::new_view_only(
            spend_public_key,
            view_private_key,
            Network::Testnet,
            "test_password",
            PathBuf::from("view_only.bin"),
            100,
        )
        .expect("Failed to create view-only wallet");

        assert!(wallet_state.is_view_only());
        assert_eq!(wallet_state.network, Network::Testnet);
        assert_eq!(wallet_state.refresh_from_height, 100);
    }

    #[test]
    fn test_balance_calculation() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let mut wallet_state = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test_password",
            PathBuf::from("test.bin"),
            0,
        )
        .unwrap();

        // Add some outputs
        let output1 = SerializableOutput {
            tx_hash: [1u8; 32],
            output_index: 0,
            amount: 1000000000000,
            key_image: [1u8; 32],
            subaddress_indices: (0, 0),
            height: 100,
            unlocked: true,
            spent: false,
            frozen: false,
        };

        let output2 = SerializableOutput {
            tx_hash: [2u8; 32],
            output_index: 0,
            amount: 2000000000000,
            key_image: [2u8; 32],
            subaddress_indices: (0, 1),
            height: 110,
            unlocked: true,
            spent: false,
            frozen: false,
        };

        wallet_state.outputs.insert([1u8; 32], output1);
        wallet_state.outputs.insert([2u8; 32], output2);

        assert_eq!(wallet_state.get_balance(), 3000000000000);

        // Mark one as spent
        wallet_state.spent_outputs.insert([1u8; 32]);
        assert_eq!(wallet_state.get_balance(), 2000000000000);
    }

    #[test]
    fn test_password_hashing() {
        let password = "my_secure_password_123!";
        let salt = WalletState::generate_salt();

        // Hash the password
        let hash1 = WalletState::hash_password(password, &salt).expect("Failed to hash password");
        let hash2 = WalletState::hash_password(password, &salt).expect("Failed to hash password");

        // Same password with same salt should produce same hash
        assert_eq!(hash1, hash2);

        // Different salt should produce different hash
        let different_salt = WalletState::generate_salt();
        let hash3 = WalletState::hash_password(password, &different_salt)
            .expect("Failed to hash password");
        assert_ne!(hash1, hash3);

        // Different password should produce different hash
        let hash4 = WalletState::hash_password("different_password", &salt)
            .expect("Failed to hash password");
        assert_ne!(hash1, hash4);
    }

    #[test]
    fn test_password_verification() {
        let password = "correct_password";
        let wrong_password = "wrong_password";
        let salt = WalletState::generate_salt();
        let hash = WalletState::hash_password(password, &salt).expect("Failed to hash password");

        // Correct password should verify
        assert!(WalletState::verify_password(password, &salt, &hash).is_ok());

        // Wrong password should fail
        assert!(WalletState::verify_password(wrong_password, &salt, &hash).is_err());

        // Wrong salt should fail
        let wrong_salt = WalletState::generate_salt();
        assert!(WalletState::verify_password(password, &wrong_salt, &hash).is_err());
    }

    #[test]
    fn test_wallet_stores_password_correctly() {
        use rand_core::OsRng;

        let password = "wallet_password_456";
        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let wallet_state = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            password,
            PathBuf::from("test.bin"),
            0,
        )
        .unwrap();

        // Verify the stored password hash
        assert!(
            WalletState::verify_password(password, &wallet_state.password_salt, &wallet_state.password_hash).is_ok()
        );

        // Wrong password should fail
        assert!(
            WalletState::verify_password("wrong", &wallet_state.password_salt, &wallet_state.password_hash).is_err()
        );
    }

    #[test]
    fn test_is_synced() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let mut wallet_state = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test_password",
            PathBuf::from("test.bin"),
            0,
        )
        .unwrap();

        // Not synced if not connected
        assert!(!wallet_state.is_synced());

        wallet_state.is_connected = true;
        wallet_state.daemon_height = 100;
        wallet_state.current_scanned_height = 50;

        // Not synced if behind
        assert!(!wallet_state.is_synced());

        wallet_state.current_scanned_height = 100;

        // Synced when caught up
        assert!(wallet_state.is_synced());
    }

    #[test]
    fn test_scanner_initialized() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let wallet = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test",
            PathBuf::from("test.bin"),
            0,
        ).unwrap();

        // No subaddresses registered yet (primary is automatic)
        assert_eq!(wallet.get_registered_subaddresses().len(), 0);
    }

    #[test]
    fn test_register_subaddress() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let mut wallet = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test",
            PathBuf::from("test.bin"),
            0,
        ).unwrap();

        assert_eq!(wallet.registered_subaddresses.len(), 0);

        wallet.register_subaddress(0, 1).unwrap();
        assert_eq!(wallet.registered_subaddresses.len(), 1);
        assert!(wallet.get_registered_subaddresses().contains(&(0, 1)));

        // registering again should be idempotent
        wallet.register_subaddress(0, 1).unwrap();
        assert_eq!(wallet.registered_subaddresses.len(), 1);
    }

    #[test]
    fn test_register_subaddress_range() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let mut wallet = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test",
            PathBuf::from("test.bin"),
            0,
        ).unwrap();

        let count = wallet.register_subaddress_range(0, 1, 5).unwrap();
        assert_eq!(count, 5);
        assert_eq!(wallet.registered_subaddresses.len(), 5);

        let registered = wallet.get_registered_subaddresses();
        for addr in 1..=5 {
            assert!(registered.contains(&(0, addr)));
        }
    }

    #[test]
    fn test_handle_reorganization() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let mut wallet = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test",
            PathBuf::from("test.bin"),
            0,
        ).unwrap();

        // add outputs at different heights
        for (i, height) in [100u64, 105, 110].iter().enumerate() {
            let output = SerializableOutput {
                tx_hash: [i as u8; 32],
                output_index: 0,
                amount: 1000000000000,
                key_image: [i as u8; 32],
                subaddress_indices: (0, 0),
                height: *height,
                unlocked: false,
                spent: false,
                frozen: false,
            };
            wallet.outputs.insert([i as u8; 32], output);
        }
        wallet.current_scanned_height = 110;
        assert_eq!(wallet.outputs.len(), 3);

        // reorg at height 105 removes outputs at 105 and above
        let removed = wallet.handle_reorganization(105);
        assert_eq!(removed, 2);
        assert_eq!(wallet.outputs.len(), 1);
        assert!(wallet.outputs.contains_key(&[0u8; 32]));
        assert_eq!(wallet.current_scanned_height, 104);
    }

    #[test]
    fn test_reorg_cleans_hashsets() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let mut wallet = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test",
            PathBuf::from("test.bin"),
            0,
        ).unwrap();

        let output = SerializableOutput {
            tx_hash: [1u8; 32],
            output_index: 0,
            amount: 1000000000000,
            key_image: [1u8; 32],
            subaddress_indices: (0, 0),
            height: 110,
            unlocked: false,
            spent: false,
            frozen: false,
        };
        wallet.outputs.insert([1u8; 32], output);
        wallet.spent_outputs.insert([1u8; 32]);
        wallet.frozen_outputs.insert([1u8; 32]);

        wallet.handle_reorganization(105);

        assert!(wallet.spent_outputs.is_empty());
        assert!(wallet.frozen_outputs.is_empty());
    }

    #[test]
    fn test_subaddresses_persist() {
        use rand_core::OsRng;
        use tempfile::NamedTempFile;

        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let mut wallet = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test",
            path.clone(),
            0,
        ).unwrap();

        wallet.register_subaddress(0, 1).unwrap();
        wallet.register_subaddress(0, 2).unwrap();
        wallet.register_subaddress(1, 0).unwrap();
        assert_eq!(wallet.registered_subaddresses.len(), 3);

        wallet.save("test").unwrap();

        let loaded = WalletState::load_from_file(&path, "test").unwrap();
        assert_eq!(loaded.registered_subaddresses.len(), 3);

        let registered = loaded.get_registered_subaddresses();
        assert!(registered.contains(&(0, 1)));
        assert!(registered.contains(&(0, 2)));
        assert!(registered.contains(&(1, 0)));
    }
}
