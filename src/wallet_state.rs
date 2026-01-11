//! Wallet state management and persistence.
//!
//! This module defines the core `WalletState` structure that maintains all
//! wallet data including keys, outputs, transactions, and synchronization state.
//! The state is designed to be serializable for persistence to disk with
//! encryption.

use crate::crypto::{decrypt_wallet_data, encrypt_wallet_data, generate_nonce};
use crate::types::{KeyImage, SerializableOutput, Transaction, TxKey};
use crate::WalletError;
use curve25519_dalek::scalar::Scalar;
use monero_seed::Seed;
use monero_wallet::{address::{Network, SubaddressIndex}, rpc::Rpc, ViewPair, Scanner};
use monero_generators::biased_hash_to_point;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;
use std::ops::Deref;

/// Comprehensive wallet state containing all data needed for wallet operations.
///
/// This structure maintains the complete state of a Monero wallet, including:
/// - Cryptographic keys and seed (with secure memory handling)
/// - Transaction and output tracking
/// - Synchronization state with the blockchain
/// - Connection status and configuration
///
/// # Security Considerations
///
/// Sensitive fields (seed, spend_key) use `Zeroizing` to ensure they are
/// cleared from memory when dropped. When serialized, the entire structure
/// should be encrypted before writing to disk.
///
/// # Serialization
///
/// The state implements `Serialize` and custom `Deserialize` for persistence.
/// ViewPair is automatically reconstructed from the seed after deserialization.
/// Fields marked with `#[serde(skip)]` are runtime-only and not persisted.
#[derive(Serialize)]
pub struct WalletState {
    // ========================================================================
    // VERSIONING - Format version for forward compatibility
    // ========================================================================
    /// Serialization format version
    ///
    /// This allows detecting and handling different wallet file formats.
    /// Current version: 1
    /// Increment when making breaking changes to serialization format.
    #[serde(default = "default_version")]
    pub version: u32,

    // ========================================================================
    // IDENTITY - Cryptographic keys and network configuration
    // ========================================================================
    /// Mnemonic seed (25 words) - SENSITIVE
    ///
    /// Stored as raw entropy bytes in a Zeroizing wrapper.
    /// None for view-only wallets (no spend capability).
    /// Custom serialization is used to handle the Zeroizing wrapper.
    #[serde(serialize_with = "serialize_seed_option")]
    pub seed: Option<Zeroizing<Seed>>,

    /// View pair containing public spend key and private view key
    ///
    /// Used for address generation and output scanning.
    /// The ViewPair itself contains a Zeroizing wrapper for the view key.
    /// Not serialized - automatically reconstructed from seed on deserialization.
    #[serde(skip)]
    pub view_pair: ViewPair,

    /// Public spend key (compressed Edwards point) - for view-only wallets
    ///
    /// Only present (Some) for view-only wallets where seed is None.
    /// Used to reconstruct ViewPair during deserialization.
    pub view_only_spend_public: Option<[u8; 32]>,

    /// Private view key (scalar) - SENSITIVE, for view-only wallets
    ///
    /// Only present (Some) for view-only wallets where seed is None.
    /// Used to reconstruct ViewPair during deserialization.
    #[serde(
        serialize_with = "serialize_view_key_option",
        deserialize_with = "deserialize_view_key_option"
    )]
    pub view_only_view_private: Option<Zeroizing<[u8; 32]>>,

    /// Private spend key - SENSITIVE
    ///
    /// None for view-only wallets (which cannot spend funds).
    /// Wrapped in double Zeroizing for extra security.
    #[serde(
        serialize_with = "serialize_spend_key",
        deserialize_with = "deserialize_spend_key"
    )]
    pub spend_key: Option<Zeroizing<Scalar>>,

    /// Network type (Mainnet, Testnet, or Stagenet)
    #[serde(
        serialize_with = "serialize_network",
        deserialize_with = "deserialize_network"
    )]
    pub network: Network,

    /// Language of the mnemonic seed
    ///
    /// Stored to allow returning the seed in the same language it was created.
    pub seed_language: String,

    // ========================================================================
    // OUTPUTS - UTXO tracking and management
    // ========================================================================
    /// All outputs owned by this wallet, indexed by key image
    ///
    /// Includes both spent and unspent outputs for historical tracking.
    pub outputs: HashMap<KeyImage, SerializableOutput>,

    /// Set of manually frozen (locked) outputs
    ///
    /// Frozen outputs are excluded from input selection during transaction
    /// creation, preventing them from being spent until thawed.
    pub frozen_outputs: HashSet<KeyImage>,

    /// Set of spent output key images
    ///
    /// Tracked separately for efficient balance calculation and to detect
    /// when outputs have been spent by other wallet instances.
    pub spent_outputs: HashSet<KeyImage>,

    // ========================================================================
    // TRANSACTIONS - Transaction history and keys
    // ========================================================================
    /// All transactions involving this wallet, indexed by txid
    ///
    /// Includes both incoming and outgoing transactions.
    pub transactions: HashMap<[u8; 32], Transaction>,

    /// Transaction private keys for outgoing transactions
    ///
    /// Used to generate payment proofs showing that a transaction
    /// was sent to a specific address.
    pub tx_keys: HashMap<[u8; 32], TxKey>,

    // ========================================================================
    // SYNC STATE - Blockchain synchronization tracking
    // ========================================================================
    /// Block height to start scanning from (restore height)
    ///
    /// Set to wallet creation height or earlier to avoid scanning
    /// the entire blockchain. Can be adjusted with setRefreshFromBlockHeight().
    pub refresh_from_height: u64,

    /// Current block height the wallet has scanned up to
    ///
    /// Updated as the wallet scans new blocks. Used to track sync progress.
    pub current_scanned_height: u64,

    /// Most recent daemon block height
    ///
    /// Cached from the last daemon query. Used to determine if wallet
    /// is fully synchronized and to calculate confirmations.
    pub daemon_height: u64,

    /// Whether the wallet is currently syncing
    ///
    /// True when the sync loop is running, false otherwise.
    /// This is runtime state and is not serialized - always defaults to false on load.
    #[serde(skip)]
    pub is_syncing: bool,

    // ========================================================================
    // CONNECTION - Daemon connection state
    // ========================================================================
    /// Daemon RPC address (e.g., "http://node.example.com:18081")
    ///
    /// None if not connected to any daemon.
    pub daemon_address: Option<String>,

    /// Whether currently connected to the daemon
    ///
    /// Updated based on RPC health checks. If connection is lost,
    /// this is set to false and reconnection is attempted.
    pub is_connected: bool,

    // ========================================================================
    // CONFIGURATION - Wallet metadata and settings
    // ========================================================================
    /// Password salt for Argon2 key derivation
    ///
    /// Random 32 bytes generated on wallet creation.
    /// Used with Argon2id to derive both the password verification hash
    /// and the encryption key.
    pub password_salt: [u8; 32],

    /// Hash of the wallet password for verification
    ///
    /// Derived using Argon2id with the salt above.
    /// Used only to verify the correct password on wallet open.
    /// NOT used for encryption (password is stretched with Argon2 for that).
    pub password_hash: [u8; 32],

    /// File path where this wallet is stored
    ///
    /// Used for save operations and displayed via getPath().
    pub wallet_path: PathBuf,

    /// Whether the wallet has been closed
    ///
    /// Set to true when close() is called. Operations should check this
    /// and return an error if the wallet is closed.
    pub is_closed: bool,

    /// Checksum of ViewPair public keys for integrity verification
    ///
    /// This is a SHA256 hash of (public_spend_key || public_view_key) used to verify
    /// that the reconstructed ViewPair matches the original keys. Protects against
    /// seed corruption or tampering during storage.
    #[serde(rename = "keys_checksum")]
    pub keys_checksum: [u8; 32],

    // ========================================================================
    // RUNTIME STATE - Not serialized (reconstructed on load)
    // ========================================================================
    /// Whether auto-save is enabled
    ///
    /// Controls the periodic background save task.
    /// Not persisted - must be re-enabled after loading.
    #[serde(skip)]
    pub auto_save_enabled: bool,

    /// RPC client for daemon communication
    ///
    /// Wrapped in Arc<RwLock> for thread-safe async access.
    /// None when not connected. This is runtime-only and not serialized.
    /// Connection must be re-established after loading the wallet.
    #[serde(skip)]
    pub rpc_client: std::sync::Arc<tokio::sync::RwLock<Option<monero_simple_request_rpc::SimpleRequestRpc>>>,

    /// Health check task handle
    ///
    /// Handle to the background task that periodically checks daemon connectivity.
    /// None when health check is not running. Not serialized.
    #[serde(skip)]
    pub health_check_handle: Option<tokio::task::JoinHandle<()>>,

    /// Reconnection policy configuration
    ///
    /// Defines how automatic reconnection attempts should be made.
    /// Runtime-only configuration, not persisted.
    #[serde(skip)]
    pub reconnection_policy: crate::rpc::ReconnectionPolicy,

    /// Reconnection attempt counter
    ///
    /// Tracks the current number of consecutive reconnection attempts.
    /// Reset to 0 on successful connection. Not persisted.
    #[serde(skip)]
    pub reconnection_attempts: u32,

    /// Stored connection configuration for reconnection
    ///
    /// Keeps the full ConnectionConfig (including credentials) to enable
    /// automatic reconnection with the same settings. Not persisted for security.
    /// This allows the health check task to reconnect with authentication.
    #[serde(skip)]
    pub connection_config: Option<crate::rpc::ConnectionConfig>,

    // ========================================================================
    // SCANNING - Output detection and blockchain scanning
    // ========================================================================
    /// Scanner for detecting owned outputs in blocks
    ///
    /// Uses the ViewPair to check if outputs belong to this wallet.
    /// Reconstructed from ViewPair on deserialization (not serialized).
    #[serde(skip)]
    pub scanner: Scanner,

    /// List of registered subaddresses for scanning
    ///
    /// The scanner will check blocks for outputs sent to these addresses.
    /// Primary address (0, 0) is always registered automatically.
    /// Custom serialization stores as Vec<(u32, u32)> for compatibility.
    #[serde(
        serialize_with = "serialize_subaddress_vec",
        deserialize_with = "deserialize_subaddress_vec"
    )]
    pub registered_subaddresses: Vec<SubaddressIndex>,
}

/// Current wallet serialization format version
const WALLET_VERSION: u32 = 1;

/// Default version for deserialization (for backwards compatibility)
fn default_version() -> u32 {
    WALLET_VERSION
}

// Helper function to compute ViewPair keys checksum for integrity verification
fn compute_keys_checksum(view_pair: &ViewPair) -> [u8; 32] {
    use sha3::{Digest, Keccak256};

    let spend_bytes = view_pair.spend().compress().to_bytes();
    let view_bytes = view_pair.view().compress().to_bytes();

    let mut hasher = Keccak256::new();
    hasher.update(&spend_bytes);
    hasher.update(&view_bytes);
    hasher.finalize().into()
}

// Custom serialization for Vec<SubaddressIndex>
// SubaddressIndex doesn't implement Serialize, so we convert to Vec<(u32, u32)>
fn serialize_subaddress_vec<S>(
    vec: &Vec<SubaddressIndex>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let pairs: Vec<(u32, u32)> = vec
        .iter()
        .map(|idx| (idx.account(), idx.address()))
        .collect();
    pairs.serialize(serializer)
}

fn deserialize_subaddress_vec<'de, D>(
    deserializer: D,
) -> Result<Vec<SubaddressIndex>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let pairs: Vec<(u32, u32)> = Vec::deserialize(deserializer)?;
    let mut result = Vec::new();
    for (account, address) in pairs {
        if let Some(idx) = SubaddressIndex::new(account, address) {
            result.push(idx);
        } else {
            return Err(serde::de::Error::custom(format!(
                "Invalid subaddress index: ({}, {})",
                account, address
            )));
        }
    }
    Ok(result)
}

// Custom serialization for Zeroizing<Seed>
//
// SECURITY: We serialize the raw entropy bytes to avoid creating unprotected
// String copies of the mnemonic. The entropy is already in a Zeroizing wrapper.
fn serialize_seed_option<S>(
    seed: &Option<Zeroizing<Seed>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match seed {
        Some(s) => {
            // Serialize the 32-byte entropy directly
            // The Zeroizing wrapper ensures the bytes are protected
            let entropy_bytes: &[u8] = &*s.entropy();
            serializer.serialize_some(entropy_bytes)
        }
        None => serializer.serialize_none(),
    }
}

// Custom serialization for view-only wallet's private view key
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

// Module for serializing Option<Zeroizing<[u8; 32]>> with default support
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

// Custom serialization for Network
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

// Custom serialization for Option<Zeroizing<Scalar>>
//
// SECURITY: We must avoid creating unprotected copies of the spend key bytes.
fn serialize_spend_key<S>(
    spend_key: &Option<Zeroizing<Scalar>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match spend_key {
        Some(key) => {
            // Get bytes in a Zeroizing wrapper to ensure they're cleared
            let bytes = Zeroizing::new(key.to_bytes());
            serializer.serialize_some(&*bytes)
        }
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
    // ========================================================================
    // PASSWORD UTILITIES - Argon2id hashing and verification
    // ========================================================================

    /// Hash a password using Argon2id with the given salt.
    ///
    /// Uses recommended parameters:
    /// - Algorithm: Argon2id
    /// - Memory: 64 MB
    /// - Iterations: 3
    /// - Parallelism: 4
    /// - Output: 32 bytes
    ///
    /// # Arguments
    /// * `password` - The password to hash
    /// * `salt` - 32-byte salt for key derivation
    ///
    /// # Returns
    /// 32-byte password hash
    pub fn hash_password(password: &str, salt: &[u8; 32]) -> Result<[u8; 32], String> {
        use argon2::{
            password_hash::{PasswordHasher, SaltString},
            Algorithm, Argon2, ParamsBuilder, Version,
        };

        // Configure Argon2id with recommended parameters
        let mut builder = ParamsBuilder::new();
        builder.m_cost(65536); // 64 MB
        builder.t_cost(3); // 3 iterations
        builder.p_cost(4); // 4 parallelism
        builder.output_len(32); // 32 byte output
        let params = builder.build()
            .map_err(|e| format!("Failed to build Argon2 parameters: {}", e))?;

        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        // Convert salt bytes to SaltString format
        let salt_string = SaltString::encode_b64(salt)
            .map_err(|e| format!("Failed to encode salt: {}", e))?;

        // Hash the password
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt_string)
            .map_err(|e| format!("Failed to hash password: {}", e))?;

        // Extract the hash bytes
        let hash_bytes = password_hash
            .hash
            .ok_or_else(|| "Password hash missing".to_string())?;

        if hash_bytes.len() != 32 {
            return Err(format!(
                "Unexpected hash length: expected 32, got {}",
                hash_bytes.len()
            ));
        }

        let mut result = [0u8; 32];
        result.copy_from_slice(hash_bytes.as_bytes());
        Ok(result)
    }

    /// Generate a random salt for password hashing.
    ///
    /// # Returns
    /// 32 bytes of cryptographically secure random data
    pub fn generate_salt() -> [u8; 32] {
        use rand_core::RngCore;
        let mut salt = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut salt);
        salt
    }

    /// Verify a password against a stored hash and salt.
    ///
    /// # Arguments
    /// * `password` - The password to verify
    /// * `salt` - The salt used during hashing
    /// * `expected_hash` - The expected password hash
    ///
    /// # Returns
    /// `Ok(())` if password is correct, `Err` otherwise
    pub fn verify_password(
        password: &str,
        salt: &[u8; 32],
        expected_hash: &[u8; 32],
    ) -> Result<(), String> {
        let computed_hash = Self::hash_password(password, salt)?;

        // Constant-time comparison to prevent timing attacks
        use subtle::ConstantTimeEq;
        if computed_hash.ct_eq(expected_hash).into() {
            Ok(())
        } else {
            Err("Invalid password".to_string())
        }
    }

    // ========================================================================
    // CONSTRUCTORS
    // ========================================================================

    /// Creates a new wallet state from a seed and network.
    ///
    /// # Arguments
    ///
    /// * `seed` - Mnemonic seed for key derivation
    /// * `seed_language` - Language of the mnemonic
    /// * `network` - Network type (Mainnet, Testnet, Stagenet)
    /// * `password` - Password for wallet encryption and verification
    /// * `wallet_path` - Path where wallet will be saved
    /// * `refresh_from_height` - Block height to start scanning from
    ///
    /// # Returns
    ///
    /// A new `WalletState` initialized with the provided parameters.
    pub fn new(
        seed: Seed,
        seed_language: String,
        network: Network,
        password: &str,
        wallet_path: PathBuf,
        refresh_from_height: u64,
    ) -> Result<Self, String> {
        // Generate salt and hash password
        let password_salt = Self::generate_salt();
        let password_hash = Self::hash_password(password, &password_salt)?;
        use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint};
        use sha3::{Digest, Keccak256};

        // Derive keys from seed
        let spend: [u8; 32] = *seed.entropy();
        let spend_scalar = Scalar::from_bytes_mod_order(spend);
        let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);

        let view_pair =
            ViewPair::new(spend_point, Zeroizing::new(view_scalar)).map_err(|e| e.to_string())?;

        // Compute keys checksum for integrity verification
        let keys_checksum = compute_keys_checksum(&view_pair);

        // Initialize scanner
        // Note: Scanner::new() automatically handles the primary address (0, 0)
        // Only actual subaddresses (non-(0,0)) need to be registered
        let scanner = Scanner::new(view_pair.clone());
        let registered_subaddresses = Vec::new();

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
            registered_subaddresses,
        })
    }

    /// Creates a view-only wallet state (no spend key).
    ///
    /// View-only wallets can track balances and transactions but cannot
    /// spend funds.
    ///
    /// # Arguments
    /// * `spend_public_key` - Public spend key (32 bytes, compressed Edwards point)
    /// * `view_private_key` - Private view key (32 bytes, scalar)
    /// * `password` - Password for wallet encryption and verification
    pub fn new_view_only(
        spend_public_key: [u8; 32],
        view_private_key: [u8; 32],
        network: Network,
        password: &str,
        wallet_path: PathBuf,
        refresh_from_height: u64,
    ) -> Result<Self, String> {
        // Generate salt and hash password
        let password_salt = Self::generate_salt();
        let password_hash = Self::hash_password(password, &password_salt)?;

        use curve25519_dalek::edwards::CompressedEdwardsY;

        // Decompress the spend public key
        let spend_point = CompressedEdwardsY(spend_public_key)
            .decompress()
            .ok_or_else(|| "Invalid spend public key".to_string())?;

        // Construct view scalar
        let view_scalar = Scalar::from_bytes_mod_order(view_private_key);

        // Create ViewPair
        let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar))
            .map_err(|e| e.to_string())?;

        // Compute keys checksum for integrity verification
        let keys_checksum = compute_keys_checksum(&view_pair);

        // Initialize scanner
        // Note: Scanner::new() automatically handles the primary address (0, 0)
        // Only actual subaddresses (non-(0,0)) need to be registered
        let scanner = Scanner::new(view_pair.clone());
        let registered_subaddresses = Vec::new();

        Ok(Self {
            version: WALLET_VERSION,
            seed: None, // No seed for view-only wallets
            view_pair,
            view_only_spend_public: Some(spend_public_key),
            view_only_view_private: Some(Zeroizing::new(view_private_key)),
            spend_key: None, // No spend key for view-only
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
            registered_subaddresses,
        })
    }

    /// Checks if this is a view-only wallet.
    pub fn is_view_only(&self) -> bool {
        self.spend_key.is_none()
    }

    /// Calculates the total balance (all unspent outputs).
    ///
    /// # Returns
    ///
    /// Total balance in atomic units (piconeros).
    pub fn get_balance(&self) -> u64 {
        self.outputs
            .iter()
            .filter(|(ki, _)| !self.spent_outputs.contains(*ki))
            .map(|(_, output)| output.amount)
            .sum()
    }

    /// Calculates the unlocked balance (spendable outputs only).
    ///
    /// Outputs must be at least 10 blocks old to be considered unlocked.
    /// Frozen outputs are excluded as they cannot be spent.
    ///
    /// # Returns
    ///
    /// Unlocked balance in atomic units (piconeros).
    pub fn get_unlocked_balance(&self) -> u64 {
        const LOCK_BLOCKS: u64 = 10;

        self.outputs
            .iter()
            .filter(|(ki, output)| {
                // Must not be spent
                !self.spent_outputs.contains(*ki)
                    // Must not be frozen (manually locked by user)
                    // NOTE: frozen_outputs HashSet is the canonical source of truth
                    && !self.frozen_outputs.contains(*ki)
                    // Must have at least 10 confirmations
                    && self.daemon_height >= output.height.saturating_add(LOCK_BLOCKS)
            })
            .map(|(_, output)| output.amount)
            .sum()
    }

    /// Checks if the wallet is fully synchronized with the daemon.
    pub fn is_synced(&self) -> bool {
        self.is_connected && self.current_scanned_height >= self.daemon_height
    }

    // ========================================================================
    // RPC CONNECTION MANAGEMENT
    // ========================================================================

    /// Connects to a Monero daemon for blockchain operations.
    ///
    /// This establishes an RPC connection to the specified daemon and starts
    /// background health monitoring. If already connected, the existing connection
    /// is closed first.
    ///
    /// # Arguments
    /// * `config` - Connection configuration including daemon address, credentials, etc.
    ///
    /// # Returns
    /// `Ok(())` if connection is successful, `Err(WalletError)` otherwise
    ///
    /// # Example
    /// ```no_run
    /// use monero_rust::{rpc::ConnectionConfig, WalletState};
    ///
    /// # async fn example(wallet: &mut WalletState) -> Result<(), Box<dyn std::error::Error>> {
    /// let config = ConnectionConfig::new("http://node.example.com:18081".to_string())
    ///     .with_trusted(false)
    ///     .with_credentials("user".to_string(), "pass".to_string());
    ///
    /// wallet.connect(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(&mut self, config: crate::rpc::ConnectionConfig) -> Result<(), WalletError> {
        use monero_simple_request_rpc::SimpleRequestRpc;

        // Idempotency check: if already connected to the same daemon, do nothing
        if self.is_connected && self.daemon_address.as_ref() == Some(&config.daemon_address) {
            return Ok(());
        }

        // Disconnect existing connection if connected to a different daemon
        if self.is_connected {
            self.disconnect().await;
        }

        // Build the URL with embedded credentials if provided
        // This returns a Zeroizing<String> for security
        let url = config.build_url();

        // Create the RPC client with the specified timeout
        // Always use custom timeout for consistency (no magic number comparison)
        let rpc = SimpleRequestRpc::with_custom_timeout(url.as_str().to_string(), config.timeout).await
            .map_err(|e| WalletError::RpcError(e))?;

        // Perform initial health check to verify connection
        let daemon_height = rpc.get_height().await
            .map_err(|e| WalletError::RpcError(e))?;

        // Update wallet state
        self.daemon_address = Some(config.daemon_address.clone());
        self.daemon_height = daemon_height as u64;
        self.is_connected = true;
        self.reconnection_attempts = 0;

        // Store the connection config for reconnection (includes credentials)
        self.connection_config = Some(config);

        // Store the RPC client
        {
            let mut rpc_client = self.rpc_client.write().await;
            *rpc_client = Some(rpc);
        }

        // Start health check background task
        self.start_health_check().await;

        Ok(())
    }

    /// Disconnects from the daemon and stops all background tasks.
    ///
    /// This cleanly shuts down the RPC connection and health check task.
    /// Safe to call even if not connected.
    pub async fn disconnect(&mut self) {
        // Stop health check task and wait for it to complete
        self.stop_health_check().await;

        // Clear RPC client
        {
            let mut rpc_client = self.rpc_client.write().await;
            *rpc_client = None;
        }

        // Clear connection config (including credentials)
        self.connection_config = None;

        // Update state
        self.is_connected = false;
        self.reconnection_attempts = 0;
    }

    /// Returns whether the wallet is currently connected to a daemon.
    ///
    /// # Returns
    /// `true` if connected, `false` otherwise
    pub fn is_connected_to_daemon(&self) -> bool {
        self.is_connected
    }

    // ==================== BASIC GETTERS ====================

    /// Returns the wallet's mnemonic seed as a string.
    ///
    /// # Returns
    /// - `Some(String)` containing the mnemonic seed for normal wallets
    /// - `None` for view-only wallets (no seed available)
    ///
    /// # Security Note
    /// The returned String is NOT zeroized. Callers should avoid storing this value
    /// long-term or should use a future secure API that returns Zeroizing types.
    pub fn get_seed(&self) -> Option<String> {
        self.seed.as_ref().map(|seed| {
            // Seed::to_string() returns Zeroizing<String>
            // We extract the inner String (this creates a copy, not zeroized)
            let zeroized_string = seed.to_string();
            (*zeroized_string).clone()
        })
    }

    /// Returns the language of the wallet's mnemonic seed.
    ///
    /// # Returns
    /// The seed language as a string (e.g., "English", "Spanish", etc.)
    pub fn get_seed_language(&self) -> &str {
        &self.seed_language
    }

    /// Returns the wallet's private spend key as a hex string.
    ///
    /// # Returns
    /// - `Some(String)` containing the hex-encoded private spend key for normal wallets
    /// - `None` for view-only wallets (no spend key available)
    ///
    /// # Security Note
    /// Intermediate values are zeroized during computation, but the returned String
    /// is NOT zeroized. Callers should avoid storing this value long-term or should
    /// use a future secure API that returns Zeroizing types.
    pub fn get_private_spend_key(&self) -> Option<String> {
        self.spend_key.as_ref().map(|key| {
            // Use Zeroizing for intermediate byte array
            let bytes = Zeroizing::new(key.to_bytes());
            hex::encode(&*bytes)
        })
    }

    /// Returns the wallet's private view key as a hex string.
    ///
    /// # Returns
    /// The hex-encoded private view key (available for all wallet types)
    ///
    /// # Implementation Notes
    ///
    /// For normal wallets: Recomputes the view key from the seed using Keccak256
    /// as per Monero's key derivation spec (view_key = Keccak256(seed_entropy)).
    /// This recomputation is necessary because ViewPair's private view scalar is
    /// not directly accessible (pub(crate) field).
    ///
    /// For view-only wallets: Returns the stored private view key that was
    /// provided during wallet creation.
    ///
    /// # Security Note
    /// Intermediate values are zeroized during computation, but the returned String
    /// is NOT zeroized. Callers should avoid storing this value long-term or should
    /// use a future secure API that returns Zeroizing types.
    ///
    /// # Panics
    /// Panics if the wallet is in an invalid state (no seed AND no view key).
    /// This should never happen if the wallet was constructed correctly.
    pub fn get_private_view_key(&self) -> String {
        use sha3::{Digest, Keccak256};

        // For regular wallets, compute from seed
        if let Some(seed) = &self.seed {
            // Use Zeroizing for the computed view key bytes
            let view_bytes: [u8; 32] = Keccak256::digest(seed.entropy()).into();
            let view = Zeroizing::new(view_bytes);
            hex::encode(&*view)
        }
        // For view-only wallets, use stored private view key
        else if let Some(view_private) = &self.view_only_view_private {
            hex::encode(&**view_private)
        }
        // This should never happen - panic to catch bugs early
        else {
            panic!("CRITICAL: WalletState invariant violated - no seed or view key found. This indicates a bug in wallet construction.");
        }
    }

    /// Returns the wallet's public spend key as a hex string.
    ///
    /// # Returns
    /// The hex-encoded public spend key (available for all wallet types)
    pub fn get_public_spend_key(&self) -> String {
        hex::encode(self.view_pair.spend().compress().to_bytes())
    }

    /// Returns the wallet's public view key as a hex string.
    ///
    /// # Returns
    /// The hex-encoded public view key (available for all wallet types)
    pub fn get_public_view_key(&self) -> String {
        // ViewPair::view() returns the public view key (EdwardsPoint)
        hex::encode(self.view_pair.view().compress().to_bytes())
    }

    /// Returns the filesystem path where this wallet is stored.
    ///
    /// # Returns
    /// A reference to the wallet's file path
    pub fn get_path(&self) -> &std::path::Path {
        &self.wallet_path
    }

    // ==================== END BASIC GETTERS ====================

    // ========================================================================
    // BLOCKCHAIN SCANNING - Output detection and block processing
    // ========================================================================

    /// Scans a single block for owned outputs.
    ///
    /// This is the low-level scanning method that processes a ScannableBlock directly.
    /// For most use cases, prefer using `scan_block_by_height()` which handles fetching.
    ///
    /// # Arguments
    /// * `block` - The scannable block to process
    /// * `block_height` - The height/number of the block being scanned
    ///
    /// # Returns
    /// * Number of outputs found in this block
    ///
    /// # Errors
    /// * `WalletError::WalletClosed` - Wallet is closed
    /// * `WalletError::Other` - Scanner error occurred
    ///
    /// # Example
    /// ```no_run
    /// # use monero_rust::{WalletState};
    /// # async fn example(wallet: &mut WalletState, block: monero_rpc::ScannableBlock) {
    /// let count = wallet.scan_block(block, 1234567).await.unwrap();
    /// println!("Found {} outputs in this block", count);
    /// # }
    /// ```
    pub async fn scan_block(
        &mut self,
        block: monero_rpc::ScannableBlock,
        block_height: u64,
    ) -> Result<usize, WalletError> {
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }

        // Scan the block for owned outputs
        let scan_result = self
            .scanner
            .scan(block)
            .map_err(|e| WalletError::Other(format!("Scan error: {:?}", e)))?;

        // Extract outputs that are not additionally timelocked
        // (most outputs fall into this category)
        let outputs = scan_result.not_additionally_locked();

        let count = outputs.len();

        // Process each discovered output with the correct block height
        for wallet_output in outputs {
            self.process_discovered_output(wallet_output, block_height)?;
        }

        // CRITICAL: Update scanned height after successful scan
        // Without this, wallet appears perpetually unsynced and reorg detection breaks
        self.current_scanned_height = block_height;

        Ok(count)
    }

    /// Fetches and scans a block by height.
    ///
    /// This is the high-level convenience method that fetches a block from the daemon
    /// and scans it for owned outputs in one call.
    ///
    /// # Arguments
    /// * `height` - The block height/number to fetch and scan
    ///
    /// # Returns
    /// * Number of outputs found in this block
    ///
    /// # Errors
    /// * `WalletError::NotConnected` - Not connected to daemon
    /// * `WalletError::WalletClosed` - Wallet is closed
    /// * `WalletError::RpcError` - Failed to fetch block from daemon
    /// * `WalletError::Other` - Scanner error occurred
    ///
    /// # Example
    /// ```no_run
    /// # use monero_rust::{WalletState, rpc::ConnectionConfig};
    /// # async fn example(wallet: &mut WalletState) -> Result<(), Box<dyn std::error::Error>> {
    /// let config = ConnectionConfig::new("http://node.example.com:18081".to_string());
    /// wallet.connect(config).await?;
    /// let count = wallet.scan_block_by_height(1234567).await?;
    /// println!("Found {} outputs in block 1234567", count);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn scan_block_by_height(&mut self, height: u64) -> Result<usize, WalletError> {
        // Get RPC client
        let rpc = self.get_rpc().await?;

        // Convert height to usize with bounds checking (important for 32-bit systems)
        let height_usize = height.try_into()
            .map_err(|_| WalletError::Other(
                format!("Block height {} exceeds platform limit (usize::MAX)", height)
            ))?;

        // Fetch the block from daemon
        let block = rpc
            .get_scannable_block_by_number(height_usize)
            .await
            .map_err(|e| WalletError::RpcError(e))?;

        // Scan the fetched block
        self.scan_block(block, height).await
    }

    /// Processes a single discovered output and adds it to wallet state.
    ///
    /// Converts monero-oxide WalletOutput to our SerializableOutput format
    /// and stores it indexed by key image.
    ///
    /// # Arguments
    /// * `wallet_output` - The discovered output from scanner
    /// * `block_height` - The height of the block containing this output
    ///
    /// # Returns
    /// * `Ok(())` on success
    ///
    /// # Errors
    /// * `WalletError::Other` - If key image computation fails
    fn process_discovered_output(
        &mut self,
        wallet_output: monero_wallet::WalletOutput,
        block_height: u64,
    ) -> Result<(), WalletError> {
        // Extract key image (for indexing and spent detection)
        let key_image = self.compute_key_image(&wallet_output)?;

        // Convert to our serializable format
        // Note: wallet_output.transaction() returns [u8; 32] (Copy type)
        let tx_hash = wallet_output.transaction();

        // Extract subaddress - if None, this is the primary address (0, 0)
        // The Scanner returns None for primary address outputs
        let subaddress_indices = wallet_output.subaddress()
            .map(|idx| (idx.account(), idx.address()))
            .unwrap_or((0, 0));  // Primary address when Scanner returns None

        let serializable_output = SerializableOutput {
            tx_hash,
            output_index: wallet_output.index_in_transaction(),
            amount: wallet_output.commitment().amount,
            key_image,
            subaddress_indices,
            height: block_height,
            unlocked: false, // Will be updated when 10 blocks pass
            spent: false,
            frozen: false,
        };

        // Check for duplicates before inserting
        if let Some(existing) = self.outputs.get(&key_image) {
            // Check if it's the exact same output (rescanning after reorg)
    /// Computes the cryptographically correct key image for an output.
                // Same output being rescanned - this is OK, just update it
                self.outputs.insert(key_image, serializable_output);
                return Ok(());
            }

            // Different output with same key image - this is a critical error
            // This indicates either:
            // 1. Broken key image computation (our current placeholder)
            // 2. Cryptographic collision (extremely unlikely)
            // 3. Data corruption
            return Err(WalletError::Other(format!(
                "Key image collision detected! Key image {} already exists for output {}:{}, \
                 cannot insert new output {}:{}. This indicates broken key image computation.",
                hex::encode(key_image),
                hex::encode(existing.tx_hash), existing.output_index,
                hex::encode(tx_hash), serializable_output.output_index
            )));
        }

        // Store in outputs map (indexed by key image)
        self.outputs.insert(key_image, serializable_output);

        Ok(())
    }

    /// Computes the key image for an output.
    ///
    /// Key images are used to:
    /// 1. Index outputs uniquely
    /// 2. Detect when an output has been spent on-chain
    /// 3. Prevent double-spending
    ///
    /// # Arguments
    /// * `wallet_output` - The output to compute key image for
    ///
    /// # Returns
    /// * The 32-byte key image
    ///
    /// # Key Image Formula
    /// For full wallets: `key_image = (spend_key + key_offset) * H_p(output_key)`
    /// where:
    /// - spend_key: The wallet's private spend key
    /// - key_offset: The per-output derivation scalar from WalletOutput
    /// - H_p: Monero's hash-to-point function (biased_hash_to_point)
    /// - output_key: The output's public key
    ///
    /// For view-only wallets: Uses a deterministic placeholder based on
    /// transaction hash and output index, as view-only wallets cannot compute
    /// cryptographically correct key images (they lack the spend key).
    ///
    /// # Implementation Note: Why Not Use monero-oxide's Function?
    ///
    /// This reimplements the logic from monero-oxide's transaction signing code
    /// (`vendored/monero-oxide/monero-oxide/wallet/src/send/mod.rs:604`) because
    /// monero-oxide doesn't expose key image computation as a standalone utility.
    ///
    /// In monero-oxide, key images are computed inside `SignableTransaction::sign()`,
    /// which only runs when SPENDING (not scanning). That function:
    /// - Requires inputs already selected for a transaction
    /// - Returns a complete signed transaction
    /// - Cannot be called just to compute a key image during scanning
    ///
    /// Since the computation is simple (2 lines of cryptographic operations), and we
    /// need it during the scanning phase (Phase 2) before we have transactions to sign,
    /// duplication is more pragmatic than architectural complexity.
    ///
    /// Our implementation is validated against monero-oxide via comprehensive test
    /// vectors in `tests/key_image_tests.rs` to ensure correctness.
    ///
    /// If monero-oxide adds a `WalletOutput::compute_key_image()` helper in the future,
    /// we should switch to using that for single source of truth.
    ///
    /// # Errors
    /// Returns `WalletError::ViewOnlyWallet` if attempting to compute a proper
    /// key image for spending on a view-only wallet.
    fn compute_key_image(
        &self,
        wallet_output: &monero_wallet::WalletOutput,
    ) -> Result<KeyImage, WalletError> {
        match &self.spend_key {
            Some(spend_key) => {
                // Full wallet: Compute cryptographically correct key image
                // Formula: key_image = (spend_key + key_offset) * H_p(output_key)

                // Get the output's public key and key offset
                let output_key = wallet_output.key();
                let key_offset = wallet_output.key_offset();

                // Compute effective spend key = spend_key + key_offset
                let effective_spend_key = spend_key.deref() + key_offset;

                // Compute H_p(output_key) - hash output key to curve point
                let hash_point = biased_hash_to_point(output_key.compress().to_bytes());

                // Compute key image = effective_spend_key * H_p(output_key)
                let key_image_point = effective_spend_key * hash_point;

                // Compress to 32 bytes
                let key_image = key_image_point.compress().to_bytes();

                Ok(key_image)
            }
            None => {
                // View-only wallet: Use deterministic placeholder
                // View-only wallets cannot compute proper key images as they lack the spend key.
                // We use a deterministic hash of (tx_hash || output_index) as a placeholder.
                // This allows output tracking but NOT spending.
                use sha3::{Digest, Keccak256};

                let mut hasher = Keccak256::new();
                hasher.update(wallet_output.transaction());
                hasher.update(&wallet_output.index_in_transaction().to_le_bytes());
                let key_image: [u8; 32] = hasher.finalize().into();

                Ok(key_image)
            }
        }
    }

    // ========================================================================
    // BLOCKCHAIN REORGANIZATION - Handle chain reorgs
    // ========================================================================

    /// Handles a blockchain reorganization.
    ///
    /// When the blockchain reorganizes, blocks after the fork point
    /// are no longer valid. We must:
    /// 1. Remove outputs found in invalidated blocks
    /// 2. Revert sync height to before the fork
    /// 3. Re-scan from the fork point
    ///
    /// # Arguments
    /// * `fork_height` - The block height where the fork occurred.
    ///   All data from this height onward is invalidated.
    ///
    /// # Returns
    /// Number of outputs that were removed
    ///
    /// # Example
    /// ```no_run
    /// # use monero_rust::WalletState;
    /// # fn example(wallet: &mut WalletState) {
    /// // Detected a reorg at height 1000
    /// let removed_count = wallet.handle_reorganization(1000);
    /// println!("Removed {} outputs due to reorganization", removed_count);
    /// # }
    /// ```
    pub fn handle_reorganization(&mut self, fork_height: u64) -> usize {
        let mut removed_count = 0;
        let mut removed_key_images = Vec::new();

        // Remove all outputs found at or after the fork height
        self.outputs.retain(|key_image, output| {
            if output.height >= fork_height {
                removed_key_images.push(*key_image);
                removed_count += 1;
                false // Remove this output
            } else {
                true // Keep this output
            }
        });

        // Remove transaction records from invalidated blocks
        self.transactions.retain(|_txid, tx| {
            tx.height.map_or(true, |h| h < fork_height)
        });

        // CRITICAL: Clean up spent_outputs and frozen_outputs HashSets
        // to prevent dangling key image references
        for key_image in removed_key_images {
            self.spent_outputs.remove(&key_image);
            self.frozen_outputs.remove(&key_image);
        }

        // Reset sync height to just before the fork
        self.current_scanned_height = fork_height.saturating_sub(1);

        // Log the reorganization
        eprintln!(
            "Reorganization detected at height {}. Removed {} outputs. Rewinding to height {}.",
            fork_height, removed_count, self.current_scanned_height
        );

        removed_count
    }

    /// Detects if a blockchain reorganization has occurred.
    ///
    /// Compares our locally stored block height with daemon's view.
    /// If the daemon's height is less than ours, a reorg likely occurred.
    ///
    /// # Returns
    /// * `Some(fork_height)` if reorganization detected
    /// * `None` if blockchain is consistent
    ///
    /// # Errors
    /// * `WalletError::NotConnected` - Not connected to daemon
    /// * `WalletError::RpcError` - RPC communication failed
    ///
    /// # Note
    /// This is a conservative detection method. For more robust detection,
    /// compare block hashes at specific heights.
    pub async fn detect_reorganization(&mut self) -> Result<Option<u64>, WalletError> {
        // Query daemon for current height
        let rpc = self.get_rpc().await?;
        let daemon_height = rpc.get_height().await
            .map_err(|e| WalletError::RpcError(e))? as u64;

        // Update cached daemon height
        self.daemon_height = daemon_height;

        // If daemon height < our scanned height, a reorganization occurred
        if daemon_height < self.current_scanned_height {
            // Conservative approach: Rewind to daemon height - 10 blocks
            // This accounts for potential deeper reorganizations
            let fork_height = daemon_height.saturating_sub(10);
            return Ok(Some(fork_height));
        }

        // Could also verify block hashes here for more robust detection:
        // Compare our stored hash for block N with daemon's hash for block N
        // If mismatch, binary search to find exact fork point
        // (This requires storing block hashes, which we don't currently do)

        Ok(None)
    }

    // ========================================================================
    // SUBADDRESS MANAGEMENT - Register addresses for scanning
    // ========================================================================

    /// Registers a subaddress for scanning.
    ///
    /// The scanner will check all blocks for outputs sent to this subaddress.
    /// This is necessary before scanning can detect outputs to non-primary addresses.
    ///
    /// # Arguments
    /// * `account` - Account index (0 for primary account)
    /// * `address` - Address index within the account
    ///
    /// # Returns
    /// * `Ok(())` on success
    ///
    /// # Errors
    /// * `WalletError::Other` - If subaddress index is invalid
    ///
    /// # Example
    /// ```no_run
    /// # use monero_rust::WalletState;
    /// # fn example(wallet: &mut WalletState) {
    /// // Register subaddress (0, 1) - first subaddress of primary account
    /// wallet.register_subaddress(0, 1).unwrap();
    /// # }
    /// ```
    pub fn register_subaddress(&mut self, account: u32, address: u32) -> Result<(), WalletError> {
        let subaddress_index = SubaddressIndex::new(account, address)
            .ok_or_else(|| WalletError::Other("Invalid subaddress index".to_string()))?;

        // Check if already registered (avoid duplicates)
        if !self.registered_subaddresses.iter().any(|idx| idx == &subaddress_index) {
            // Register with scanner
            self.scanner.register_subaddress(subaddress_index);

            // Track that we're monitoring this subaddress
            self.registered_subaddresses.push(subaddress_index);
        }

        Ok(())
    }

    /// Registers a range of subaddresses at once.
    ///
    /// Useful for wallets that use many subaddresses (e.g., merchants).
    ///
    /// # Arguments
    /// * `account` - Account index
    /// * `start_address` - First address index to register
    /// * `end_address` - Last address index to register (inclusive)
    ///
    /// # Returns
    /// * Number of subaddresses registered (excluding duplicates)
    ///
    /// # Errors
    /// * `WalletError::Other` - If any subaddress index is invalid
    ///
    /// # Example
    /// ```no_run
    /// # use monero_rust::WalletState;
    /// # fn example(wallet: &mut WalletState) {
    /// // Register subaddresses (0, 1) through (0, 10)
    /// let count = wallet.register_subaddress_range(0, 1, 10).unwrap();
    /// println!("Registered {} subaddresses", count);
    /// # }
    /// ```
    pub fn register_subaddress_range(
        &mut self,
        account: u32,
        start_address: u32,
        end_address: u32,
    ) -> Result<usize, WalletError> {
        let mut count = 0;
        for address in start_address..=end_address {
            self.register_subaddress(account, address)?;
            count += 1;
        }
        Ok(count)
    }

    /// Returns all registered subaddresses.
    ///
    /// # Returns
    /// * Vector of (account, address) tuples for all registered subaddresses
    ///
    /// # Example
    /// ```no_run
    /// # use monero_rust::WalletState;
    /// # fn example(wallet: &WalletState) {
    /// let registered = wallet.get_registered_subaddresses();
    /// for (account, address) in registered {
    ///     println!("Registered: ({}, {})", account, address);
    /// }
    /// # }
    /// ```
    pub fn get_registered_subaddresses(&self) -> Vec<(u32, u32)> {
        self.registered_subaddresses
            .iter()
            .map(|idx| (idx.account(), idx.address()))
            .collect()
    }

    // ========================================================================
    // RPC CONNECTION MANAGEMENT
    // ========================================================================

    /// Gets a cloned reference to the RPC client if connected.
    ///
    /// This is an internal helper method that also performs an on-demand
    /// health check to verify the connection is still alive.
    ///
    /// # Returns
    /// `Ok(SimpleRequestRpc)` if connected, `Err(WalletError::NotConnected)` otherwise
    async fn get_rpc(&self) -> Result<monero_simple_request_rpc::SimpleRequestRpc, WalletError> {
        let rpc_client = self.rpc_client.read().await;

        match &*rpc_client {
            Some(rpc) => {
                // Clone the RPC client for use
                // SimpleRequestRpc is cheaply cloneable (Arc internally)
                Ok(rpc.clone())
            }
            None => Err(WalletError::NotConnected),
        }
    }

    /// Performs an on-demand connection health check.
    ///
    /// Queries the daemon height to verify the connection is still alive.
    /// Updates daemon_height and is_connected fields based on the result.
    ///
    /// # Returns
    /// `Ok(())` if connection is healthy, `Err(WalletError)` otherwise
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

    /// Starts the background health check task.
    ///
    /// This task periodically verifies the daemon connection is alive
    /// and attempts automatic reconnection if the connection is lost.
    ///
    /// Safe to call if health check is already running (it will be restarted).
    async fn start_health_check(&mut self) {
        // Stop existing health check if any
        self.stop_health_check().await;

        let rpc_client = self.rpc_client.clone();
        let interval = self.reconnection_policy.health_check_interval;
        let policy = self.reconnection_policy.clone();
        let connection_config = self.connection_config.clone();

        // Spawn the health check task
        let handle = tokio::spawn(async move {
            let mut check_interval = tokio::time::interval(interval);

            loop {
                check_interval.tick().await;

                // Try to get the RPC client
                let rpc = {
                    let client_lock = rpc_client.read().await;
                    client_lock.clone()
                };

                if let Some(rpc) = rpc {
                    // Perform health check
                    match rpc.get_height().await {
                        Ok(_height) => {
                            // Connection is healthy
                            // In a full implementation, we would update daemon_height here
                            // But we can't easily access &mut self from this task
                            // This is acceptable as health checks are primarily for detecting disconnection
                        }
                        Err(_e) => {
                            // Connection lost - attempt reconnection with exponential backoff
                            Self::attempt_reconnect_task(
                                rpc_client.clone(),
                                connection_config.clone(),
                                policy.clone(),
                            )
                            .await;
                        }
                    }
                } else {
                    // No RPC client, health check task should stop
                    break;
                }
            }
        });

        self.health_check_handle = Some(handle);
    }

    /// Stops the background health check task.
    ///
    /// Safe to call even if no health check is running.
    async fn stop_health_check(&mut self) {
        if let Some(handle) = self.health_check_handle.take() {
            handle.abort();
            // Wait for the task to fully stop (abort is not instant)
            let _ = handle.await;
        }
    }

    /// Attempts to reconnect to the daemon with exponential backoff.
    ///
    /// This is called automatically by the health check task when a connection is lost.
    /// It will retry up to `max_attempts` times with increasing delays between attempts.
    ///
    /// Note: This is a static method for use in the background task.
    /// It cannot directly modify the WalletState, so it operates on the RPC client directly.
    /// Uses the stored ConnectionConfig (including credentials) for reconnection.
    async fn attempt_reconnect_task(
        rpc_client: std::sync::Arc<tokio::sync::RwLock<Option<monero_simple_request_rpc::SimpleRequestRpc>>>,
        connection_config: Option<crate::rpc::ConnectionConfig>,
        policy: crate::rpc::ReconnectionPolicy,
    ) {
        use monero_simple_request_rpc::SimpleRequestRpc;

        let config = match connection_config {
            Some(config) => config,
            None => return,  // No config, can't reconnect
        };

        for attempt in 0..policy.max_attempts {
            // Wait with exponential backoff
            let delay = policy.delay_for_attempt(attempt);
            tokio::time::sleep(delay).await;

            // Build URL with credentials (returns Zeroizing<String>)
            let url = config.build_url();

            // Attempt to create a new RPC client with the configured timeout
            match SimpleRequestRpc::with_custom_timeout(url.as_str().to_string(), config.timeout).await {
                Ok(new_rpc) => {
                    // Verify connection with a health check
                    match new_rpc.get_height().await {
                        Ok(_) => {
                            // Success! Update the RPC client
                            let mut client_lock = rpc_client.write().await;
                            *client_lock = Some(new_rpc);
                            // Successfully reconnected - credentials were preserved
                            return;
                        }
                        Err(_) => {
                            // Health check failed, try again
                            continue;
                        }
                    }
                }
                Err(_) => {
                    // Connection attempt failed, try again
                    continue;
                }
            }
        }

        // All reconnection attempts failed
        // In a production system, this should emit an event or callback
        // rather than just silently failing
    }

    // ========================================================================
    // FILE I/O - Encrypted wallet persistence
    // ========================================================================

    /// Magic bytes identifying a monero-rust wallet file: "MNRS"
    const MAGIC_BYTES: &'static [u8; 4] = b"MNRS";

    /// Size of the fixed header in bytes (magic + version + salt + nonce)
    const HEADER_SIZE: usize = 4 + 4 + 32 + 12; // 52 bytes

    /// Saves the wallet to its configured path with password encryption.
    ///
    /// This is a convenience method that saves to `self.wallet_path`.
    /// For saving to a different location, use `save_to_file()`.
    ///
    /// # Arguments
    /// * `password` - Password for encrypting the wallet file
    ///
    /// # Errors
    /// Returns `WalletError` if the wallet is closed, serialization fails,
    /// encryption fails, or file writing fails.
    ///
    /// # File Format
    /// The wallet file has the following structure:
    /// ```text
    /// [Magic: "MNRS" (4 bytes)]
    /// [Version: u32 little-endian (4 bytes)]
    /// [Salt: (32 bytes)]
    /// [Nonce: (12 bytes)]
    /// [Encrypted data with auth tag: (variable)]
    /// ```
    ///
    /// # Security
    /// - The entire wallet state is encrypted with AES-256-GCM
    /// - Password is stretched with Argon2id for key derivation
    /// - Authentication tag prevents tampering
    /// - Atomic writes (temp file + rename) prevent corruption
    pub fn save(&self, password: &str) -> Result<(), WalletError> {
        let path = self.wallet_path.clone();
        self.save_to_file(&path, password)
    }

    /// Saves the wallet to a specific path with password encryption.
    ///
    /// # Arguments
    /// * `path` - Path where the wallet file should be saved
    /// * `password` - Password for encrypting the wallet file
    ///
    /// # Errors
    /// Returns `WalletError` if the wallet is closed, serialization fails,
    /// encryption fails, or file writing fails.
    ///
    /// # Implementation Notes
    /// Uses atomic writes (write to temp file, then rename) to prevent
    /// corruption if the process is interrupted.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P, password: &str) -> Result<(), WalletError> {
        // Check if wallet is closed
        if self.is_closed {
            return Err(WalletError::WalletClosed);
        }

        // CRITICAL: Verify password matches wallet's stored password before saving
        // This prevents accidentally saving with wrong password and locking out funds
        Self::verify_password(password, &self.password_salt, &self.password_hash)
            .map_err(|_| WalletError::InvalidPassword)?;

        let path = path.as_ref();

        // Serialize the wallet state to bytes using bincode
        let serialized_data = bincode::serialize(self)
            .map_err(|e| WalletError::SerializationError(format!("Failed to serialize wallet: {}", e)))?;

        // Generate a new nonce for this encryption
        let nonce = generate_nonce();

        // Encrypt the serialized data
        let encrypted_data = encrypt_wallet_data(
            &serialized_data,
            password,
            &self.password_salt,
            &nonce,
        )
        .map_err(|e| WalletError::EncryptionError(e))?;

        // Build the complete file contents
        let mut file_contents = Vec::with_capacity(Self::HEADER_SIZE + encrypted_data.len());

        // Write magic bytes
        file_contents.extend_from_slice(Self::MAGIC_BYTES);

        // Write version (u32 little-endian)
        file_contents.extend_from_slice(&self.version.to_le_bytes());

        // Write salt
        file_contents.extend_from_slice(&self.password_salt);

        // Write nonce
        file_contents.extend_from_slice(&nonce);

        // Write encrypted data (includes auth tag from AES-GCM)
        file_contents.extend_from_slice(&encrypted_data);

        // Atomic write: write to temp file with proper permissions, then rename
        let temp_path = path.with_extension("tmp");

        // Create temp file with exclusive access (prevents TOCTOU race conditions)
        // Set restrictive permissions on Unix (owner read/write only)
        #[cfg(unix)]
        let mut temp_file = {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .write(true)
                .create_new(true)  // Fails if file exists - prevents races and symlink attacks
                .mode(0o600)       // Owner read/write only
                .open(&temp_path)?
        };

        #[cfg(not(unix))]
        let mut temp_file = OpenOptions::new()
            .write(true)
            .create_new(true)  // Fails if file exists - prevents races
            .open(&temp_path)?;

        // Write data and ensure it's flushed to disk before rename
        temp_file.write_all(&file_contents)?;
        temp_file.sync_all()?;
        drop(temp_file);  // Close file before rename

        // Atomically rename to final destination
        // If rename fails, clean up temp file
        match fs::rename(&temp_path, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Clean up temp file on failure
                let _ = fs::remove_file(&temp_path);
                Err(WalletError::IoError(e))
            }
        }
    }

    /// Loads a wallet from a file with password decryption.
    ///
    /// # Arguments
    /// * `path` - Path to the wallet file
    /// * `password` - Password for decrypting the wallet file
    ///
    /// # Returns
    /// A fully reconstructed `WalletState` with ViewPair initialized.
    ///
    /// # Errors
    /// Returns `WalletError` if:
    /// - File cannot be read
    /// - File format is invalid (wrong magic bytes, truncated header)
    /// - Wallet version is unsupported
    /// - Password is incorrect
    /// - Data is corrupted or tampered with
    /// - Deserialization fails
    ///
    /// # Security
    /// - Password is verified indirectly through decryption (wrong password = decryption failure)
    /// - Authentication tag ensures data integrity
    /// - Checksum verification ensures keys are correctly reconstructed
    pub fn load_from_file<P: AsRef<Path>>(path: P, password: &str) -> Result<Self, WalletError> {
        let path = path.as_ref();

        // Read the entire file
        let mut file = fs::File::open(path)?;
        let mut file_contents = Vec::new();
        file.read_to_end(&mut file_contents)?;

        // Validate minimum size
        if file_contents.len() < Self::HEADER_SIZE {
            return Err(WalletError::CorruptedFile(format!(
                "File too small: expected at least {} bytes, got {}",
                Self::HEADER_SIZE,
                file_contents.len()
            )));
        }

        // Parse magic bytes
        let magic = &file_contents[0..4];
        if magic != Self::MAGIC_BYTES {
            return Err(WalletError::CorruptedFile(format!(
                "Invalid magic bytes: expected {:?}, got {:?}",
                Self::MAGIC_BYTES,
                magic
            )));
        }

        // Parse version
        let version_bytes: [u8; 4] = file_contents[4..8]
            .try_into()
            .map_err(|_| WalletError::CorruptedFile("Failed to read version".to_string()))?;
        let version = u32::from_le_bytes(version_bytes);

        // Check version compatibility
        if version > WALLET_VERSION {
            return Err(WalletError::UnsupportedVersion(version));
        }

        // Parse salt
        let salt: [u8; 32] = file_contents[8..40]
            .try_into()
            .map_err(|_| WalletError::CorruptedFile("Failed to read salt".to_string()))?;

        // Parse nonce
        let nonce: [u8; 12] = file_contents[40..52]
            .try_into()
            .map_err(|_| WalletError::CorruptedFile("Failed to read nonce".to_string()))?;

        // Remaining bytes are encrypted data (includes 16-byte auth tag)
        let encrypted_data = &file_contents[52..];

        // Decrypt the data - wrap in Zeroizing to ensure memory is cleared even on error
        let decrypted_data = Zeroizing::new(
            decrypt_wallet_data(encrypted_data, password, &salt, &nonce).map_err(|e| {
                // Check if it's a decryption failure (likely wrong password)
                if e.contains("invalid password") || e.contains("corrupted") {
                    WalletError::InvalidPassword
                } else {
                    WalletError::EncryptionError(e)
                }
            })?,
        );

        // Deserialize the wallet state
        // decrypted_data is automatically zeroized when it goes out of scope
        let wallet: WalletState = bincode::deserialize(&*decrypted_data)
            .map_err(|e| WalletError::SerializationError(format!("Failed to deserialize wallet: {}", e)))?;

        // The custom Deserialize implementation automatically:
        // - Reconstructs ViewPair from seed or view-only keys
        // - Verifies keys checksum for integrity
        // - Sets runtime state (is_syncing, auto_save_enabled) to defaults

        Ok(wallet)
    }

    /// Reconstructs the ViewPair from the seed.
    ///
    /// NOTE: This method is deprecated and no longer needed since ViewPair
    /// is now automatically reconstructed during deserialization.
    /// Kept for API compatibility but does nothing for view-only wallets.
    pub fn reconstruct_view_pair(&mut self) -> Result<(), String> {
        use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint};
        use sha3::{Digest, Keccak256};

        // For view-only wallets, ViewPair is already reconstructed from stored components
        let seed = match &self.seed {
            Some(s) => s,
            None => return Ok(()), // View-only wallet, nothing to reconstruct
        };

        let spend: [u8; 32] = *seed.entropy();
        let spend_scalar = Scalar::from_bytes_mod_order(spend);
        let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);

        self.view_pair =
            ViewPair::new(spend_point, Zeroizing::new(view_scalar)).map_err(|e| e.to_string())?;

        // Also reconstruct spend_key if not view-only
        if self.spend_key.is_some() {
            self.spend_key = Some(Zeroizing::new(spend_scalar));
        }

        Ok(())
    }
}

// Custom Deserialize implementation that automatically reconstructs ViewPair
//
// This eliminates the footgun of forgetting to call reconstruct_view_pair() manually.
impl<'de> Deserialize<'de> for WalletState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Helper struct that matches the serialized format
        #[derive(Deserialize)]
        struct WalletStateHelper {
            #[serde(default = "default_version")]
            version: u32,
            // Deserialize as raw bytes, we'll reconstruct Seed with proper language later
            // None for view-only wallets
            seed: Option<Vec<u8>>,
            // View-only wallet fields (must come right after seed to match serialization order)
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
            // Registered subaddresses for scanning (defaults to empty if not present for backwards compat)
            // Stored as Vec<(u32, u32)> since SubaddressIndex doesn't implement Deserialize
            #[serde(default)]
            registered_subaddresses: Vec<(u32, u32)>,
        }

        let helper = WalletStateHelper::deserialize(deserializer)?;

        // Check version compatibility
        if helper.version > WALLET_VERSION {
            return Err(serde::de::Error::custom(format!(
                "Wallet file version {} is newer than supported version {}. Please upgrade monero-rust.",
                helper.version, WALLET_VERSION
            )));
        }
        // Note: We support loading older versions (version < WALLET_VERSION) for backwards compatibility

        use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint};
        use sha3::{Digest, Keccak256};

        // Reconstruct Seed and ViewPair based on wallet type
        let (seed_opt, view_pair) = if let Some(seed_bytes) = helper.seed {
            // Regular wallet: reconstruct from seed
            if seed_bytes.len() != 32 {
                return Err(serde::de::Error::custom(format!(
                    "Invalid seed entropy length: expected 32, got {}",
                    seed_bytes.len()
                )));
            }

            let mut entropy = Zeroizing::new([0u8; 32]);
            entropy.copy_from_slice(&seed_bytes);

            // Parse the language from the seed_language string
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
                _ => Language::English, // Default to English if unknown
            };

            let seed = Seed::from_entropy(language, entropy)
                .ok_or_else(|| serde::de::Error::custom("Invalid seed entropy (not a valid scalar)"))?;

            // Reconstruct ViewPair from seed
            let spend: [u8; 32] = *seed.entropy();
            let spend_scalar = Scalar::from_bytes_mod_order(spend);
            let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;

            let view: [u8; 32] = Keccak256::digest(&spend).into();
            let view_scalar = Scalar::from_bytes_mod_order(view);

            let vp = ViewPair::new(spend_point, Zeroizing::new(view_scalar))
                .map_err(|e| serde::de::Error::custom(format!("Failed to reconstruct ViewPair: {}", e)))?;

            // CRITICAL: Verify that reconstructed keys match the stored checksum
            let reconstructed_checksum = compute_keys_checksum(&vp);
            if reconstructed_checksum != helper.keys_checksum {
                return Err(serde::de::Error::custom(
                    "CRITICAL: ViewPair keys checksum mismatch! Seed may be corrupted or tampered with. \
                     The reconstructed keys do not match the original keys. DO NOT use this wallet."
                ));
            }

            (Some(Zeroizing::new(seed)), vp)
        } else {
            // View-only wallet: reconstruct from stored ViewPair components
            let spend_public = helper.view_only_spend_public
                .as_ref()
                .ok_or_else(|| serde::de::Error::custom("View-only wallet missing spend public key"))?;
            let view_private = helper.view_only_view_private
                .as_ref()
                .ok_or_else(|| serde::de::Error::custom("View-only wallet missing view private key"))?;

            // Decompress the spend public key
            let spend_point = curve25519_dalek::edwards::CompressedEdwardsY(*spend_public)
                .decompress()
                .ok_or_else(|| serde::de::Error::custom("Invalid spend public key"))?;

            // Reconstruct view scalar
            let view_scalar = Scalar::from_bytes_mod_order(**view_private);

            let vp = ViewPair::new(spend_point, Zeroizing::new(view_scalar))
                .map_err(|e| serde::de::Error::custom(format!("Failed to reconstruct ViewPair: {}", e)))?;

            // Verify checksum for view-only wallets too
            let reconstructed_checksum = compute_keys_checksum(&vp);
            if reconstructed_checksum != helper.keys_checksum {
                return Err(serde::de::Error::custom(
                    "CRITICAL: ViewPair keys checksum mismatch! ViewPair may be corrupted or tampered with. \
                     DO NOT use this wallet."
                ));
            }

            (None, vp)
        };

        // Reconstruct Scanner from ViewPair and re-register all subaddresses
        // Note: Scanner::new() automatically handles primary address (0, 0)
        let mut scanner = Scanner::new(view_pair.clone());
        let mut registered_subaddresses = Vec::new();

        // Convert Vec<(u32, u32)> to Vec<SubaddressIndex>
        // Skip (0, 0) as it's not a valid SubaddressIndex (primary address is handled by scanner)
        for (account, address) in helper.registered_subaddresses {
            // Skip primary address (0, 0) - it's automatically handled
            if account == 0 && address == 0 {
                continue;
            }

            if let Some(idx) = SubaddressIndex::new(account, address) {
                registered_subaddresses.push(idx);
                // Register with scanner
                scanner.register_subaddress(idx);
            } else {
                return Err(serde::de::Error::custom(format!(
                    "Invalid subaddress index during deserialization: ({}, {})",
                    account, address
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
            is_syncing: false, // Runtime state, always false on deserialization
            daemon_address: helper.daemon_address,
            is_connected: helper.is_connected,
            password_salt: helper.password_salt,
            password_hash: helper.password_hash,
            wallet_path: helper.wallet_path,
            is_closed: false, // Always open wallets on load - user can close explicitly if needed
            keys_checksum: helper.keys_checksum,
            auto_save_enabled: false, // Runtime state, not serialized
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

    // ========================================================================
    // SCANNING TESTS - Output detection and blockchain scanning
    // ========================================================================

    #[test]
    fn test_scanner_initialized_with_primary_address() {
        use rand_core::OsRng;

        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let wallet_state = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test_password",
            PathBuf::from("test.bin"),
            0,
        )
        .unwrap();

        // Scanner should be initialized (we can't directly test it but check side effects)
        // Primary address (0, 0) is handled automatically by Scanner, not stored in registered_subaddresses
        let registered = wallet_state.get_registered_subaddresses();
        assert_eq!(registered.len(), 0); // No subaddresses registered yet (primary is automatic)
    }

    #[test]
    fn test_register_subaddress() {
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

        // Initially no subaddresses (primary is handled automatically)
        assert_eq!(wallet_state.registered_subaddresses.len(), 0);

        // Register a new subaddress
        wallet_state.register_subaddress(0, 1).expect("Failed to register subaddress");

        // Should now have 1 registered subaddress
        assert_eq!(wallet_state.registered_subaddresses.len(), 1);

        let registered = wallet_state.get_registered_subaddresses();
        assert!(registered.contains(&(0, 1)));

        // Registering the same address again should be idempotent (no duplicates)
        wallet_state.register_subaddress(0, 1).expect("Failed to register subaddress again");
        assert_eq!(wallet_state.registered_subaddresses.len(), 1);
    }

    #[test]
    fn test_register_subaddress_range() {
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

        // Register range (0, 1) through (0, 5)
        let count = wallet_state.register_subaddress_range(0, 1, 5)
            .expect("Failed to register range");

        assert_eq!(count, 5);

        // Should now have 5 subaddresses: (0,1) through (0,5)
        // Primary (0, 0) is automatic, not in this list
        assert_eq!(wallet_state.registered_subaddresses.len(), 5);

        let registered = wallet_state.get_registered_subaddresses();
        assert!(registered.contains(&(0, 1)));
        assert!(registered.contains(&(0, 2)));
        assert!(registered.contains(&(0, 3)));
        assert!(registered.contains(&(0, 4)));
        assert!(registered.contains(&(0, 5)));
    }

    #[test]
    fn test_handle_reorganization_removes_outputs() {
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

        // Add outputs at different heights
        let output_at_100 = SerializableOutput {
            tx_hash: [1u8; 32],
            output_index: 0,
            amount: 1000000000000,
            key_image: [1u8; 32],
            subaddress_indices: (0, 0),
            height: 100,
            unlocked: false,
            spent: false,
            frozen: false,
        };

        let output_at_105 = SerializableOutput {
            tx_hash: [2u8; 32],
            output_index: 0,
            amount: 2000000000000,
            key_image: [2u8; 32],
            subaddress_indices: (0, 0),
            height: 105,
            unlocked: false,
            spent: false,
            frozen: false,
        };

        let output_at_110 = SerializableOutput {
            tx_hash: [3u8; 32],
            output_index: 0,
            amount: 3000000000000,
            key_image: [3u8; 32],
            subaddress_indices: (0, 0),
            height: 110,
            unlocked: false,
            spent: false,
            frozen: false,
        };

        wallet_state.outputs.insert([1u8; 32], output_at_100);
        wallet_state.outputs.insert([2u8; 32], output_at_105);
        wallet_state.outputs.insert([3u8; 32], output_at_110);
        wallet_state.current_scanned_height = 110;

        assert_eq!(wallet_state.outputs.len(), 3);

        // Reorganization at height 105 should remove outputs at 105 and above
        let removed_count = wallet_state.handle_reorganization(105);

        assert_eq!(removed_count, 2); // Outputs at 105 and 110 removed
        assert_eq!(wallet_state.outputs.len(), 1); // Only output at 100 remains
        assert!(wallet_state.outputs.contains_key(&[1u8; 32]));
        assert!(!wallet_state.outputs.contains_key(&[2u8; 32]));
        assert!(!wallet_state.outputs.contains_key(&[3u8; 32]));

        // Scanned height should be rewound to fork_height - 1
        assert_eq!(wallet_state.current_scanned_height, 104);
    }

    #[test]
    fn test_handle_reorganization_cleans_hashsets() {
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

        // Add outputs and mark some as spent/frozen
        let output1 = SerializableOutput {
            tx_hash: [1u8; 32],
            output_index: 0,
            amount: 1000000000000,
            key_image: [1u8; 32],
            subaddress_indices: (0, 0),
            height: 100,
            unlocked: false,
            spent: false,
            frozen: false,
        };

        let output2 = SerializableOutput {
            tx_hash: [2u8; 32],
            output_index: 0,
            amount: 2000000000000,
            key_image: [2u8; 32],
            subaddress_indices: (0, 0),
            height: 110,
            unlocked: false,
            spent: false,
            frozen: false,
        };

        wallet_state.outputs.insert([1u8; 32], output1);
        wallet_state.outputs.insert([2u8; 32], output2);

        // Mark output2 as spent and frozen
        wallet_state.spent_outputs.insert([2u8; 32]);
        wallet_state.frozen_outputs.insert([2u8; 32]);

        assert_eq!(wallet_state.spent_outputs.len(), 1);
        assert_eq!(wallet_state.frozen_outputs.len(), 1);

        // Reorganization at height 105 removes output2
        wallet_state.handle_reorganization(105);

        // Key image [2] should be removed from HashSets
        assert_eq!(wallet_state.spent_outputs.len(), 0);
        assert_eq!(wallet_state.frozen_outputs.len(), 0);
        assert!(!wallet_state.spent_outputs.contains(&[2u8; 32]));
        assert!(!wallet_state.frozen_outputs.contains(&[2u8; 32]));
    }

    #[test]
    fn test_handle_reorganization_cleans_transactions() {
        use rand_core::OsRng;
        use crate::types::Transaction;

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

        // Add transactions at different heights
        let tx1 = Transaction::new_incoming([1u8; 32], Some(100), 1234567890, 1000000000000);
        let tx2 = Transaction::new_incoming([2u8; 32], Some(105), 1234567895, 2000000000000);
        let tx3 = Transaction::new_incoming([3u8; 32], Some(110), 1234567900, 3000000000000);

        wallet_state.transactions.insert([1u8; 32], tx1);
        wallet_state.transactions.insert([2u8; 32], tx2);
        wallet_state.transactions.insert([3u8; 32], tx3);

        assert_eq!(wallet_state.transactions.len(), 3);

        // Reorganization at height 105
        wallet_state.handle_reorganization(105);

        // Only tx1 (height 100) should remain
        assert_eq!(wallet_state.transactions.len(), 1);
        assert!(wallet_state.transactions.contains_key(&[1u8; 32]));
        assert!(!wallet_state.transactions.contains_key(&[2u8; 32]));
        assert!(!wallet_state.transactions.contains_key(&[3u8; 32]));
    }

    #[test]
    fn test_subaddresses_persist_across_save_load() {
        use rand_core::OsRng;
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let wallet_path = temp_file.path().to_path_buf();

        // Create wallet and register subaddresses
        let seed = Seed::new(&mut OsRng, monero_seed::Language::English);
        let seed_str = seed.to_string().to_string();

        let mut wallet_state = WalletState::new(
            seed,
            String::from("English"),
            Network::Mainnet,
            "test_password",
            wallet_path.clone(),
            0,
        )
        .unwrap();

        // Register some subaddresses
        wallet_state.register_subaddress(0, 1).unwrap();
        wallet_state.register_subaddress(0, 2).unwrap();
        wallet_state.register_subaddress(1, 0).unwrap();

        assert_eq!(wallet_state.registered_subaddresses.len(), 3);

        // Save wallet
        wallet_state.save("test_password").expect("Failed to save wallet");

        // Load wallet
        let loaded_wallet = WalletState::load_from_file(
            wallet_path.to_str().unwrap(),
            "test_password"
        ).expect("Failed to load wallet");

        // Verify subaddresses were restored
        assert_eq!(loaded_wallet.registered_subaddresses.len(), 3);

        let registered = loaded_wallet.get_registered_subaddresses();
        assert!(registered.contains(&(0, 1)));
        assert!(registered.contains(&(0, 2)));
        assert!(registered.contains(&(1, 0)));
    }

    #[test]
    fn test_invalid_subaddress_index() {
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

        // SubaddressIndex::new() returns None for invalid indices
        // In practice, this would be very large numbers that overflow internal limits
        // For now, all reasonable values should work, so we just verify the API
        // returns Result and handles the Option correctly

        // This should succeed for reasonable values
        let result = wallet_state.register_subaddress(0, 100);
        assert!(result.is_ok());

        let result = wallet_state.register_subaddress(10, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_registered_subaddresses() {
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

        wallet_state.register_subaddress(0, 1).unwrap();
        wallet_state.register_subaddress(0, 5).unwrap();
        wallet_state.register_subaddress(1, 0).unwrap();

        let registered = wallet_state.get_registered_subaddresses();

        // Should have 3 registered subaddresses (primary is automatic)
        assert_eq!(registered.len(), 3);

        // Verify all are present
        assert!(registered.contains(&(0, 1)));
        assert!(registered.contains(&(0, 5)));
        assert!(registered.contains(&(1, 0)));
    }
}

