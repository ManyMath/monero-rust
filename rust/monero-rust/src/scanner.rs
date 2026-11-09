//! Unified scanning implementation for Monero wallets.
//!
//! This module provides wallet scanning functionality that works across
//! both native and WASM targets through generic RpcConnection support.

use crate::monero_rpc::{
    BlockOutputIndices, GetBlocksFastResponse, PoolInfo, PoolTx, Rpc, RpcConnection,
};
use crate::seed_compat::{Language, Seed};
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE,
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
};
use monero_oxide::{
    block::{Block as OxideBlock, BlockHeader as OxideBlockHeader},
    transaction::{
        Input as OxideInput, NotPruned, Pruned, Timelock as OxideTimelock,
        Transaction as OxideTransaction, TransactionPrefix as OxideTransactionPrefix,
    },
};
use monero_wallet::{
    address::{MoneroAddress, Network, SubaddressIndex},
    ed25519::{Point as OxidePoint, Scalar as OxideScalar},
    extra::PaymentId,
    interface::ScannableBlock,
    Scanner, ViewPair as OxideViewPair, WalletOutput as OxideWalletOutput,
};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

use crate::wallet_output::WalletOutput;
use crate::wallet_state::WalletState;
use std::collections::{BTreeMap, HashMap, HashSet};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Yield interval for the non-WASM code path; WASM uses time-based yielding (~16 ms).
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
const YIELD_EVERY_N_BLOCKS: usize = 50;

/// Yield to other tasks on wasm; do nothing on native targets.
#[inline]
async fn yield_to_event_loop() {
    #[cfg(target_arch = "wasm32")]
    {
        gloo_timers::future::TimeoutFuture::new(0).await;
    }
}

/// Computes a Monero block ID.
pub fn compute_block_id(block: &OxideBlock) -> [u8; 32] {
    block.hash()
}

/// Fallback key image extraction from raw tx bytes when `Transaction::read()` fails.
pub fn extract_key_images_from_raw_tx(tx_blob: &[u8]) -> Vec<String> {
    extract_spent_rings_from_raw_tx(tx_blob)
        .into_iter()
        .map(|(key_image, _)| key_image)
        .collect()
}

/// The key image and ring (relative offsets) of each key input of a raw transaction.
pub fn extract_spent_rings_from_raw_tx(tx_blob: &[u8]) -> Vec<(String, Vec<u64>)> {
    use std::io::{Cursor, Read};

    fn read_varint(r: &mut Cursor<&[u8]>) -> Option<u64> {
        let mut bits = 0u32;
        let mut res = 0u64;
        loop {
            let mut b = [0u8; 1];
            r.read_exact(&mut b).ok()?;
            let b = b[0];
            res += u64::from(b & 0x7f) << bits;
            bits += 7;
            if bits > 64 {
                return None;
            }
            if b & 0x80 == 0 {
                return Some(res);
            }
        }
    }

    let mut key_images = Vec::new();
    let mut cursor = Cursor::new(tx_blob);

    let Some(_version) = read_varint(&mut cursor) else {
        return key_images;
    };
    let Some(_timelock) = read_varint(&mut cursor) else {
        return key_images;
    };
    let Some(num_inputs) = read_varint(&mut cursor) else {
        return key_images;
    };

    for _ in 0..num_inputs {
        let mut type_byte = [0u8; 1];
        if cursor.read_exact(&mut type_byte).is_err() {
            break;
        }

        match type_byte[0] {
            0xff => {
                // Gen input: varint height
                if read_varint(&mut cursor).is_none() {
                    break;
                }
            }
            0x02 => {
                // ToKey input: amount, key_offsets, 32-byte key_image
                let Some(_amount) = read_varint(&mut cursor) else {
                    break;
                };
                let Some(num_offsets) = read_varint(&mut cursor) else {
                    break;
                };
                let mut offsets = Vec::new();
                for _ in 0..num_offsets {
                    match read_varint(&mut cursor) {
                        Some(offset) => offsets.push(offset),
                        None => break,
                    }
                }
                let mut ki = [0u8; 32];
                if cursor.read_exact(&mut ki).is_err() {
                    break;
                }
                key_images.push((hex::encode(ki), offsets));
            }
            _ => break,
        }
    }

    key_images
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockScanResult {
    pub block_height: u64,
    pub block_hash: String,
    /// Hash of the block this one builds on, or empty when unknown.
    #[serde(default)]
    pub previous_block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: usize,
    pub outputs: Vec<WalletOutput>,
    pub daemon_height: u64,
    pub spent_key_images: Vec<String>,
    pub spent_key_image_tx_hashes: Vec<String>,
    /// The ring of each spent key image as relative offsets, parallel to
    /// `spent_key_images`; empty or missing when unknown.
    #[serde(default)]
    pub spent_key_image_rings: Vec<Vec<u64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolScanResult {
    pub tx_count: usize,
    pub outputs: Vec<WalletOutput>,
    pub spent_key_images: Vec<String>,
    pub spent_key_image_tx_hashes: Vec<String>,
}

/// Multi-wallet scan result containing outputs for each wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiWalletScanResult {
    pub block_height: u64,
    pub block_hash: String,
    /// Hash of the block this one builds on; empty in old records.
    #[serde(default)]
    pub previous_block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: usize,
    pub daemon_height: u64,
    pub spent_key_images: Vec<String>,
    pub spent_key_image_tx_hashes: Vec<String>,
    /// The ring of each spent key image; see `BlockScanResult::spent_key_image_rings`.
    #[serde(default)]
    pub spent_key_image_rings: Vec<Vec<u64>>,
    /// Map of wallet address to its scan result
    pub wallet_results: HashMap<String, WalletScanData>,
}

/// Individual wallet's scan data within a multi-wallet scan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletScanData {
    pub address: String,
    pub outputs: Vec<WalletOutput>,
}

/// Configuration for a single wallet in multi-wallet scanning
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct WalletScanConfig {
    pub mnemonic: String,
    #[zeroize(skip)]
    pub network: Network,
    pub lookahead: Lookahead,
    pub passphrase: String,
}

impl std::fmt::Debug for WalletScanConfig {
    // The mnemonic and the passphrase are the wallet: never print them.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalletScanConfig")
            .field("network", &self.network)
            .field("lookahead", &self.lookahead)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct DerivedKeys {
    /// `None` for view-only wallets.
    pub secret_spend_key: Option<Zeroizing<[u8; 32]>>,
    pub secret_view_key: Zeroizing<[u8; 32]>,
    pub public_spend_key: [u8; 32],
    pub public_view_key: [u8; 32],
    pub address: MoneroAddress,
}

impl std::fmt::Debug for DerivedKeys {
    // `Zeroizing` prints what it holds, so the secret keys are left out.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DerivedKeys")
            .field("public_spend_key", &hex::encode(self.public_spend_key))
            .field("public_view_key", &hex::encode(self.public_view_key))
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Zeroize)]
pub struct Lookahead {
    pub account: u32,
    pub subaddress: u32,
}

/// wallet2's default subaddress window (`SUBADDRESS_LOOKAHEAD_MAJOR` 50,
/// `SUBADDRESS_LOOKAHEAD_MINOR` 200); the bounds here are inclusive.
pub const DEFAULT_LOOKAHEAD: Lookahead = Lookahead {
    account: 49,
    subaddress: 199,
};

pub const WALLET_CLI_SOFTWARE_LOOKAHEAD: Lookahead = Lookahead {
    account: 50,
    subaddress: 200,
};

pub const WALLET_CLI_HARDWARE_LOOKAHEAD: Lookahead = Lookahead {
    account: 5,
    subaddress: 20,
};

/// Derive a legacy (standard) address from raw public keys.
fn standard_address(network: Network, spend: EdwardsPoint, view: EdwardsPoint) -> MoneroAddress {
    let convert = |point: EdwardsPoint| {
        monero_wallet::ed25519::CompressedPoint::from(point.compress().to_bytes())
            .decompress()
            .expect("deriving an address from an invalid public key")
    };
    MoneroAddress::new(
        network,
        monero_wallet::address::AddressType::Legacy,
        convert(spend),
        convert(view),
    )
}

fn spend_key_from_seed(seed: &Seed, passphrase: &str) -> EdwardsPoint {
    // SEC-02: Use Zeroizing to ensure spend scalar is cleaned up on drop
    let key_bytes = seed.key_bytes_with_passphrase(passphrase);
    let spend_scalar = Zeroizing::new(Scalar::from_bytes_mod_order(*key_bytes));
    &*spend_scalar * ED25519_BASEPOINT_TABLE
}

/// Returns the compressed public spend key for `mnemonic`/`passphrase`.
///
/// Matches the value stored in `CachedScanner::fingerprint`.
pub fn spend_key_fingerprint(mnemonic: &str, passphrase: &str) -> Result<[u8; 32], String> {
    let seed = resolve_seed(mnemonic)?;
    let spend_point = spend_key_from_seed(&seed, passphrase);
    Ok(spend_point.compress().to_bytes())
}

fn view_key_from_seed(seed: &Seed, passphrase: &str) -> Scalar {
    // SEC-02: Use Zeroizing to ensure spend scalar intermediate is cleaned up on drop
    let key_bytes = seed.key_bytes_with_passphrase(passphrase);
    let spend_scalar = Zeroizing::new(Scalar::from_bytes_mod_order(*key_bytes));
    let view: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    Scalar::from_bytes_mod_order(view)
}

fn spend_key_scalar_from_seed(seed: &Seed, passphrase: &str) -> Zeroizing<Scalar> {
    let key_bytes = seed.key_bytes_with_passphrase(passphrase);
    Zeroizing::new(Scalar::from_bytes_mod_order(*key_bytes))
}

/// Compute the key image `x * H_p(xG)` for an owned one-time key.
#[allow(dead_code)] // native targets fill key images after scanning instead
fn calculate_key_image(spend_scalar: &Scalar, key_offset: &Scalar) -> EdwardsPoint {
    let one_time_key_scalar = Zeroizing::new(spend_scalar + key_offset);
    let one_time_key = &*one_time_key_scalar * ED25519_BASEPOINT_TABLE;
    let hash_point: EdwardsPoint =
        OxidePoint::biased_hash(one_time_key.compress().to_bytes()).into();
    hash_point * *one_time_key_scalar
}

pub fn build_subaddress_indices(lookahead: Lookahead) -> Vec<SubaddressIndex> {
    let mut indices = Vec::new();
    for account in 0..=lookahead.account {
        for address in 0..=lookahead.subaddress {
            if let Some(index) = SubaddressIndex::new(account, address) {
                indices.push(index);
            }
        }
    }
    indices
}

pub fn register_subaddresses(scanner: &mut Scanner, lookahead: Lookahead) {
    for index in build_subaddress_indices(lookahead) {
        scanner.register_subaddress(index);
    }
}

/// How often to yield during subaddress registration on WASM.
const YIELD_EVERY_N_REGISTRATIONS: usize = 100;

/// Register subaddresses asynchronously, yielding every 100 registrations on wasm
/// so other tasks can run during large lookaheads.
pub async fn register_subaddresses_async(scanner: &mut Scanner, lookahead: Lookahead) {
    for (i, index) in build_subaddress_indices(lookahead).into_iter().enumerate() {
        scanner.register_subaddress(index);
        if i % YIELD_EVERY_N_REGISTRATIONS == YIELD_EVERY_N_REGISTRATIONS - 1 {
            yield_to_event_loop().await;
        }
    }
}

/// Tracks the high-water mark of registered subaddresses per account so we can
/// expand the scanning window when outputs are discovered near the edge.
struct SubaddressWatermark {
    /// For each account we've registered, the max minor index registered.
    /// Accounts `0..max_minor_per_account.len()` are covered.
    max_minor_per_account: Vec<u32>,
}

impl SubaddressWatermark {
    fn new(lookahead: Lookahead) -> Self {
        if lookahead.account == 0 && lookahead.subaddress == 0 {
            return Self {
                max_minor_per_account: Vec::new(),
            };
        }
        Self {
            max_minor_per_account: vec![lookahead.subaddress; (lookahead.account + 1) as usize],
        }
    }
}

/// Opaque wrapper caching a `Scanner` across batch scans.
///
/// Holds the pre-registered scanner plus a fingerprint (compressed public spend key)
/// and the lookahead used to build it. On each batch the caller validates these
/// against the current wallet parameters; a match means we skip the expensive
/// `register_subaddresses` step entirely.
pub struct CachedScanner {
    scanner: Scanner,
    lookahead: Lookahead,
    fingerprint: [u8; 32],
    watermark: SubaddressWatermark,
    spend_scalar: Option<Zeroizing<Scalar>>,
}

/// Expand the registered subaddress window if `found` is near the edge.
/// Mimics wallet2's `expand_subaddresses`: after discovering an output at
/// `(major, minor)`, ensure the scanner covers `major + lookahead.account`
/// accounts and `minor + lookahead.subaddress` addresses within that account.
fn expand_subaddresses_if_needed(
    scanner: &mut Scanner,
    watermark: &mut SubaddressWatermark,
    lookahead: Lookahead,
    found: SubaddressIndex,
) -> bool {
    if lookahead.account == 0 && lookahead.subaddress == 0 {
        return false;
    }

    let found_account = found.account();
    let found_address = found.address();
    let mut expanded = false;

    // 1. Account expansion: ensure we cover up to found_account + lookahead.account
    let needed_account = found_account.saturating_add(lookahead.account);
    let current_max_account = watermark.max_minor_per_account.len().saturating_sub(1) as u32;

    if needed_account > current_max_account {
        for acct in (current_max_account + 1)..=needed_account {
            for addr in 0..=lookahead.subaddress {
                if let Some(idx) = SubaddressIndex::new(acct, addr) {
                    scanner.register_subaddress(idx);
                }
            }
            watermark.max_minor_per_account.push(lookahead.subaddress);
        }
        expanded = true;
    }

    // 2. Subaddress expansion within found_account
    let needed_address = found_address.saturating_add(lookahead.subaddress);
    let Some(&current_max_address) = watermark.max_minor_per_account.get(found_account as usize)
    else {
        return expanded;
    };

    if needed_address > current_max_address {
        for addr in (current_max_address + 1)..=needed_address {
            if let Some(idx) = SubaddressIndex::new(found_account, addr) {
                scanner.register_subaddress(idx);
            }
        }
        watermark.max_minor_per_account[found_account as usize] = needed_address;
        expanded = true;
    }

    expanded
}

/// Build the scanner for a wallet whose earlier receipts are known. Pass
/// `WalletState::used_subaddresses` to get the window wallet2 would have.
pub async fn cached_scanner_for_wallet(
    mnemonic: &str,
    lookahead: Lookahead,
    passphrase: &str,
    used_subaddresses: &[(u32, u32)],
) -> Result<CachedScanner, String> {
    let view_only = parse_view_only_keys(mnemonic);
    let (spend_point, view_scalar, spend_scalar) = match view_only {
        Some((view_scalar, spend_point)) => (spend_point, view_scalar, None),
        None => {
            let seed = resolve_seed(mnemonic)?;
            (
                spend_key_from_seed(&seed, passphrase),
                view_key_from_seed(&seed, passphrase),
                Some(spend_key_scalar_from_seed(&seed, passphrase)),
            )
        }
    };
    let mut scanner = oxide_scanner_from_keys(&spend_point, &view_scalar)?;
    register_subaddresses_async(&mut scanner, lookahead).await;
    let mut watermark = SubaddressWatermark::new(lookahead);
    for &(account, address) in used_subaddresses {
        if let Some(found) = SubaddressIndex::new(account, address) {
            expand_subaddresses_if_needed(&mut scanner, &mut watermark, lookahead, found);
        }
    }
    Ok(CachedScanner {
        scanner,
        lookahead,
        fingerprint: spend_point.compress().to_bytes(),
        watermark,
        spend_scalar,
    })
}

impl CachedScanner {
    /// Returns the compressed public spend key fingerprint used to identify this cached scanner.
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
}

impl std::fmt::Debug for CachedScanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedScanner")
            .field("lookahead", &self.lookahead)
            .finish_non_exhaustive()
    }
}

/// Multi-wallet variant of `CachedScanner`.
pub struct CachedScanners {
    entries: Vec<CachedScannerEntry>,
}

impl std::fmt::Debug for CachedScanners {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedScanners")
            .field("count", &self.entries.len())
            .finish_non_exhaustive()
    }
}

struct CachedScannerEntry {
    scanner: Scanner,
    address: String,
    lookahead: Lookahead,
    fingerprint: [u8; 32],
    watermark: SubaddressWatermark,
    spend_scalar: Option<Zeroizing<Scalar>>,
}

/// Resolve a mnemonic with explicit BIP39 passphrase and account index.
/// If 12 words, convert from BIP39 first using the given passphrase and account index.
/// Non-BIP39 seeds (16-word polyseed, 25-word classic) pass through unchanged.
pub fn resolve_seed_bip39(
    mnemonic: &str,
    passphrase: &str,
    account_index: u32,
) -> Result<Seed, String> {
    let word_count = mnemonic.split_whitespace().count();
    if word_count == 12 {
        let legacy =
            crate::bip39_conv::bip39_to_legacy_mnemonic(mnemonic, passphrase, account_index)?;
        Seed::from_string(Zeroizing::new(legacy))
            .map_err(|e| format!("Failed to parse derived legacy seed: {:?}", e))
    } else {
        Seed::from_string(Zeroizing::new(mnemonic.to_string()))
            .map_err(|e| format!("Failed to parse seed: {:?}", e))
    }
}
pub fn resolve_seed(mnemonic: &str) -> Result<Seed, String> {
    resolve_seed_bip39(mnemonic, "", 0)
}

/// Detect `viewonly:<secret_view_key_hex>:<public_spend_key_hex>` sentinel.
/// Returns `(view_scalar, spend_point)` if the prefix matches.
pub fn parse_view_only_keys(seed: &str) -> Option<(Scalar, EdwardsPoint)> {
    let rest = seed.strip_prefix("viewonly:")?;
    let mut parts = rest.splitn(2, ':');
    let view_hex = parts.next()?;
    let spend_hex = parts.next()?;
    if view_hex.len() != 64 || spend_hex.len() != 64 {
        return None;
    }
    let view_bytes: [u8; 32] = hex::decode(view_hex).ok()?.try_into().ok()?;
    let spend_bytes: [u8; 32] = hex::decode(spend_hex).ok()?.try_into().ok()?;
    let view_scalar = Option::<Scalar>::from(Scalar::from_canonical_bytes(view_bytes))?;
    let spend_point = decompress_public_spend_key(spend_bytes)?;
    Some((view_scalar, spend_point))
}

/// Decompress an imported public spend key and check its subgroup.
///
/// `monero-wallet` requires a prime-order point for `ViewPair`. Reject torsioned
/// keys at import so later address derivation cannot panic on them.
pub fn decompress_public_spend_key(bytes: [u8; 32]) -> Option<EdwardsPoint> {
    let point = CompressedEdwardsY(bytes).decompress()?;
    point.is_torsion_free().then_some(point)
}

/// Derive keys for a view-only wallet (no secret spend key available).
pub fn derive_keys_from_view_only(
    secret_view_key_hex: &str,
    public_spend_key_hex: &str,
    network: Network,
) -> Result<DerivedKeys, String> {
    let (view_scalar, spend_point) = parse_view_only_keys(&format!(
        "viewonly:{}:{}",
        secret_view_key_hex, public_spend_key_hex
    ))
    .ok_or_else(|| "Invalid view-only key hex".to_string())?;
    let view_point: EdwardsPoint = &view_scalar * ED25519_BASEPOINT_TABLE;
    let address = standard_address(network, spend_point, view_point);
    Ok(DerivedKeys {
        secret_spend_key: None,
        secret_view_key: Zeroizing::new(view_scalar.to_bytes()),
        public_spend_key: spend_point.compress().to_bytes(),
        public_view_key: view_point.compress().to_bytes(),
        address,
    })
}

/// Derive address for a view-only wallet.
pub fn derive_address_from_view_only(
    secret_view_key_hex: &str,
    public_spend_key_hex: &str,
    network: Network,
) -> Result<MoneroAddress, String> {
    let (view_scalar, spend_point) = parse_view_only_keys(&format!(
        "viewonly:{}:{}",
        secret_view_key_hex, public_spend_key_hex
    ))
    .ok_or_else(|| "Invalid view-only key hex".to_string())?;
    let view_point: EdwardsPoint = &view_scalar * ED25519_BASEPOINT_TABLE;
    let address = standard_address(network, spend_point, view_point);
    Ok(address)
}

/// Seed format to generate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedFormat {
    Classic,
    Polyseed,
    Bip39,
}

pub fn generate_seed(format: SeedFormat) -> Result<String, String> {
    // Use thread_rng which works in both native and WASM contexts
    let mut rng = rand::thread_rng();

    let seed = match format {
        SeedFormat::Classic => Seed::new(&mut rng, Language::English),
        SeedFormat::Polyseed => Seed::new_polyseed(&mut rng),
        SeedFormat::Bip39 => return crate::bip39_conv::generate_bip39(),
    };

    Ok(Seed::to_string(&seed).to_string())
}

pub fn seed_birthday(mnemonic: &str) -> Option<u64> {
    let seed = Seed::from_string(Zeroizing::new(mnemonic.to_string())).ok()?;
    seed.birthday()
}

pub fn validate_seed(mnemonic: &str) -> Result<(), String> {
    if mnemonic.trim().is_empty() {
        return Err("Seed phrase is empty".to_string());
    }

    let word_count = mnemonic.split_whitespace().count();
    if word_count == 12 {
        return crate::bip39_conv::validate_bip39(mnemonic);
    }

    Seed::from_string(Zeroizing::new(mnemonic.to_string()))
        .map(|_| ())
        .map_err(|e| format!("Invalid seed phrase: {:?}", e))
}

fn address_from_seed(seed: &Seed, network: Network, passphrase: &str) -> MoneroAddress {
    let spend_point = spend_key_from_seed(seed, passphrase);
    let view_scalar = view_key_from_seed(seed, passphrase);
    let view_point: EdwardsPoint = &view_scalar * ED25519_BASEPOINT_TABLE;

    standard_address(network, spend_point, view_point)
}

pub fn derive_address(
    mnemonic: &str,
    network: Network,
    passphrase: &str,
) -> Result<MoneroAddress, String> {
    if let Some((view_hex, spend_hex)) = mnemonic.strip_prefix("viewonly:").and_then(|r| {
        let mut parts = r.splitn(2, ':');
        Some((parts.next()?, parts.next()?))
    }) {
        return derive_address_from_view_only(view_hex, spend_hex, network);
    }
    let seed = resolve_seed(mnemonic)?;
    Ok(address_from_seed(&seed, network, passphrase))
}

pub fn derive_subaddress(
    mnemonic: &str,
    network: Network,
    account: u32,
    address_index: u32,
    passphrase: &str,
) -> Result<MoneroAddress, String> {
    let (spend_point, view_scalar) = if let Some((vs, sp)) = parse_view_only_keys(mnemonic) {
        (sp, vs)
    } else {
        let seed = resolve_seed(mnemonic)?;
        let spend: [u8; 32] = *seed.key_bytes_with_passphrase(passphrase);
        let spend_scalar = Scalar::from_bytes_mod_order(spend);
        let sp: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;
        let view: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
        let vs = Scalar::from_bytes_mod_order(view);
        (sp, vs)
    };

    let view_pair = OxideViewPair::new(
        OxidePoint::from(spend_point),
        Zeroizing::new(OxideScalar::from(view_scalar)),
    )
    .map_err(|e| format!("Invalid view keys: {}", e))?;

    if account == 0 && address_index == 0 {
        return Ok(view_pair.legacy_address(network));
    }

    let subaddress_index = SubaddressIndex::new(account, address_index).ok_or_else(|| {
        format!(
            "Invalid subaddress index: ({}, {}). Note: (0, 0) should use derive_address() instead.",
            account, address_index
        )
    })?;

    Ok(view_pair.subaddress(network, subaddress_index))
}

pub fn derive_keys(
    mnemonic: &str,
    network: Network,
    passphrase: &str,
) -> Result<DerivedKeys, String> {
    if let Some((view_hex, spend_hex)) = mnemonic.strip_prefix("viewonly:").and_then(|r| {
        let mut parts = r.splitn(2, ':');
        Some((parts.next()?, parts.next()?))
    }) {
        return derive_keys_from_view_only(view_hex, spend_hex, network);
    }

    let seed = resolve_seed(mnemonic)?;

    let spend: [u8; 32] = *seed.key_bytes_with_passphrase(passphrase);
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * ED25519_BASEPOINT_TABLE;

    let view: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    let view_scalar = Scalar::from_bytes_mod_order(view);
    let view_point: EdwardsPoint = &view_scalar * ED25519_BASEPOINT_TABLE;

    let address = standard_address(network, spend_point, view_point);

    Ok(DerivedKeys {
        secret_spend_key: Some(Zeroizing::new(spend_scalar.to_bytes())),
        secret_view_key: Zeroizing::new(view_scalar.to_bytes()),
        public_spend_key: spend_point.compress().to_bytes(),
        public_view_key: view_point.compress().to_bytes(),
        address,
    })
}

/// Require one spent status per requested key image.
///
/// Callers pair statuses with key images by position. A short reply would leave
/// outputs unchecked and apparently unspent; a long reply has unrequested entries.
/// Monero's wallet2 rejects both cases.
fn check_spent_status(requested: usize, statuses: Vec<u32>) -> Result<Vec<u32>, String> {
    if statuses.len() != requested {
        return Err(format!(
            "daemon returned wrong response for is_key_image_spent: {} statuses for {} key images",
            statuses.len(),
            requested
        ));
    }
    Ok(statuses)
}

pub async fn key_image_spent_status<R: RpcConnection>(
    rpc: &Rpc<R>,
    key_images: &[String],
) -> Result<Vec<u32>, String> {
    #[derive(serde::Serialize, Debug)]
    struct Req {
        key_images: Vec<String>,
    }
    #[derive(serde::Deserialize, Debug)]
    struct Resp {
        spent_status: Vec<u32>,
    }

    let params = Req {
        key_images: key_images.to_vec(),
    };

    let resp: Resp = rpc
        .rpc_call("is_key_image_spent", Some(params))
        .await
        .map_err(|e| format!("RPC error: {:?}", e))?;
    check_spent_status(key_images.len(), resp.spent_status)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn is_key_image_spent(node_url: &str, key_images: &[String]) -> Result<Vec<u32>, String> {
    crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
    key_image_spent_status(&rpc, key_images).await
}

pub async fn daemon_height<R: RpcConnection>(rpc: &Rpc<R>) -> Result<u64, String> {
    let height = rpc
        .get_height()
        .await
        .map_err(|e| format!("Failed to get height: {:?}", e))?;
    Ok(height as u64)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn get_daemon_height(node_url: &str) -> Result<u64, String> {
    crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
    daemon_height(&rpc).await
}

/// Scan a single block for outputs belonging to the given wallet.
///
/// This function is generic over the RPC connection type, allowing it to work
/// with native or wasm RPC connections.
#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_block_for_outputs_with_url(
    node_url: &str,
    block_height: u64,
    mnemonic: &str,
    network: Network,
    passphrase: &str,
) -> Result<BlockScanResult, String> {
    crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
    scan_block_for_outputs_with_url_and_lookahead(
        node_url,
        block_height,
        mnemonic,
        network,
        DEFAULT_LOOKAHEAD,
        passphrase,
    )
    .await
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_block_for_outputs_with_url_and_lookahead(
    node_url: &str,
    block_height: u64,
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    passphrase: &str,
) -> Result<BlockScanResult, String> {
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    scan_block_for_outputs_with_lookahead(
        &rpc,
        block_height,
        mnemonic,
        network,
        lookahead,
        passphrase,
    )
    .await
}

/// Build a `monero-wallet` scanner from dalek key material.
pub(crate) fn oxide_scanner_from_keys(
    spend_point: &EdwardsPoint,
    view_scalar: &Scalar,
) -> Result<Scanner, String> {
    let spend = OxidePoint::from(*spend_point);
    let view = Zeroizing::new(OxideScalar::from(*view_scalar));
    let view_pair =
        OxideViewPair::new(spend, view).map_err(|e| format!("Invalid view keys: {}", e))?;
    Ok(Scanner::new(view_pair))
}

/// Parse a full transaction blob into the hash and pruned representation used
/// for scanning. Returns `None` when the blob isn't one whole transaction.
pub(crate) fn parse_full_tx_blob(bytes: &[u8]) -> Option<([u8; 32], OxideTransaction<Pruned>)> {
    let mut cursor = std::io::Cursor::new(bytes);
    let tx = OxideTransaction::<NotPruned>::read(&mut cursor).ok()?;
    if cursor.position() as usize != bytes.len() {
        return None;
    }
    Some((tx.hash(), OxideTransaction::<Pruned>::from(tx)))
}

/// Parse a pruned `/getblocks.bin` transaction (blob, then prunable hash). The
/// hash is `None` when it cannot be computed (zero prunable hash, or version 1).
pub(crate) fn parse_pruned_tx_blob(
    bytes: &[u8],
) -> Option<(Option<[u8; 32]>, OxideTransaction<Pruned>)> {
    use monero_oxide::io::VarInt;

    let (blob, prunable_hash) = bytes.split_at(bytes.len().checked_sub(32)?);
    let mut rest = blob;
    let tx = OxideTransaction::<Pruned>::read(&mut rest).ok()?;
    if !rest.is_empty() {
        return None;
    }
    if !matches!(tx, OxideTransaction::V2 { .. }) {
        return Some((None, tx));
    }

    let mut after_prefix = blob;
    let version = <u64 as VarInt>::read(&mut after_prefix).ok()?;
    OxideTransactionPrefix::read(&mut after_prefix, version).ok()?;
    let prefix_len = blob.len() - after_prefix.len();

    if prunable_hash == [0u8; 32] {
        return Some((None, tx));
    }
    let mut hashes = Vec::with_capacity(96);
    hashes.extend_from_slice(&Keccak256::digest(&blob[..prefix_len]));
    hashes.extend_from_slice(&Keccak256::digest(&blob[prefix_len..]));
    hashes.extend_from_slice(prunable_hash);
    Some((Some(Keccak256::digest(&hashes).into()), tx))
}

/// Check a block's `/getblocks.bin` output indices and return the global index
/// of its first RingCT output. The checks must not depend on whether the block
/// pays the wallet, or the outcome tells the node which blocks do.
fn first_ringct_index_from_rpc(
    block: &OxideBlock,
    txs: &[OxideTransaction<Pruned>],
    indices: Option<&BlockOutputIndices>,
) -> Result<Option<u64>, String> {
    let height = block.number();
    let miner = block.miner_transaction();
    let miner_outputs = if miner.version() == 2 {
        miner.prefix().outputs.len() as u64
    } else {
        0
    };
    let is_ringct = |tx: &&OxideTransaction<Pruned>| matches!(tx, OxideTransaction::V2 { .. });
    if miner_outputs == 0
        && txs
            .iter()
            .filter(is_ringct)
            .all(|tx| tx.prefix().outputs.is_empty())
    {
        return Ok(None);
    }

    let entries = indices
        .ok_or_else(|| format!("Node sent no output indices for block {height}"))?
        .indices
        .as_slice();
    let (miner_entry, tx_entries) = if entries.len() == 1 + txs.len() {
        (Some(&entries[0]), &entries[1..])
    } else if entries.len() == txs.len() {
        (None, entries)
    } else {
        return Err(format!(
            "Block {height} has {} transactions but the node sent output indices for {}",
            1 + txs.len(),
            entries.len()
        ));
    };

    let miner_indices = miner_entry
        .filter(|_| miner.version() == 2)
        .map(|entry| entry.indices.as_slice())
        .filter(|indices| !indices.is_empty());
    let ringct = std::iter::once((miner_outputs, miner_indices)).chain(
        txs.iter()
            .zip(tx_entries)
            .filter(|(tx, _)| is_ringct(tx))
            .map(|(tx, entry)| {
                (
                    tx.prefix().outputs.len() as u64,
                    Some(entry.indices.as_slice()),
                )
            }),
    );

    let mut first = None;
    let mut next: Option<u64> = None;
    let mut unindexed = 0u64;
    for (outputs, sent) in ringct {
        let Some(sent) = sent else {
            unindexed += outputs;
            continue;
        };
        if sent.len() as u64 != outputs {
            return Err(format!(
                "Block {height}: the node sent {} output indices for a transaction with {} \
                 outputs",
                sent.len(),
                outputs
            ));
        }
        for index in sent {
            match next {
                Some(expected) if *index != expected => {
                    return Err(format!(
                        "Block {height}: output index {index} does not follow {}",
                        expected.wrapping_sub(1)
                    ));
                }
                Some(_) => {}
                None => {
                    first = Some(index.checked_sub(unindexed).ok_or_else(|| {
                        format!("Block {height}: output index {index} is too low for its position")
                    })?);
                }
            }
            next = Some(index.checked_add(1).ok_or_else(|| {
                format!("Block {height}: output index {index} leaves no room for the next")
            })?);
        }
    }

    first.map(Some).ok_or_else(|| {
        format!("Block {height}: the output indices sent do not fix its first RingCT index")
    })
}

/// The per-amount global indices of the outputs of each version 1 transaction
/// of a block, by hash; `None` for a miner transaction sent without its entry.
fn version_1_output_indices<'a>(
    block: &OxideBlock,
    txs: &[([u8; 32], OxideTransaction<Pruned>)],
    indices: Option<&'a BlockOutputIndices>,
) -> Result<HashMap<[u8; 32], Option<&'a [u64]>>, String> {
    let height = block.number();
    let miner = block.miner_transaction();
    let miner_hash = (miner.version() == 1).then(|| miner.hash());
    let is_v1 = |tx: &OxideTransaction<Pruned>| matches!(tx, OxideTransaction::V1 { .. });
    let any_v1_tx = txs.iter().any(|(_, tx)| is_v1(tx));
    let mut found: HashMap<[u8; 32], Option<&[u64]>> =
        miner_hash.map(|hash| (hash, None)).into_iter().collect();
    if found.is_empty() && !any_v1_tx {
        return Ok(found);
    }

    let entries = indices.map_or(&[][..], |indices| indices.indices.as_slice());
    let (miner_entry, tx_entries) = if entries.len() == 1 + txs.len() {
        (entries.first(), &entries[1..])
    } else if entries.len() == txs.len() {
        (None, entries)
    } else if any_v1_tx {
        return Err(format!(
            "Block {height} has {} transactions but the node sent output indices for {}",
            1 + txs.len(),
            entries.len()
        ));
    } else {
        return Ok(found);
    };

    let miner_entry = miner_hash.zip(
        miner_entry
            .map(|entry| entry.indices.as_slice())
            .filter(|entry| !entry.is_empty()),
    );
    let miner_outputs = miner.prefix().outputs.as_slice();
    let version_1 = miner_entry
        .map(|(hash, entry)| (hash, miner_outputs, entry))
        .into_iter()
        .chain(
            txs.iter()
                .zip(tx_entries)
                .filter(|((_, tx), _)| is_v1(tx))
                .map(|((hash, tx), entry)| {
                    (
                        *hash,
                        tx.prefix().outputs.as_slice(),
                        entry.indices.as_slice(),
                    )
                }),
        );

    let mut next: HashMap<u64, u64> = HashMap::new();
    for (hash, outputs, sent) in version_1 {
        if sent.len() != outputs.len() {
            return Err(format!(
                "Block {height}: the node sent {} output indices for version 1 transaction {} \
                 with {} outputs",
                sent.len(),
                hex::encode(hash),
                outputs.len()
            ));
        }
        for (output, index) in outputs.iter().zip(sent) {
            let amount = output.amount.unwrap_or(0);
            if let Some(expected) = next.get(&amount) {
                if index != expected {
                    return Err(format!(
                        "Block {height}: output index {index} for amount {amount} does not \
                         follow {}",
                        expected.wrapping_sub(1)
                    ));
                }
            }
            let following = index.checked_add(1).ok_or_else(|| {
                format!("Block {height}: output index {index} leaves no room for the next")
            })?;
            next.insert(amount, following);
        }
        found.insert(hash, Some(sent));
    }
    Ok(found)
}

/// Drop zero-amount outputs, which wallet2 does not record (wallet2.cpp:2272-2277).
fn without_zero_amounts(mut outputs: Vec<OxideWalletOutput>) -> Vec<OxideWalletOutput> {
    outputs.retain(|output| output.commitment().amount != 0);
    outputs
}

/// Scan one fully parsed block for the scanner's outputs.
fn scan_parsed_block(
    scanner: &mut Scanner,
    block: &OxideBlock,
    txs: &[OxideTransaction<Pruned>],
    first_ringct_index: u64,
) -> Result<Vec<OxideWalletOutput>, String> {
    scanner
        .scan(ScannableBlock {
            block: block.clone(),
            transactions: txs.to_vec(),
            output_index_for_first_ringct_output: Some(first_ringct_index),
        })
        .map(|timelocked| without_zero_amounts(timelocked.ignore_additional_timelock()))
        .map_err(|e| format!("Failed to scan block {}: {}", block.number(), e))
}

/// Scan a fully parsed block, expanding the subaddress lookahead window and
/// rescanning until no output lands near the edge of the window. This mirrors
/// the per-transaction expansion the previous scanner applied within a block.
fn scan_parsed_block_expanding(
    scanner: &mut Scanner,
    watermark: &mut SubaddressWatermark,
    lookahead: Lookahead,
    block: &OxideBlock,
    txs: &[OxideTransaction<Pruned>],
    first_ringct_index: u64,
) -> Result<Vec<OxideWalletOutput>, String> {
    loop {
        let outputs = scan_parsed_block(scanner, block, txs, first_ringct_index)?;
        let mut expanded = false;
        for output in &outputs {
            if let Some(found) = output.subaddress() {
                expanded |= expand_subaddresses_if_needed(scanner, watermark, lookahead, found);
            }
        }
        if !expanded {
            return Ok(outputs);
        }
    }
}

/// Scan a single transaction outside its block context (mempool). The RingCT
/// indices of the returned outputs are relative to the transaction, not the chain.
pub fn scan_single_transaction(
    scanner: &mut Scanner,
    tx_hash: [u8; 32],
    tx: &OxideTransaction<Pruned>,
) -> Result<Vec<OxideWalletOutput>, String> {
    let miner_transaction = OxideTransaction::<NotPruned>::V2 {
        prefix: OxideTransactionPrefix {
            additional_timelock: OxideTimelock::None,
            inputs: vec![OxideInput::Gen(0)],
            outputs: vec![],
            extra: vec![],
        },
        proofs: None,
    };
    let header = OxideBlockHeader {
        hardfork_version: 16,
        hardfork_signal: 0,
        timestamp: 0,
        previous: [0; 32],
        nonce: 0,
    };
    let block = OxideBlock::new(header, miner_transaction, vec![tx_hash])
        .ok_or_else(|| "Failed to build synthetic scan block".to_string())?;
    scanner
        .scan(ScannableBlock {
            block,
            transactions: vec![tx.clone()],
            output_index_for_first_ringct_output: Some(0),
        })
        .map(|timelocked| without_zero_amounts(timelocked.ignore_additional_timelock()))
        .map_err(|e| format!("Failed to scan transaction {}: {}", hex::encode(tx_hash), e))
}

/// Map a `monero-wallet` payment ID into the hex representation the previous
/// scanner reported: encrypted IDs of all zeroes mean "no payment ID".
fn oxide_payment_id_hex(payment_id: PaymentId) -> Option<String> {
    match payment_id {
        PaymentId::Unencrypted(id) => Some(hex::encode(id)),
        PaymentId::Encrypted(id) if id == [0; 8] => None,
        PaymentId::Encrypted(id) => Some(hex::encode(id)),
    }
}

fn map_oxide_output(
    output: &OxideWalletOutput,
    block_height: u64,
    miner_tx_hash: [u8; 32],
    key_image_scalar: Option<&Scalar>,
    global_index: Option<u64>,
    pre_ringct: bool,
) -> Result<WalletOutput, String> {
    let output_index = output.index_in_transaction();
    let key_offset_bytes = <[u8; 32]>::from(output.key_offset());
    let commitment = output.commitment();
    let amount = commitment.amount;

    let key_image = match key_image_scalar {
        Some(spend_scalar) => {
            let key_offset: Scalar = output.key_offset().into();
            let key_image_point = calculate_key_image(spend_scalar, &key_offset);
            hex::encode(key_image_point.compress().to_bytes())
        }
        None => String::new(),
    };

    Ok(WalletOutput {
        tx_hash: hex::encode(output.transaction()),
        output_index,
        amount,
        key: hex::encode(output.key().compress().to_bytes()),
        key_offset: hex::encode(key_offset_bytes),
        commitment_mask: hex::encode(<[u8; 32]>::from(commitment.mask)),
        subaddress_index: output
            .subaddress()
            .map(|idx| (idx.account(), idx.address())),
        payment_id: output.payment_id().and_then(oxide_payment_id_hex),
        received_output_bytes: hex::encode(output.serialize()),
        global_index,
        ring: None,
        block_height,
        spent: false,
        spent_height: None,
        key_image,
        is_coinbase: output.transaction() == miner_tx_hash,
        frozen: false,
        pre_ringct,
    })
}

/// Convert a block's outputs; a wallet output without a global index is refused.
fn map_block_outputs(
    scanned: &[OxideWalletOutput],
    block_height: u64,
    miner_tx_hash: [u8; 32],
    key_image_scalar: Option<&Scalar>,
    first_ringct_index: Option<u64>,
    version_1: &HashMap<[u8; 32], Option<&[u64]>>,
) -> Result<Vec<WalletOutput>, String> {
    let mut outputs = Vec::with_capacity(scanned.len());
    for output in scanned {
        let (global_index, pre_ringct) = match version_1.get(&output.transaction()) {
            Some(indices) => (
                indices
                    .and_then(|indices| {
                        usize::try_from(output.index_in_transaction())
                            .ok()
                            .and_then(|position| indices.get(position))
                    })
                    .copied(),
                true,
            ),
            None => (
                first_ringct_index.map(|_| output.index_on_blockchain()),
                false,
            ),
        };
        if global_index.is_none() {
            return Err(format!(
                "Block {block_height} contains a wallet output but no authoritative global \
                 output index; refuse to store an output that cannot be spent privately"
            ));
        }
        outputs.push(map_oxide_output(
            output,
            block_height,
            miner_tx_hash,
            key_image_scalar,
            global_index,
            pre_ringct,
        )?);
    }
    Ok(outputs)
}

pub async fn scan_block_for_outputs<R: RpcConnection>(
    rpc: &Rpc<R>,
    block_height: u64,
    mnemonic: &str,
    network: Network,
    passphrase: &str,
) -> Result<BlockScanResult, String> {
    scan_block_for_outputs_with_lookahead(
        rpc,
        block_height,
        mnemonic,
        network,
        DEFAULT_LOOKAHEAD,
        passphrase,
    )
    .await
}

pub async fn scan_block_for_outputs_with_lookahead<R: RpcConnection>(
    rpc: &Rpc<R>,
    block_height: u64,
    mnemonic: &str,
    _network: Network,
    lookahead: Lookahead,
    passphrase: &str,
) -> Result<BlockScanResult, String> {
    if parse_view_only_keys(mnemonic).is_none() {
        resolve_seed(mnemonic)?;
    }
    // `/getblocks.bin` gives the whole block's output indexes; asking per matching
    // transaction would tell the node which ones pay the wallet.
    scan_blocks_batch(
        rpc,
        block_height,
        mnemonic,
        _network,
        lookahead,
        false,
        passphrase,
    )
    .await?
    .into_iter()
    .find(|result| result.block_height == block_height)
    .ok_or_else(|| format!("Node did not return requested block {block_height}"))
}

/// Reject a batch that starts after the requested height.
///
/// An earlier start is valid when the daemon reports a fork point. A later start
/// would let the wallet advance past blocks it has never scanned. Checking each
/// block's declared height is insufficient: the daemon supplies both the blocks
/// and the response height, and this scanner does not verify proof of work.
fn check_batch_start_height(
    returned_start_height: u64,
    requested_start_height: u64,
) -> Result<(), String> {
    if returned_start_height > requested_start_height {
        return Err(format!(
            "Node returned a batch starting at {} when {} was requested; \
             a daemon may rewind to a fork point but never skip forward",
            returned_start_height, requested_start_height
        ));
    }
    Ok(())
}

fn check_block_follows_parent(
    block: &OxideBlock,
    block_height: u64,
    previous_hash: Option<[u8; 32]>,
) -> Result<(), String> {
    if let Some(expected) = previous_hash {
        if block.header.previous != expected {
            return Err(format!(
                "Block at height {} does not follow the block before it in the batch: \
                 expected parent {}, got {}",
                block_height,
                hex::encode(expected),
                hex::encode(block.header.previous)
            ));
        }
    }
    Ok(())
}

/// Require one transaction blob per hash listed in the block. Extra blobs must
/// not be scanned as though they belonged to the block.
fn check_block_transaction_count(
    block: &OxideBlock,
    blob_count: usize,
    block_height: u64,
) -> Result<(), String> {
    if blob_count != block.transactions.len() {
        return Err(format!(
            "Block at height {} lists {} transactions but {} blobs were sent with it",
            block_height,
            block.transactions.len(),
            blob_count
        ));
    }
    Ok(())
}

/// Parse transaction blobs and check each hash against its position in the block.
/// A blob that does not parse fails the batch, so a node cannot hide one.
fn parse_block_transaction_blobs(
    block: &OxideBlock,
    blobs: &[Vec<u8>],
    pruned: bool,
    block_height: u64,
) -> Result<Vec<([u8; 32], OxideTransaction<Pruned>)>, String> {
    let mut parsed = Vec::with_capacity(blobs.len());

    for (index, blob) in blobs.iter().enumerate() {
        let parsed_blob = if pruned {
            parse_pruned_tx_blob(blob)
        } else {
            parse_full_tx_blob(blob).map(|(hash, tx)| (Some(hash), tx))
        };
        match parsed_blob {
            Some((hash, tx)) => {
                let listed = block.transactions.get(index).ok_or_else(|| {
                    format!(
                        "Block at height {} was sent more transaction blobs than it lists",
                        block_height
                    )
                })?;
                let hash = hash.unwrap_or(*listed);
                if hash != *listed {
                    return Err(format!(
                        "Block at height {} lists transaction {} at position {}, but the blob \
                         sent in its place is transaction {}",
                        block_height,
                        hex::encode(listed),
                        index,
                        hex::encode(hash)
                    ));
                }
                parsed.push((hash, tx));
            }
            None => {
                return Err(format!(
                    "Block at height {} carries a transaction blob at position {} that does \
                     not parse",
                    block_height, index
                ));
            }
        }
    }

    Ok(parsed)
}

/// Scan a batch of blocks fetched via `/getblocks.bin` for outputs belonging to the wallet.
///
/// A node can tell this request from wallet2's; `Refresh` sends wallet2's.
pub async fn scan_blocks_batch<R: RpcConnection>(
    rpc: &Rpc<R>,
    start_height: u64,
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    prune: bool,
    passphrase: &str,
) -> Result<Vec<BlockScanResult>, String> {
    // The id of the block below `start_height`: monerod answers a list that
    // starts with its top block with no blocks.
    let anchor_height = start_height.saturating_sub(1);
    let known_hash = rpc
        .get_block_hash(anchor_height as usize)
        .await
        .map_err(|e| format!("Failed to get block hash at {}: {:?}", anchor_height, e))?;

    // Fetch batch of blocks via binary RPC (up to ~1000 blocks per call)
    let response = rpc
        .get_blocks_fast(&[known_hash], start_height, prune)
        .await
        .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;

    let (results, _cached) = process_batch_response(
        response,
        start_height,
        mnemonic,
        network,
        lookahead,
        None,
        passphrase,
    )
    .await?;
    Ok(results)
}

/// Process a batch of blocks fetched via `/getblocks.bin` and scan them for outputs.
///
/// This is the core scanning logic, separated from the RPC layer for testability.
/// When `cached` is provided and its fingerprint + lookahead match the current wallet,
/// the expensive `register_subaddresses` step is skipped entirely.
///
/// `requested_start_height` records the request independently of the daemon's
/// response. `check_batch_start_height` compares the two before scanning.
pub async fn process_batch_response(
    response: GetBlocksFastResponse,
    requested_start_height: u64,
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    cached: Option<CachedScanner>,
    passphrase: &str,
) -> Result<(Vec<BlockScanResult>, CachedScanner), String> {
    scan_batch_response(
        response,
        requested_start_height,
        0,
        mnemonic,
        network,
        lookahead,
        cached,
        passphrase,
    )
    .await
}

/// `process_batch_response` that reports blocks below `scan_from` without
/// scanning them, as wallet2 skips them (`should_skip_block`).
#[allow(clippy::too_many_arguments)]
async fn scan_batch_response(
    response: GetBlocksFastResponse,
    requested_start_height: u64,
    scan_from: u64,
    mnemonic: &str,
    _network: Network,
    lookahead: Lookahead,
    cached: Option<CachedScanner>,
    passphrase: &str,
) -> Result<(Vec<BlockScanResult>, CachedScanner), String> {
    check_batch_start_height(response.start_height, requested_start_height)?;
    let view_only = parse_view_only_keys(mnemonic);
    let seed_opt = if view_only.is_none() {
        Some(resolve_seed(mnemonic)?)
    } else {
        None
    };
    let spend_point = view_only
        .as_ref()
        .map(|(_, sp)| *sp)
        .unwrap_or_else(|| spend_key_from_seed(seed_opt.as_ref().unwrap(), passphrase));
    let view_scalar = view_only
        .as_ref()
        .map(|(vs, _)| *vs)
        .unwrap_or_else(|| view_key_from_seed(seed_opt.as_ref().unwrap(), passphrase));
    let spend_scalar = view_only
        .is_none()
        .then(|| spend_key_scalar_from_seed(seed_opt.as_ref().unwrap(), passphrase));
    let fingerprint = spend_point.compress().to_bytes();

    let mut scanner = if let Some(c) = cached {
        if c.fingerprint == fingerprint && c.lookahead == lookahead {
            c
        } else {
            drop(c);
            let mut s = oxide_scanner_from_keys(&spend_point, &view_scalar)?;
            register_subaddresses_async(&mut s, lookahead).await;
            CachedScanner {
                scanner: s,
                lookahead,
                fingerprint,
                watermark: SubaddressWatermark::new(lookahead),
                spend_scalar,
            }
        }
    } else {
        let mut s = oxide_scanner_from_keys(&spend_point, &view_scalar)?;
        register_subaddresses_async(&mut s, lookahead).await;
        CachedScanner {
            scanner: s,
            lookahead,
            fingerprint,
            watermark: SubaddressWatermark::new(lookahead),
            spend_scalar,
        }
    };

    let daemon_height = response.current_height;
    let mut results = Vec::with_capacity(response.blocks.len());
    let mut previous_block_hash: Option<[u8; 32]> = None;

    #[cfg(target_arch = "wasm32")]
    let mut last_yield_ms = js_sys::Date::now();

    for (block_idx, block_entry) in response.blocks.iter().enumerate() {
        let block_height = response.start_height + block_idx as u64;

        let block = OxideBlock::read::<&[u8]>(&mut block_entry.block.as_ref())
            .map_err(|e| format!("Failed to parse block at height {}: {:?}", block_height, e))?;

        if block.number() != block_height as usize {
            return Err(format!(
                "Block height mismatch: expected {}, got {}",
                block_height,
                block.number()
            ));
        }
        check_block_follows_parent(&block, block_height, previous_block_hash)?;
        check_block_transaction_count(&block, block_entry.txs.len(), block_height)?;
        previous_block_hash = Some(block.hash());

        let block_timestamp = block.header.timestamp;
        let block_hash = hex::encode(block.hash());

        let miner_tx_hash = block.miner_transaction().hash();
        let miner_tx = OxideTransaction::<Pruned>::from(block.miner_transaction().clone());
        let tx_count = 1 + block_entry.txs.len();

        {
            let parsed_txs = parse_block_transaction_blobs(
                &block,
                &block_entry.txs,
                block_entry.pruned,
                block_height,
            )?;

            let mut spent_key_images = Vec::new();
            let mut spent_key_image_tx_hashes: Vec<String> = Vec::new();
            let mut spent_key_image_rings: Vec<Vec<u64>> = Vec::new();
            for (tx_hash, tx) in std::iter::once((miner_tx_hash, &miner_tx))
                .chain(parsed_txs.iter().map(|(hash, tx)| (*hash, tx)))
            {
                let tx_hash_hex = hex::encode(tx_hash);
                for input in &tx.prefix().inputs {
                    if let OxideInput::ToKey {
                        key_image,
                        key_offsets,
                        ..
                    } = input
                    {
                        spent_key_images.push(hex::encode(key_image.to_bytes()));
                        spent_key_image_tx_hashes.push(tx_hash_hex.clone());
                        spent_key_image_rings.push(key_offsets.clone());
                    }
                }
            }

            let scan_txs: Vec<OxideTransaction<Pruned>> =
                parsed_txs.iter().map(|(_, tx)| tx.clone()).collect();
            let first_ringct_index = first_ringct_index_from_rpc(
                &block,
                &scan_txs,
                response.output_indices.get(block_idx),
            )?;
            let version_1 = version_1_output_indices(
                &block,
                &parsed_txs,
                response.output_indices.get(block_idx),
            )?;
            if block_height < scan_from {
                results.push(BlockScanResult {
                    previous_block_hash: hex::encode(block.header.previous),
                    block_height,
                    block_hash,
                    block_timestamp,
                    tx_count,
                    outputs: Vec::new(),
                    daemon_height,
                    spent_key_images: Vec::new(),
                    spent_key_image_tx_hashes: Vec::new(),
                    spent_key_image_rings: Vec::new(),
                });
                continue;
            }
            let scanned = scan_parsed_block_expanding(
                &mut scanner.scanner,
                &mut scanner.watermark,
                scanner.lookahead,
                &block,
                &scan_txs,
                first_ringct_index.unwrap_or(0),
            )?;

            let key_image_scalar = scanner.spend_scalar.as_ref().map(|scalar| &**scalar);
            let outputs = map_block_outputs(
                &scanned,
                block_height,
                miner_tx_hash,
                key_image_scalar,
                first_ringct_index,
                &version_1,
            )?;

            results.push(BlockScanResult {
                previous_block_hash: hex::encode(block.header.previous),
                block_height,
                block_hash,
                block_timestamp,
                tx_count,
                outputs,
                daemon_height,
                spent_key_images,
                spent_key_image_tx_hashes,
                spent_key_image_rings,
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            let now = js_sys::Date::now();
            if now - last_yield_ms >= 16.0 {
                yield_to_event_loop().await;
                last_yield_ms = js_sys::Date::now();
            }
        }
    }

    Ok((results, scanner))
}

/// Convenience wrapper for batch scanning with URL-based RPC creation.
#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_blocks_batch_with_url(
    node_url: &str,
    start_height: u64,
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    prune: bool,
    passphrase: &str,
) -> Result<Vec<BlockScanResult>, String> {
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    scan_blocks_batch(
        &rpc,
        start_height,
        mnemonic,
        network,
        lookahead,
        prune,
        passphrase,
    )
    .await
}

/// Scan a batch of blocks for outputs belonging to multiple wallets.
///
/// Fetches up to ~1000 blocks in a single `/getblocks.bin` call, parses them once,
/// then scans each wallet against all blocks sequentially. Returns one
/// `MultiWalletScanResult` per block.
pub async fn scan_blocks_batch_multi_wallet<R: RpcConnection>(
    rpc: &Rpc<R>,
    start_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
    prune: bool,
) -> Result<Vec<MultiWalletScanResult>, String> {
    if wallet_configs.is_empty() {
        return Err("No wallet configurations provided".to_string());
    }

    // Get a known block hash for the daemon to find the fork point
    let anchor_height = start_height.saturating_sub(1);
    let known_hash = rpc
        .get_block_hash(anchor_height as usize)
        .await
        .map_err(|e| format!("Failed to get block hash at {}: {:?}", anchor_height, e))?;

    // Fetch batch of blocks via binary RPC
    let response = rpc
        .get_blocks_fast(&[known_hash], start_height, prune)
        .await
        .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;

    let (results, _cached) =
        process_batch_multi_wallet_response(response, start_height, wallet_configs, None).await?;
    Ok(results)
}

/// Process a batch of blocks fetched via `/getblocks.bin` and scan for multiple wallets.
///
/// This is the core multi-wallet scanning logic, separated from the RPC layer for testability.
/// When `cached` is provided and all fingerprints + lookaheads match, the expensive
/// subaddress registration is skipped.
pub async fn process_batch_multi_wallet_response(
    response: GetBlocksFastResponse,
    requested_start_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
    cached: Option<CachedScanners>,
) -> Result<(Vec<MultiWalletScanResult>, CachedScanners), String> {
    if wallet_configs.is_empty() {
        return Err("No wallet configurations provided".to_string());
    }
    check_batch_start_height(response.start_height, requested_start_height)?;

    let daemon_height = response.current_height;

    // Derive fingerprints for each wallet (cheap hash ops)
    let mut config_fingerprints = Vec::with_capacity(wallet_configs.len());
    for config in &wallet_configs {
        if let Some((_vs, sp)) = parse_view_only_keys(&config.mnemonic) {
            config_fingerprints.push(sp.compress().to_bytes());
        } else {
            let seed = resolve_seed(&config.mnemonic)?;
            let spend_point = spend_key_from_seed(&seed, &config.passphrase);
            config_fingerprints.push(spend_point.compress().to_bytes());
        }
    }

    // Check if cached scanners are valid
    let cache_valid = cached.as_ref().map_or(false, |c| {
        if c.entries.len() != wallet_configs.len() {
            return false;
        }
        c.entries
            .iter()
            .zip(wallet_configs.iter())
            .zip(config_fingerprints.iter())
            .all(|((entry, config), fp)| {
                entry.fingerprint == *fp && entry.lookahead == config.lookahead
            })
    });

    let mut cached_scanners = if cache_valid {
        cached.expect("invariant: cache_valid implies Some(cached)")
    } else {
        drop(cached);
        let mut entries = Vec::with_capacity(wallet_configs.len());
        for (config, fp) in wallet_configs.iter().zip(config_fingerprints.iter()) {
            let network = config.network;

            let view_only = parse_view_only_keys(&config.mnemonic);
            let seed_opt = if view_only.is_none() {
                Some(resolve_seed(&config.mnemonic)?)
            } else {
                None
            };
            let (spend_point, view_scalar, address) = if let Some((vs, sp)) = view_only.as_ref() {
                let vp: EdwardsPoint = vs * ED25519_BASEPOINT_TABLE;
                let addr = standard_address(network, *sp, vp).to_string();
                (*sp, *vs, addr)
            } else {
                let seed = seed_opt.as_ref().unwrap();
                let addr = address_from_seed(seed, network, &config.passphrase).to_string();
                (
                    spend_key_from_seed(seed, &config.passphrase),
                    view_key_from_seed(seed, &config.passphrase),
                    addr,
                )
            };
            let spend_scalar = view_only.is_none().then(|| {
                spend_key_scalar_from_seed(seed_opt.as_ref().unwrap(), &config.passphrase)
            });

            let mut scanner = oxide_scanner_from_keys(&spend_point, &view_scalar)?;
            register_subaddresses_async(&mut scanner, config.lookahead).await;

            entries.push(CachedScannerEntry {
                scanner,
                address,
                lookahead: config.lookahead,
                fingerprint: *fp,
                watermark: SubaddressWatermark::new(config.lookahead),
                spend_scalar,
            });
        }
        CachedScanners { entries }
    };

    let mut results = Vec::with_capacity(response.blocks.len());
    let mut previous_block_hash: Option<[u8; 32]> = None;

    #[cfg(target_arch = "wasm32")]
    let mut last_yield_ms = js_sys::Date::now();

    for (block_idx, block_entry) in response.blocks.iter().enumerate() {
        let block_height = response.start_height + block_idx as u64;

        let block = OxideBlock::read::<&[u8]>(&mut block_entry.block.as_ref())
            .map_err(|e| format!("Failed to parse block at height {}: {:?}", block_height, e))?;

        if block.number() != block_height as usize {
            return Err(format!(
                "Block height mismatch: expected {}, got {}",
                block_height,
                block.number()
            ));
        }
        check_block_follows_parent(&block, block_height, previous_block_hash)?;
        check_block_transaction_count(&block, block_entry.txs.len(), block_height)?;
        previous_block_hash = Some(block.hash());

        let block_timestamp = block.header.timestamp;
        let block_hash = hex::encode(block.hash());
        let parent_hash = hex::encode(block.header.previous);

        let miner_tx_hash = block.miner_transaction().hash();
        let miner_tx = OxideTransaction::<Pruned>::from(block.miner_transaction().clone());
        let tx_count = 1 + block_entry.txs.len();

        {
            let parsed_txs = parse_block_transaction_blobs(
                &block,
                &block_entry.txs,
                block_entry.pruned,
                block_height,
            )?;

            let mut spent_key_images = Vec::new();
            let mut spent_key_image_tx_hashes: Vec<String> = Vec::new();
            let mut spent_key_image_rings: Vec<Vec<u64>> = Vec::new();
            for (tx_hash, tx) in std::iter::once((miner_tx_hash, &miner_tx))
                .chain(parsed_txs.iter().map(|(hash, tx)| (*hash, tx)))
            {
                let tx_hash_hex = hex::encode(tx_hash);
                for input in &tx.prefix().inputs {
                    if let OxideInput::ToKey {
                        key_image,
                        key_offsets,
                        ..
                    } = input
                    {
                        spent_key_images.push(hex::encode(key_image.to_bytes()));
                        spent_key_image_tx_hashes.push(tx_hash_hex.clone());
                        spent_key_image_rings.push(key_offsets.clone());
                    }
                }
            }

            let scan_txs: Vec<OxideTransaction<Pruned>> =
                parsed_txs.iter().map(|(_, tx)| tx.clone()).collect();
            let first_ringct_index = first_ringct_index_from_rpc(
                &block,
                &scan_txs,
                response.output_indices.get(block_idx),
            )?;
            let version_1 = version_1_output_indices(
                &block,
                &parsed_txs,
                response.output_indices.get(block_idx),
            )?;

            let mut wallet_results = HashMap::new();
            for entry in &mut cached_scanners.entries {
                let scanned = scan_parsed_block_expanding(
                    &mut entry.scanner,
                    &mut entry.watermark,
                    entry.lookahead,
                    &block,
                    &scan_txs,
                    first_ringct_index.unwrap_or(0),
                )?;

                let key_image_scalar = entry.spend_scalar.as_ref().map(|scalar| &**scalar);
                let outputs = map_block_outputs(
                    &scanned,
                    block_height,
                    miner_tx_hash,
                    key_image_scalar,
                    first_ringct_index,
                    &version_1,
                )?;
                wallet_results.insert(
                    entry.address.clone(),
                    WalletScanData {
                        address: entry.address.clone(),
                        outputs,
                    },
                );
            }

            results.push(MultiWalletScanResult {
                block_height,
                block_hash,
                previous_block_hash: parent_hash,
                block_timestamp,
                tx_count,
                daemon_height,
                spent_key_images,
                spent_key_image_tx_hashes,
                spent_key_image_rings,
                wallet_results,
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            let now = js_sys::Date::now();
            if now - last_yield_ms >= 16.0 {
                yield_to_event_loop().await;
                last_yield_ms = js_sys::Date::now();
            }
        }
    }

    Ok((results, cached_scanners))
}

/// Convenience wrapper for multi-wallet batch scanning with URL-based RPC creation.
#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_blocks_batch_multi_wallet_with_url(
    node_url: &str,
    start_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
    prune: bool,
) -> Result<Vec<MultiWalletScanResult>, String> {
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    scan_blocks_batch_multi_wallet(&rpc, start_height, wallet_configs, prune).await
}

/// Opaque wrapper around `GetBlocksFastResponse` for double-buffered pipelining.
///
/// The hub crate depends on `monero-rust` but not `monero-serai`, so it cannot
/// use `GetBlocksFastResponse` directly.
pub struct FetchedBlocks {
    response: GetBlocksFastResponse,
    scan_from: u64,
    /// Requested start height, retained for validation against the daemon's response.
    requested_start_height: u64,
    /// Wallet-supplied anchors for the zero-height history negotiation sentinel.
    requested_history: Vec<(u64, [u8; 32])>,
    pool: Option<PoolUpdate>,
}

impl FetchedBlocks {
    /// Resolve the wire sentinel only after authenticating the common block against
    /// the wallet's supplied history. Never derive a logical bound from an
    /// unanchored daemon height.
    fn validated_start_height(&self) -> Result<u64, String> {
        if self.requested_start_height != 0 || self.requested_history.is_empty() {
            check_batch_start_height(self.response.start_height, self.requested_start_height)?;
            return Ok(self.requested_start_height);
        }
        // monerod answers a list that starts with its top block with no blocks
        // (core_rpc_server.cpp:704-709).
        if self.response.blocks.is_empty() {
            let (tip, _) = self.requested_history[0];
            if self.response.start_height == 0 && self.response.current_height == tip + 1 {
                return Ok(tip + 1);
            }
            return Err(
                "History-negotiated batch is missing its common starting block".to_string(),
            );
        }
        let entry = self.response.blocks.first().ok_or_else(|| {
            "History-negotiated batch is missing its common starting block".to_string()
        })?;
        let mut bytes = entry.block.as_slice();
        let block = OxideBlock::read(&mut bytes)
            .map_err(|e| format!("Failed to parse history anchor block: {e}"))?;
        if !bytes.is_empty() || block.number() as u64 != self.response.start_height {
            return Err("History anchor block height or encoding mismatch".to_string());
        }
        let anchor = (self.response.start_height, block.hash());
        if !self.requested_history.contains(&anchor) {
            return Err(
                "Returned common starting block is not in the supplied wallet history".to_string(),
            );
        }
        Ok(anchor.0)
    }

    pub fn block_count(&self) -> usize {
        self.response.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.response.blocks.is_empty()
    }

    /// Height of the first block in the batch, checked against the request.
    pub fn start_height(&self) -> Result<u64, String> {
        self.validated_start_height()
    }

    /// The chain length the node claimed. Unverified.
    pub fn daemon_height(&self) -> u64 {
        self.response.current_height
    }

    /// The pool changes fetched with this batch, for `scan_pool_update` after the
    /// blocks as wallet2 does (wallet2.cpp:4335-4337).
    pub fn take_pool(&mut self) -> Option<PoolUpdate> {
        self.pool.take()
    }

    fn last_block_ids(&self, count: usize) -> Result<Vec<(u64, [u8; 32])>, String> {
        let start = self.validated_start_height()?;
        let blocks = &self.response.blocks;
        let first = blocks.len().saturating_sub(count);
        blocks[first..]
            .iter()
            .enumerate()
            .map(|(offset, entry)| {
                let block = OxideBlock::read(&mut entry.block.as_slice())
                    .map_err(|e| format!("Failed to parse block: {e}"))?;
                Ok((start + (first + offset) as u64, block.hash()))
            })
            .collect()
    }
}

/// The id of a network's genesis block; a regtest daemon runs mainnet's.
pub fn genesis_block_hash(network: Network) -> [u8; 32] {
    let hex = match network {
        Network::Mainnet => "418015bb9ae982a1975da7d79277c2705727a56894ba0fb246adaabb1f4632e3",
        Network::Testnet => "48ca7cd3c8de5b6a4d53d2861fbdaedca141553559f9be9520068053cda8430b",
        Network::Stagenet => "76ee3cc98646292206cd3e86f74d88b4dcc1d937088645e9b0cbca84b7ce74eb",
    };
    let mut hash = [0u8; 32];
    hex::decode_to_slice(hex, &mut hash).expect("constant genesis id");
    hash
}

/// wallet2's `drop_from_short_history` (wallet2.cpp:784-795): remove the `n`
/// entries before the last one, which is genesis.
fn drop_from_short_history<T>(history: &mut Vec<T>, n: usize) {
    if history.len() > n {
        let last = history.len() - 1;
        history.drain(last - n..last);
    }
}

/// wallet2's block id list for `chain`, and the entries of it the wallet holds.
fn short_history(
    chain: &crate::wallet_state::BlockHashChain,
    network: Network,
    granularity: u64,
) -> Result<(Vec<(u64, [u8; 32])>, Vec<(u64, [u8; 32])>), String> {
    let genesis = genesis_block_hash(network);
    let mut genesis_only;
    let chain = if chain.is_empty() {
        genesis_only = crate::wallet_state::BlockHashChain::new();
        genesis_only.record_block(0, hex::encode(genesis));
        &genesis_only
    } else {
        chain
    };
    let mut known = Vec::new();
    for (height, hash) in chain.get_short_chain_history_with_granularity(granularity) {
        let hash = hex_to_hash(&hash)?;
        if height == 0 && hash != genesis {
            continue;
        }
        known.push((height, hash));
    }
    let mut history = known.clone();
    if history.last() != Some(&(0, genesis)) {
        history.push((0, genesis));
    }
    Ok((history, known))
}

fn block_ids_missing(
    chain: &crate::wallet_state::BlockHashChain,
    network: Network,
    restore_height: u64,
) -> bool {
    let size = chain.tip_height().map_or(1, |tip| tip + 1);
    restore_height > size || unanchored_base(chain, network).is_some()
}

fn unanchored_base(chain: &crate::wallet_state::BlockHashChain, network: Network) -> Option<u64> {
    let base = chain.base_height().filter(|base| *base > 0)?;
    let checkpoint = crate::checkpoints::at(network, base).map(hex::encode);
    match (checkpoint, chain.get_hash(base)) {
        (Some(checkpoint), Some(held)) if checkpoint == held => None,
        _ => Some(base),
    }
}

/// wallet2's `fast_refresh`: record `chain`'s missing block ids below `stop_height`.
async fn pull_block_ids<R: RpcConnection>(
    rpc: &Rpc<R>,
    chain: &mut crate::wallet_state::BlockHashChain,
    network: Network,
    stop_height: u64,
    granularity: u64,
) -> Result<(), String> {
    let (checkpoint, checkpoint_id) = crate::checkpoints::fetch_base(network, stop_height);
    let tip = chain.tip_height().unwrap_or(0);
    let mut trimmed = match checkpoint_id {
        Some(id) if stop_height > checkpoint && tip < checkpoint => {
            chain.trim_below(checkpoint);
            chain.record_block(checkpoint, hex::encode(id));
            true
        }
        _ => false,
    };
    let mut history = short_history(chain, network, if trimmed { 1 } else { granularity })?.0;
    let mut current = chain.tip_height().map_or(1, |tip| tip + 1);
    let mut reached = 0u64;
    while current < stop_height {
        let ids: Vec<[u8; 32]> = history.iter().map(|(_, id)| *id).collect();
        let reply = rpc
            .get_hashes(&ids, 0)
            .await
            .map_err(|e| format!("Failed to fetch block ids: {:?}", e))?;
        if reply.ids.len() <= 3 {
            return Ok(());
        }
        let start = reply.start_height;
        if start < chain.base_height().unwrap_or(0) {
            if !trimmed {
                return Err(format!(
                    "The node's block ids start at height {start}, below the first block \
                     the wallet holds"
                ));
            }
            chain.rollback_to(checkpoint);
            trimmed = false;
            history = short_history(chain, network, 1)?.0;
            current = chain.tip_height().map_or(1, |tip| tip + 1);
            reached = 0;
            continue;
        }
        if !history.contains(&(start, reply.ids[0])) {
            return Err(format!(
                "The node's block ids start at height {start}, which is not a block the \
                 request named"
            ));
        }
        let end = start + reply.ids.len() as u64;
        if end <= reached {
            return Err("The node's block ids do not move forward".to_string());
        }
        reached = end;
        current = start;
        if end < stop_height {
            drop_from_short_history(&mut history, 3);
            for (offset, id) in reply.ids.iter().enumerate().skip(reply.ids.len() - 3).rev() {
                history.insert(0, (start + offset as u64, *id));
            }
        }
        for id in &reply.ids {
            let id_hex = hex::encode(id);
            match chain.get_hash(current) {
                Some(held) if held != id_hex => return Ok(()),
                Some(_) => {}
                None => chain.record_block(current, id_hex),
            }
            current += 1;
            if current >= stop_height {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// The block requests of one refresh, made as wallet2's refresh makes them. Call
/// `fetch_block_ids`, then `next_batch` until `None`, processing each batch before
/// the next; start a new `Refresh` after a reorganisation or an error.
#[derive(Debug, Clone)]
pub struct Refresh {
    history: Vec<(u64, [u8; 32])>,
    network: Network,
    restore_height: u64,
    granularity: u64,
    block_ids_missing: bool,
    known: Vec<(u64, [u8; 32])>,
    start_height: u64,
    client_key: crate::monero_rpc::RpcClientKey,
    tail: Vec<(u64, [u8; 32])>,
    previous_start: Option<u64>,
    done: bool,
    pool: Option<(u64, HashSet<[u8; 32]>)>,
}

impl Refresh {
    /// A refresh of `state` on `network`, as wallet2 makes it against a
    /// trusted daemon or after its first refresh.
    pub fn new(state: &WalletState, network: Network, restore_height: u64) -> Result<Self, String> {
        Self::with_granularity(state, network, restore_height, 1)
    }

    /// A refresh whose first list is rounded down to `granularity`; wallet2 uses
    /// 1024 on its first refresh against an untrusted daemon (wallet2.cpp:4147).
    pub fn with_granularity(
        state: &WalletState,
        network: Network,
        restore_height: u64,
        granularity: u64,
    ) -> Result<Self, String> {
        let (history, known) = short_history(&state.block_hashes, network, granularity)?;
        Ok(Refresh {
            history,
            network,
            restore_height,
            granularity,
            block_ids_missing: block_ids_missing(&state.block_hashes, network, restore_height),
            known,
            start_height: 0,
            client_key: state.rpc_client_key().clone(),
            tail: Vec::new(),
            previous_start: None,
            done: false,
            pool: None,
        })
    }

    /// Ask for the pool with the first block request, as wallet2's refresh does
    /// (wallet2.cpp:3206), incrementally from `view`.
    pub fn with_pool(mut self, view: &MempoolView) -> Self {
        self.pool = Some((view.query_time, view.known_ids()));
        self
    }

    /// Whether the refresh has reached the node's tip.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Fetch the block ids `state` is missing below the restore height, as
    /// wallet2's `fast_refresh` does. `next_batch` refuses to run until this is done.
    pub async fn fetch_block_ids<R: RpcConnection>(
        &mut self,
        rpc: &Rpc<R>,
        state: &mut WalletState,
    ) -> Result<(), String> {
        if !self.block_ids_missing {
            return Ok(());
        }
        let network = self.network;
        if let Some(base) = unanchored_base(&state.block_hashes, network) {
            let mut below = crate::wallet_state::BlockHashChain::new();
            pull_block_ids(rpc, &mut below, network, base, 1).await?;
            for height in below.earliest_height().unwrap_or(base)..base {
                if let Some(hash) = below.get_hash(height) {
                    state.block_hashes.record_block(height, hash.to_string());
                }
            }
        }
        let size = state.block_hashes.tip_height().map_or(1, |tip| tip + 1);
        if self.restore_height > size {
            pull_block_ids(
                rpc,
                &mut state.block_hashes,
                network,
                self.restore_height,
                self.granularity,
            )
            .await?;
        }
        if let Some(tip) = state.block_hashes.tip_height() {
            state.current_height = state.current_height.max(tip);
        }
        let (history, known) = short_history(&state.block_hashes, network, self.granularity)?;
        self.history = history;
        self.known = known;
        self.block_ids_missing = false;
        Ok(())
    }

    /// Ask for the next batch; `None` once the refresh is done. Requests are
    /// signed with the refreshed wallet's key, whatever key `rpc` has.
    pub async fn next_batch<R: RpcConnection>(
        &mut self,
        rpc: &Rpc<R>,
    ) -> Result<Option<FetchedBlocks>, String> {
        let rpc = &rpc.with_client_key(&self.client_key);
        if self.done {
            return Ok(None);
        }
        if self.block_ids_missing {
            return Err(
                "The wallet holds no block ids below its restore height; call \
                 Refresh::fetch_block_ids before asking for blocks"
                    .to_string(),
            );
        }
        drop_from_short_history(&mut self.history, 3);
        for id in self.tail.drain(..) {
            self.history.insert(0, id);
            self.known.insert(0, id);
        }
        let block_ids: Vec<[u8; 32]> = self.history.iter().map(|(_, hash)| *hash).collect();
        let (response, pool) = match self.pool.take() {
            None => (
                rpc.get_blocks_fast(&block_ids, self.start_height, true)
                    .await
                    .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?,
                None,
            ),
            Some((since, known)) => {
                let (response, info) = rpc
                    .get_blocks_and_pool(&block_ids, self.start_height, true, since)
                    .await
                    .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;
                let update = match info {
                    Some(info) => pool_update_from_info(rpc, info, &known).await?,
                    None => fetch_pool_by_query(rpc, &known).await?,
                };
                (response, Some(update))
            }
        };
        let fetched = FetchedBlocks {
            response,
            scan_from: self.restore_height,
            requested_start_height: self.start_height,
            requested_history: if self.start_height == 0 {
                self.known.clone()
            } else {
                Vec::new()
            },
            pool,
        };
        let start = fetched.validated_start_height()?;
        self.start_height = 0;
        if fetched.is_empty() || self.previous_start == Some(start) {
            self.done = true;
        } else {
            let tail = fetched.last_block_ids(3)?;
            let (last, _) = tail[tail.len() - 1];
            if last + 1 >= fetched.daemon_height() {
                self.done = true;
            }
            self.tail = tail;
        }
        self.previous_start = Some(start);
        Ok(Some(fetched))
    }
}

/// Fetch a batch of blocks without scanning them.
///
/// A node can tell this request from wallet2's; `Refresh` sends wallet2's.
pub async fn fetch_blocks_batch<R: RpcConnection>(
    rpc: &Rpc<R>,
    start_height: u64,
    prune: bool,
) -> Result<FetchedBlocks, String> {
    let anchor_height = start_height.saturating_sub(1);
    let known_hash = rpc
        .get_block_hash(anchor_height as usize)
        .await
        .map_err(|e| format!("Failed to get block hash at {}: {:?}", anchor_height, e))?;
    let response = rpc
        .get_blocks_fast(&[known_hash], start_height, prune)
        .await
        .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;
    Ok(FetchedBlocks {
        response,
        scan_from: 0,
        requested_start_height: start_height,
        requested_history: Vec::new(),
        pool: None,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_blocks_batch_with_url(
    node_url: &str,
    start_height: u64,
    prune: bool,
) -> Result<FetchedBlocks, String> {
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    fetch_blocks_batch(&rpc, start_height, prune).await
}

/// Process a previously fetched batch for a single wallet (no caching).
pub async fn process_fetched_batch(
    fetched: FetchedBlocks,
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    passphrase: &str,
) -> Result<Vec<BlockScanResult>, String> {
    let validated_start_height = fetched.validated_start_height()?;
    let (results, _cached) = scan_batch_response(
        fetched.response,
        validated_start_height,
        fetched.scan_from,
        mnemonic,
        network,
        lookahead,
        None,
        passphrase,
    )
    .await?;
    Ok(results)
}

/// Process a previously fetched batch for a single wallet, with scanner caching.
pub async fn process_fetched_batch_cached(
    fetched: FetchedBlocks,
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    cached: Option<CachedScanner>,
    passphrase: &str,
) -> Result<(Vec<BlockScanResult>, CachedScanner), String> {
    let validated_start_height = fetched.validated_start_height()?;
    scan_batch_response(
        fetched.response,
        validated_start_height,
        fetched.scan_from,
        mnemonic,
        network,
        lookahead,
        cached,
        passphrase,
    )
    .await
}

fn hex_to_hash(hex_str: &str) -> Result<[u8; 32], String> {
    let bytes =
        hex::decode(hex_str).map_err(|e| format!("Invalid hex hash '{}': {}", hex_str, e))?;
    if bytes.len() != 32 {
        return Err(format!("Hash hex must be 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

/// Scan a batch of blocks, sending `known_hashes` as the block id history.
/// Returns the results and the height the reply starts at.
pub async fn scan_blocks_batch_with_history<R: RpcConnection>(
    rpc: &Rpc<R>,
    start_height: u64,
    known_hashes: &[(u64, String)],
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    prune: bool,
    passphrase: &str,
) -> Result<(Vec<BlockScanResult>, u64), String> {
    let (fetched, actual_start) =
        fetch_blocks_batch_with_history(rpc, start_height, known_hashes, prune).await?;
    let results = process_fetched_batch(fetched, mnemonic, network, lookahead, passphrase).await?;
    Ok((results, actual_start))
}

/// Convenience wrapper for history-aware batch scanning with URL-based RPC.
#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_blocks_batch_with_history_url(
    node_url: &str,
    start_height: u64,
    known_hashes: &[(u64, String)],
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    prune: bool,
    passphrase: &str,
) -> Result<(Vec<BlockScanResult>, u64), String> {
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    scan_blocks_batch_with_history(
        &rpc,
        start_height,
        known_hashes,
        mnemonic,
        network,
        lookahead,
        prune,
        passphrase,
    )
    .await
}

/// Fetch a batch of blocks using known hash history, without scanning; see
/// `scan_blocks_batch_with_history`.
///
/// The request matches wallet2's only when made as `Refresh` makes it.
pub async fn fetch_blocks_batch_with_history<R: RpcConnection>(
    rpc: &Rpc<R>,
    start_height: u64,
    known_hashes: &[(u64, String)],
    prune: bool,
) -> Result<(FetchedBlocks, u64), String> {
    let mut requested_history = known_hashes
        .iter()
        .map(|(height, hash)| Ok((*height, hex_to_hash(hash)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let mut block_ids: Vec<[u8; 32]> = if requested_history.is_empty() {
        let anchor_height = start_height.saturating_sub(1);
        let hash = rpc
            .get_block_hash(anchor_height as usize)
            .await
            .map_err(|e| format!("Failed to get block hash at {}: {:?}", anchor_height, e))?;
        vec![hash]
    } else {
        requested_history.iter().map(|(_, hash)| *hash).collect()
    };
    // monerod refuses a list that does not end in its genesis id, and blocks a
    // client that keeps sending one.
    if start_height == 0 && !requested_history.is_empty() {
        let genesis = rpc
            .genesis_hash()
            .await
            .map_err(|e| format!("Failed to get the genesis block hash: {:?}", e))?;
        requested_history.retain(|(height, hash)| *height != 0 || *hash == genesis);
        block_ids = requested_history.iter().map(|(_, hash)| *hash).collect();
        if block_ids.last() != Some(&genesis) {
            block_ids.push(genesis);
        }
    }
    let response = rpc
        .get_blocks_fast(&block_ids, start_height, prune)
        .await
        .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;
    let actual_start = response.start_height;
    let fetched = FetchedBlocks {
        response,
        scan_from: 0,
        requested_start_height: start_height,
        requested_history,
        pool: None,
    };
    fetched.validated_start_height()?;
    Ok((fetched, actual_start))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_blocks_batch_with_history_url(
    node_url: &str,
    start_height: u64,
    known_hashes: &[(u64, String)],
    prune: bool,
) -> Result<(FetchedBlocks, u64), String> {
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    fetch_blocks_batch_with_history(&rpc, start_height, known_hashes, prune).await
}

/// Process a previously fetched batch for multiple wallets (no caching).
pub async fn process_fetched_batch_multi_wallet(
    fetched: FetchedBlocks,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<Vec<MultiWalletScanResult>, String> {
    let validated_start_height = fetched.validated_start_height()?;
    let (results, _cached) = process_batch_multi_wallet_response(
        fetched.response,
        validated_start_height,
        wallet_configs,
        None,
    )
    .await?;
    Ok(results)
}

/// Process a previously fetched batch for multiple wallets, with scanner caching.
pub async fn process_fetched_batch_multi_wallet_cached(
    fetched: FetchedBlocks,
    wallet_configs: Vec<WalletScanConfig>,
    cached: Option<CachedScanners>,
) -> Result<(Vec<MultiWalletScanResult>, CachedScanners), String> {
    let validated_start_height = fetched.validated_start_height()?;
    process_batch_multi_wallet_response(
        fetched.response,
        validated_start_height,
        wallet_configs,
        cached,
    )
    .await
}

/// Scan one confirmed block for multiple wallets through `/getblocks.bin`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_block_multi_wallet<R: RpcConnection + Send + Sync + Clone + 'static>(
    rpc: &Rpc<R>,
    block_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<MultiWalletScanResult, String> {
    scan_blocks_batch_multi_wallet(rpc, block_height, wallet_configs, false)
        .await?
        .into_iter()
        .find(|result| result.block_height == block_height)
        .ok_or_else(|| format!("Node did not return requested block {block_height}"))
}

/// Convenience wrapper for multi-wallet scanning with URL-based RPC creation
#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_block_multi_wallet_with_url(
    node_url: &str,
    block_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<MultiWalletScanResult, String> {
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    scan_block_multi_wallet(&rpc, block_height, wallet_configs).await
}

/// WASM variant of `scan_block_multi_wallet`.
#[cfg(target_arch = "wasm32")]
pub async fn scan_block_multi_wallet_wasm<R: RpcConnection>(
    rpc: &Rpc<R>,
    block_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<MultiWalletScanResult, String> {
    scan_blocks_batch_multi_wallet(rpc, block_height, wallet_configs, false)
        .await?
        .into_iter()
        .find(|result| result.block_height == block_height)
        .ok_or_else(|| format!("Node did not return requested block {block_height}"))
}

/// What a wallet knows of the node's pool between refreshes. As in wallet2, the
/// first update in a session asks for and scans the whole pool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MempoolView {
    #[serde(skip)]
    query_time: u64,
    #[serde(default)]
    txs: BTreeMap<String, MempoolTx>,
    #[serde(skip)]
    updated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MempoolTx {
    key_images: Vec<String>,
    outputs: Vec<WalletOutput>,
}

impl MempoolView {
    pub fn new() -> Self {
        Self::default()
    }

    /// The `pool_info_since` of the next request; zero before any reply
    /// carried pool data.
    pub fn query_time(&self) -> u64 {
        self.query_time
    }

    /// Whether the pool held `tx_hash` (hex) when the view was last updated.
    pub fn contains(&self, tx_hash: &str) -> bool {
        self.txs.contains_key(tx_hash)
    }

    /// Make the next update ask for the whole pool, as wallet2 does on a daemon change.
    pub fn node_changed(&mut self) {
        self.query_time = 0;
    }

    /// Number of pool transactions in the view.
    pub fn len(&self) -> usize {
        self.txs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    fn known_ids(&self) -> HashSet<[u8; 32]> {
        if !self.updated {
            return HashSet::new();
        }
        self.txs
            .keys()
            .filter_map(|hash| hex_to_hash(hash).ok())
            .collect()
    }

    /// The whole pool as the view holds it, in the form `scan_mempool`
    /// reports it.
    pub fn result(&self, accounts_to_scan: Option<&[u32]>) -> MempoolScanResult {
        let mut spent_key_images = Vec::new();
        let mut spent_key_image_tx_hashes = Vec::new();
        let mut outputs = Vec::new();
        for (tx_hash, tx) in &self.txs {
            for key_image in &tx.key_images {
                spent_key_images.push(key_image.clone());
                spent_key_image_tx_hashes.push(tx_hash.clone());
            }
            outputs.extend(tx.outputs.iter().cloned());
        }
        retain_mempool_outputs_for_accounts(&mut outputs, accounts_to_scan);
        MempoolScanResult {
            tx_count: self.txs.len(),
            outputs,
            spent_key_images,
            spent_key_image_tx_hashes,
        }
    }
}

#[derive(Debug, Clone)]
enum PoolMembership {
    Whole(HashSet<[u8; 32]>),
    Removed(Vec<[u8; 32]>),
}

/// Pool changes fetched from the node, to be applied to a `MempoolView`
/// with `scan_pool_update`.
#[derive(Debug, Clone)]
pub struct PoolUpdate {
    added: Vec<PoolTx>,
    membership: PoolMembership,
    daemon_time: Option<u64>,
}

async fn pool_update_from_info<R: RpcConnection>(
    rpc: &Rpc<R>,
    info: PoolInfo,
    known: &HashSet<[u8; 32]>,
) -> Result<PoolUpdate, String> {
    // A transaction listed both removed and added came back: take the removals
    // first, as monerod asks (tx_pool.cpp:1805-1808).
    let mut known = known.clone();
    for id in &info.removed {
        known.remove(id);
    }
    let known = &known;
    let membership = if info.incremental {
        PoolMembership::Removed(info.removed)
    } else {
        PoolMembership::Whole(
            info.added
                .iter()
                .map(|tx| tx.tx_hash)
                .chain(info.remaining_added.iter().copied())
                .collect(),
        )
    };
    let missing: Vec<[u8; 32]> = info
        .remaining_added
        .iter()
        .copied()
        .filter(|hash| !known.contains(hash))
        .collect();
    let mut added: Vec<PoolTx> = info
        .added
        .into_iter()
        .filter(|tx| !known.contains(&tx.tx_hash))
        .collect();
    if !missing.is_empty() {
        added.extend(
            rpc.get_pool_transactions(&missing)
                .await
                .map_err(|e| format!("Failed to fetch pool transactions: {:?}", e))?,
        );
    }
    Ok(PoolUpdate {
        added,
        membership,
        daemon_time: Some(info.daemon_time),
    })
}

async fn fetch_pool_by_query<R: RpcConnection>(
    rpc: &Rpc<R>,
    known: &HashSet<[u8; 32]>,
) -> Result<PoolUpdate, String> {
    let hashes = rpc
        .get_pool_hashes()
        .await
        .map_err(|e| format!("Failed to fetch the pool's transaction ids: {:?}", e))?;
    let missing: Vec<[u8; 32]> = hashes
        .iter()
        .copied()
        .filter(|hash| !known.contains(hash))
        .collect();
    let added = if missing.is_empty() {
        Vec::new()
    } else {
        rpc.get_pool_transactions(&missing)
            .await
            .map_err(|e| format!("Failed to fetch pool transactions: {:?}", e))?
    };
    Ok(PoolUpdate {
        added,
        membership: PoolMembership::Whole(hashes.into_iter().collect()),
        daemon_time: None,
    })
}

fn parse_pool_tx(tx: &PoolTx) -> Result<OxideTransaction<Pruned>, String> {
    let mut pruned = tx.blob.clone();
    pruned.extend_from_slice(&[0u8; 32]);
    if let Some((_, parsed)) = parse_pruned_tx_blob(&pruned) {
        return Ok(parsed);
    }
    match parse_full_tx_blob(&tx.blob) {
        Some((hash, parsed)) if hash == tx.tx_hash => Ok(parsed),
        _ => Err(format!(
            "Pool transaction {} does not parse",
            hex::encode(tx.tx_hash)
        )),
    }
}

fn scan_pool_txs(
    scanner: &mut Scanner,
    key_image_scalar: Option<&Scalar>,
    txs: &[PoolTx],
) -> Result<Vec<(String, MempoolTx)>, String> {
    let mut records = Vec::with_capacity(txs.len());
    for tx in txs {
        let parsed = parse_pool_tx(tx)?;
        let mut record = MempoolTx::default();
        for input in &parsed.prefix().inputs {
            if let OxideInput::ToKey { key_image, .. } = input {
                record.key_images.push(hex::encode(key_image.to_bytes()));
            }
        }
        for output in &scan_single_transaction(scanner, tx.tx_hash, &parsed)? {
            record.outputs.push(map_oxide_output(
                output,
                0,         // unconfirmed: still in the mempool
                [0u8; 32], // mempool transactions are never coinbase
                key_image_scalar,
                None,
                parsed.version() == 1,
            )?);
        }
        records.push((hex::encode(tx.tx_hash), record));
    }
    Ok(records)
}

/// Apply pool changes to `view`, scanning the transactions new to it. A
/// transaction that does not parse fails the update and leaves the view as it was.
pub fn scan_pool_update(
    update: PoolUpdate,
    view: &mut MempoolView,
    mnemonic: &str,
    _network: Network,
    lookahead: Lookahead,
    accounts_to_scan: Option<&[u32]>,
    passphrase: &str,
) -> Result<MempoolScanResult, String> {
    let removed: HashSet<[u8; 32]> = match &update.membership {
        PoolMembership::Removed(ids) => ids.iter().copied().collect(),
        PoolMembership::Whole(_) => HashSet::new(),
    };
    let known = view.known_ids();
    let new: Vec<PoolTx> = update
        .added
        .into_iter()
        .filter(|tx| !known.contains(&tx.tx_hash) || removed.contains(&tx.tx_hash))
        .collect();
    let records = if new.is_empty() {
        Vec::new()
    } else {
        let view_only = parse_view_only_keys(mnemonic);
        let seed = match view_only {
            Some(_) => None,
            None => Some(resolve_seed(mnemonic)?),
        };
        let (spend_point, view_scalar) = match (&view_only, &seed) {
            (Some((view_scalar, spend_point)), _) => (*spend_point, *view_scalar),
            (None, Some(seed)) => (
                spend_key_from_seed(seed, passphrase),
                view_key_from_seed(seed, passphrase),
            ),
            (None, None) => unreachable!("a seed resolves or the call returned"),
        };
        let mut scanner = oxide_scanner_from_keys(&spend_point, &view_scalar)?;
        register_subaddresses(&mut scanner, lookahead);
        #[cfg(target_arch = "wasm32")]
        let spend_scalar = seed
            .as_ref()
            .map(|seed| spend_key_scalar_from_seed(seed, passphrase));
        #[cfg(target_arch = "wasm32")]
        let key_image_scalar = spend_scalar.as_ref().map(|scalar| &**scalar);
        #[cfg(not(target_arch = "wasm32"))]
        let key_image_scalar: Option<&Scalar> = None;
        scan_pool_txs(&mut scanner, key_image_scalar, &new)?
    };

    match update.membership {
        PoolMembership::Whole(ids) => {
            view.txs
                .retain(|hash, _| hex_to_hash(hash).map_or(false, |hash| ids.contains(&hash)));
        }
        PoolMembership::Removed(ids) => {
            for id in ids {
                view.txs.remove(&hex::encode(id));
            }
        }
    }
    view.txs.extend(records);
    if let Some(time) = update.daemon_time {
        view.query_time = time;
    }
    view.updated = true;
    Ok(view.result(accounts_to_scan))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_mempool_for_outputs(
    node_url: &str,
    mnemonic: &str,
    network: Network,
    passphrase: &str,
) -> Result<MempoolScanResult, String> {
    scan_mempool_for_outputs_with_lookahead(
        node_url,
        mnemonic,
        network,
        DEFAULT_LOOKAHEAD,
        passphrase,
    )
    .await
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_mempool_for_outputs_with_account_lookahead(
    node_url: &str,
    mnemonic: &str,
    network: Network,
    account_lookahead: u32,
    subaddress_lookahead: u32,
    accounts_to_scan: Option<&[u32]>,
    passphrase: &str,
) -> Result<MempoolScanResult, String> {
    let lookahead = Lookahead {
        account: account_lookahead,
        subaddress: if subaddress_lookahead > 0 {
            subaddress_lookahead
        } else {
            DEFAULT_LOOKAHEAD.subaddress
        },
    };
    crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
    scan_mempool(
        &rpc,
        mnemonic,
        network,
        lookahead,
        accounts_to_scan,
        passphrase,
    )
    .await
}

fn retain_mempool_outputs_for_accounts(
    outputs: &mut Vec<WalletOutput>,
    accounts_to_scan: Option<&[u32]>,
) {
    if let Some(accounts) = accounts_to_scan {
        let account_set: HashSet<u32> = accounts.iter().copied().collect();
        outputs.retain(|output| {
            let account = output
                .subaddress_index
                .map(|(account, _)| account)
                .unwrap_or(0);
            account_set.contains(&account)
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_mempool_for_outputs_with_lookahead(
    node_url: &str,
    mnemonic: &str,
    _network: Network,
    lookahead: Lookahead,
    passphrase: &str,
) -> Result<MempoolScanResult, String> {
    crate::validation::validate_node_url(node_url).map_err(|e| e.to_string())?;
    use crate::monero_rpc::url_rpc;
    let rpc = url_rpc(node_url).map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
    scan_mempool(&rpc, mnemonic, _network, lookahead, None, passphrase).await
}

/// Scan the node's whole pool for the wallet, asked for as wallet2 asks outside
/// a refresh (`update_pool_state_by_pool_query`).
pub async fn scan_mempool<R: RpcConnection>(
    rpc: &Rpc<R>,
    mnemonic: &str,
    _network: Network,
    lookahead: Lookahead,
    accounts_to_scan: Option<&[u32]>,
    passphrase: &str,
) -> Result<MempoolScanResult, String> {
    let view_only = parse_view_only_keys(mnemonic);
    let seed_opt = if view_only.is_none() {
        Some(resolve_seed(mnemonic)?)
    } else {
        None
    };
    let spend_point = view_only
        .as_ref()
        .map(|(_, sp)| *sp)
        .unwrap_or_else(|| spend_key_from_seed(seed_opt.as_ref().unwrap(), passphrase));
    let view_scalar = view_only
        .as_ref()
        .map(|(vs, _)| *vs)
        .unwrap_or_else(|| view_key_from_seed(seed_opt.as_ref().unwrap(), passphrase));
    #[cfg(target_arch = "wasm32")]
    let spend_scalar: Option<Zeroizing<Scalar>> = seed_opt
        .as_ref()
        .map(|seed| spend_key_scalar_from_seed(seed, passphrase));

    let mut scanner = oxide_scanner_from_keys(&spend_point, &view_scalar)?;
    register_subaddresses(&mut scanner, lookahead);

    #[cfg(target_arch = "wasm32")]
    let key_image_scalar: Option<&Scalar> = spend_scalar.as_deref();
    #[cfg(not(target_arch = "wasm32"))]
    let key_image_scalar: Option<&Scalar> = None;

    let update = fetch_pool_by_query(rpc, &HashSet::new()).await?;
    let mut view = MempoolView::new();
    view.txs.extend(scan_pool_txs(
        &mut scanner,
        key_image_scalar,
        &update.added,
    )?);
    Ok(view.result(accounts_to_scan))
}

/// `scan_mempool` that updates `view`, fetching and scanning only what it does
/// not hold (everything in its first update of a session).
pub async fn scan_mempool_with_view<R: RpcConnection>(
    rpc: &Rpc<R>,
    view: &mut MempoolView,
    mnemonic: &str,
    network: Network,
    lookahead: Lookahead,
    accounts_to_scan: Option<&[u32]>,
    passphrase: &str,
) -> Result<MempoolScanResult, String> {
    let update = fetch_pool_by_query(rpc, &view.known_ids()).await?;
    scan_pool_update(
        update,
        view,
        mnemonic,
        network,
        lookahead,
        accounts_to_scan,
        passphrase,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_transaction_inputs_are_read_with_their_rings() {
        let mut blob = vec![2, 0, 2];
        blob.extend_from_slice(&[0x02, 0, 3, 5, 0x80, 0x01, 7]);
        blob.extend_from_slice(&[0xaa; 32]);
        blob.extend_from_slice(&[0x02, 0, 1, 9]);
        blob.extend_from_slice(&[0xbb; 32]);
        blob.extend_from_slice(&[0, 0]);

        assert_eq!(
            extract_spent_rings_from_raw_tx(&blob),
            vec![
                ("aa".repeat(32), vec![5, 128, 7]),
                ("bb".repeat(32), vec![9]),
            ]
        );
        assert_eq!(
            extract_key_images_from_raw_tx(&blob),
            vec!["aa".repeat(32), "bb".repeat(32)]
        );
    }

    #[test]
    fn genesis_ids_are_the_networks_genesis_blocks() {
        let mainnet_tx = "013c01ff0001ffffffffffff03029b2e4c0281c0b02e7c53291a94d1d0cbff8883f80\
                          24f5142ee494ffbbd08807121017767aafcde9be00dcfd098715ebcf7f410daebc582\
                          fda69d24a28e9d0bc890d1";
        let stagenet_tx = "013c01ff0001ffffffffffff0302df5d56da0c7d643ddd1ce61901c7bdc5fb1738bfe3\
                           9fbe69c28a3a7032729c0f2101168d0c4ca86fb55a4cf6a36d31431be1c53a3bd7411b\
                           b24e8832410289fa6f3b";
        for (network, tx, nonce) in [
            (Network::Mainnet, mainnet_tx, 10000u32),
            (Network::Testnet, mainnet_tx, 10001),
            (Network::Stagenet, stagenet_tx, 10002),
        ] {
            let mut blob = vec![1, 0, 0];
            blob.extend_from_slice(&[0u8; 32]);
            blob.extend_from_slice(&nonce.to_le_bytes());
            blob.extend_from_slice(&hex::decode(tx).unwrap());
            blob.push(0);
            let block = OxideBlock::read(&mut blob.as_slice()).unwrap();
            assert_eq!(block.hash(), genesis_block_hash(network), "{network:?}");
        }
    }

    #[derive(Clone, Debug)]
    struct HashNode {
        ids: Vec<[u8; 32]>,
        requests: std::sync::Arc<std::sync::Mutex<Vec<Vec<Option<u64>>>>>,
    }

    impl HashNode {
        fn new(length: u64, checkpoints: bool) -> Self {
            let ids = (0..length)
                .map(
                    |height| match crate::checkpoints::at(Network::Mainnet, height) {
                        _ if height == 0 => genesis_block_hash(Network::Mainnet),
                        Some(id) if checkpoints => id,
                        _ => {
                            let mut id: [u8; 32] = Keccak256::digest(height.to_le_bytes()).into();
                            id[0] ^= 0x5a;
                            id
                        }
                    },
                )
                .collect();
            HashNode {
                ids,
                requests: Default::default(),
            }
        }

        fn requests(&self) -> Vec<Vec<Option<u64>>> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl RpcConnection for HashNode {
        async fn post(
            &self,
            route: &str,
            body: Vec<u8>,
        ) -> Result<Vec<u8>, crate::monero_rpc::RpcError> {
            #[derive(Deserialize)]
            struct Request {
                #[serde(with = "serde_bytes")]
                block_ids: Vec<u8>,
                start_height: u64,
            }
            #[derive(Serialize)]
            struct Reply {
                current_height: u64,
                #[serde(with = "serde_bytes")]
                m_block_ids: Vec<u8>,
                start_height: u64,
                status: String,
            }
            assert_eq!(route, "gethashes.bin", "only block ids are asked for");
            let request: Request = crate::safe_epee::from_bytes(&body).unwrap();
            assert_eq!(request.start_height, 0);
            let asked: Vec<[u8; 32]> = request
                .block_ids
                .chunks(32)
                .map(|id| id.try_into().unwrap())
                .collect();
            let height_of = |id: &[u8; 32]| self.ids.iter().position(|own| own == id);
            self.requests.lock().unwrap().push(
                asked
                    .iter()
                    .map(|id| height_of(id).map(|h| h as u64))
                    .collect(),
            );
            assert_eq!(asked.last().and_then(height_of), Some(0));
            let start = asked.iter().find_map(height_of).unwrap();
            let end = (start + crate::monero_rpc::MAX_BLOCK_IDS).min(self.ids.len());
            Ok(monero_epee_bin_serde::to_bytes(&Reply {
                current_height: self.ids.len() as u64,
                m_block_ids: self.ids[start..end].iter().flatten().copied().collect(),
                start_height: start as u64,
                status: "OK".to_string(),
            })
            .unwrap())
        }
    }

    fn chain_heights(state: &WalletState) -> (Option<u64>, Option<u64>, usize) {
        (
            state.block_hashes.earliest_height(),
            state.block_hashes.tip_height(),
            state.block_hashes.len(),
        )
    }

    #[tokio::test]
    async fn block_ids_are_fetched_from_the_checkpoint_below_the_restore_height_like_wallet2() {
        let node = HashNode::new(50_100, true);
        let rpc = Rpc::new_with_connection(node.clone());
        let mut state = WalletState::new();
        let mut refresh = Refresh::new(&state, Network::Mainnet, 50_000).unwrap();
        refresh.fetch_block_ids(&rpc, &mut state).await.unwrap();

        assert_eq!(
            node.requests(),
            vec![
                vec![Some(29_556), Some(0)],
                vec![
                    Some(39_553),
                    Some(39_554),
                    Some(39_555),
                    Some(29_556),
                    Some(0)
                ],
                vec![
                    Some(49_550),
                    Some(49_551),
                    Some(49_552),
                    Some(39_553),
                    Some(0)
                ],
            ]
        );
        assert_eq!(chain_heights(&state), (Some(29_556), Some(49_999), 20_444));
        assert_eq!(state.current_height, 49_999);
        assert_eq!(refresh.history[0].0, 49_999);
        assert_eq!(
            refresh.history[refresh.history.len() - 2..]
                .iter()
                .map(|(height, _)| *height)
                .collect::<Vec<_>>(),
            vec![29_556, 0]
        );
        refresh.fetch_block_ids(&rpc, &mut state).await.unwrap();
        assert!(
            !Refresh::new(&state, Network::Mainnet, 50_000)
                .unwrap()
                .block_ids_missing
        );
        assert_eq!(node.requests().len(), 3);
    }

    #[tokio::test]
    async fn a_node_without_the_checkpoint_is_asked_for_block_ids_from_genesis() {
        let node = HashNode::new(1_200, false);
        let rpc = Rpc::new_with_connection(node.clone());
        let mut state = WalletState::new();
        let mut refresh = Refresh::new(&state, Network::Mainnet, 1_100).unwrap();
        refresh.fetch_block_ids(&rpc, &mut state).await.unwrap();
        assert_eq!(node.requests(), vec![vec![None, Some(0)], vec![Some(0)]]);
        assert_eq!(chain_heights(&state), (Some(0), Some(1_099), 1_100));
    }

    #[tokio::test]
    async fn a_restore_height_past_the_chain_is_reached_by_block_ids_alone() {
        let node = HashNode::new(31, false);
        let rpc = Rpc::new_with_connection(node.clone());
        let mut state = WalletState::new();
        let mut refresh = Refresh::new(&state, Network::Mainnet, 36).unwrap();
        refresh.fetch_block_ids(&rpc, &mut state).await.unwrap();
        assert_eq!(
            node.requests(),
            vec![vec![Some(0)], vec![Some(28), Some(29), Some(30), Some(0)]]
        );
        assert_eq!(chain_heights(&state), (Some(0), Some(30), 31));
        assert_eq!(state.current_height, 30);
        let mut refresh = Refresh::new(&state, Network::Mainnet, 36).unwrap();
        refresh.fetch_block_ids(&rpc, &mut state).await.unwrap();
        assert_eq!(node.requests()[2][0], Some(30));
    }

    #[tokio::test]
    async fn a_chain_recorded_from_its_restore_height_gets_the_ids_below_it() {
        let node = HashNode::new(1_200, false);
        let rpc = Rpc::new_with_connection(node.clone());
        let mut state = WalletState::new();
        for height in 600..=650 {
            state
                .block_hashes
                .record_block(height, hex::encode(node.ids[height as usize]));
        }
        state.current_height = 650;
        let mut refresh = Refresh::new(&state, Network::Mainnet, 600).unwrap();
        refresh.fetch_block_ids(&rpc, &mut state).await.unwrap();
        assert_eq!(node.requests(), vec![vec![Some(0)]]);
        assert_eq!(chain_heights(&state), (Some(0), Some(650), 651));
        assert_eq!(state.current_height, 650);
    }

    #[tokio::test]
    async fn blocks_are_not_asked_for_before_the_block_ids_below_the_restore_height() {
        let node = HashNode::new(100, false);
        let rpc = Rpc::new_with_connection(node.clone());
        let state = WalletState::new();
        let mut refresh = Refresh::new(&state, Network::Mainnet, 50).unwrap();
        let Err(refused) = refresh.next_batch(&rpc).await else {
            panic!("blocks asked for before the block ids");
        };
        assert!(refused.contains("fetch_block_ids"), "{refused}");
        assert!(node.requests().is_empty());
    }

    #[tokio::test]
    async fn block_ids_from_a_block_the_request_did_not_name_are_refused() {
        #[derive(Clone, Debug)]
        struct Shifted(HashNode);
        #[async_trait::async_trait]
        impl RpcConnection for Shifted {
            async fn post(
                &self,
                route: &str,
                body: Vec<u8>,
            ) -> Result<Vec<u8>, crate::monero_rpc::RpcError> {
                let mut reply = self.0.post(route, body).await?;
                let at = reply
                    .windows(12)
                    .position(|window| window == b"start_height")
                    .unwrap();
                reply[at + 13] -= 1;
                Ok(reply)
            }
        }
        let node = HashNode::new(100, false);
        let rpc = Rpc::new_with_connection(Shifted(node.clone()));
        let mut state = WalletState::new();
        for height in 0..=50 {
            state
                .block_hashes
                .record_block(height, hex::encode(node.ids[height as usize]));
        }
        let mut refresh = Refresh::new(&state, Network::Mainnet, 80).unwrap();
        let refused = refresh.fetch_block_ids(&rpc, &mut state).await.unwrap_err();
        assert!(
            refused.contains("not a block the request named"),
            "{refused}"
        );
        assert_eq!(state.block_hashes.tip_height(), Some(50));
    }

    const TEST_VECTOR_1_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
    const TEST_VECTOR_1_SPEND_KEY: &str =
        "29adefc8f67515b4b4bf48031780ab9d071d24f8a674b879ce7f245c37523807";
    const TEST_VECTOR_1_VIEW_KEY: &str =
        "3bc0b202cde92fe5719c3cc0a16aa94f88a5d19f8c515d4e35fae361f6f2120e";
    const TEST_VECTOR_1_ADDRESS: &str = "45wsWad9EwZgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU8K2Dhi";

    const TEST_VECTOR_2_SEED: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";
    const TEST_VECTOR_2_SPEND_KEY: &str =
        "722bbfcf99a9b2c9e700ce857850dd8c4c94c73dca8d914c603f5fee0e365803";
    const TEST_VECTOR_2_VIEW_KEY: &str =
        "0a1a38f6d246e894600a3e27238a064bf5e8d91801df47a17107596b1378e501";
    const TEST_VECTOR_2_ADDRESS: &str = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

    const TEST_VECTOR_3_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
    const TEST_VECTOR_3_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";

    // Additional test vectors from monero-serai test suite
    const TEST_VECTOR_4_SEED: &str = "washing thirsty occur lectures tuesday fainted toxic adapt abnormal memoir nylon mostly building shrugged online ember northern ruby woes dauntless boil family illness inroads northern";
    const TEST_VECTOR_4_SPEND_KEY: &str =
        "c0af65c0dd837e666b9d0dfed62745f4df35aed7ea619b2798a709f0fe545403";
    const TEST_VECTOR_4_VIEW_KEY: &str =
        "513ba91c538a5a9069e0094de90e927c0cd147fa10428ce3ac1afd49f63e3b01";

    const TEST_VECTOR_5_SEED: &str = "minero ocupar mirar evadir octubre cal logro miope opaco disco ancla litio clase cuello nasal clase fiar avance deseo mente grumo negro cordón croqueta clase";
    const TEST_VECTOR_5_SPEND_KEY: &str =
        "ae2c9bebdddac067d73ec0180147fc92bdf9ac7337f1bcafbbe57dd13558eb02";
    const TEST_VECTOR_5_VIEW_KEY: &str =
        "18deafb34d55b7a43cae2c1c1c206a3c80c12cc9d1f84640b484b95b7fec3e05";

    const TEST_VECTOR_6_SEED: &str = "poids vaseux tarte bazar poivre effet entier nuance sensuel ennui pacte osselet poudre battre alibi mouton stade paquet pliage gibier type question position projet pliage";
    const TEST_VECTOR_6_SPEND_KEY: &str =
        "2dd39ff1a4628a94b5c2ec3e42fb3dfe15c2b2f010154dc3b3de6791e805b904";
    const TEST_VECTOR_6_VIEW_KEY: &str =
        "6725b32230400a1032f31d622b44c3a227f88258939b14a7c72e00939e7bdf0e";

    const TEST_VECTOR_7_SEED: &str = "Kaliber Gabelung Tapir Liveband Favorit Specht Enklave Nabel Jupiter Foliant Chronik nisten löten Vase Aussage Rekord Yeti Gesetz Eleganz Alraune Künstler Almweide Jahr Kastanie Almweide";
    const TEST_VECTOR_7_SPEND_KEY: &str =
        "79801b7a1b9796856e2397d862a113862e1fdc289a205e79d8d70995b276db06";
    const TEST_VECTOR_7_VIEW_KEY: &str =
        "99f0ec556643bd9c038a4ed86edcb9c6c16032c4622ed2e000299d527a792701";

    #[test]
    fn test_generate_seed() {
        let seed = generate_seed(SeedFormat::Classic).expect("Failed to generate seed");
        let words: Vec<&str> = seed.split_whitespace().collect();
        assert_eq!(words.len(), 25);
    }

    #[test]
    fn test_derive_address_test_vector_1_mainnet() {
        let address = derive_address(TEST_VECTOR_1_SEED, Network::Mainnet, "")
            .expect("Failed to derive address from test vector 1");

        assert_eq!(address.to_string(), TEST_VECTOR_1_ADDRESS);

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_1_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_1_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_1_VIEW_KEY);
    }

    #[test]
    fn test_derive_address_test_vector_2_stagenet() {
        let address = derive_address(TEST_VECTOR_2_SEED, Network::Stagenet, "")
            .expect("Failed to derive address from test vector 2");

        assert_eq!(address.to_string(), TEST_VECTOR_2_ADDRESS);

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_2_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_2_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_2_VIEW_KEY);
    }

    #[test]
    fn test_derive_address_test_vector_3_stagenet() {
        let address = derive_address(TEST_VECTOR_3_SEED, Network::Stagenet, "")
            .expect("Failed to derive address from test vector 3");

        assert_eq!(address.to_string(), TEST_VECTOR_3_ADDRESS);
    }

    #[test]
    fn test_derive_address_test_vector_4_english() {
        let _address = derive_address(TEST_VECTOR_4_SEED, Network::Mainnet, "")
            .expect("Failed to derive address from test vector 4");

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_4_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_4_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_4_VIEW_KEY);
    }

    #[test]
    fn test_derive_address_test_vector_5_spanish() {
        let _address = derive_address(TEST_VECTOR_5_SEED, Network::Mainnet, "")
            .expect("Failed to derive address from test vector 5");

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_5_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_5_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_5_VIEW_KEY);
    }

    #[test]
    fn test_derive_address_test_vector_6_french() {
        let _address = derive_address(TEST_VECTOR_6_SEED, Network::Mainnet, "")
            .expect("Failed to derive address from test vector 6");

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_6_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_6_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_6_VIEW_KEY);
    }

    #[test]
    fn test_derive_address_test_vector_7_german() {
        let _address = derive_address(TEST_VECTOR_7_SEED, Network::Mainnet, "")
            .expect("Failed to derive address from test vector 7");

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_7_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_7_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_7_VIEW_KEY);
    }

    #[test]
    fn test_derive_address_networks() {
        let mainnet_addr =
            derive_address(TEST_VECTOR_1_SEED, Network::Mainnet, "").expect("Failed for mainnet");
        assert!(mainnet_addr.to_string().starts_with("4"));

        let testnet_addr =
            derive_address(TEST_VECTOR_1_SEED, Network::Testnet, "").expect("Failed for testnet");
        assert!(
            testnet_addr.to_string().starts_with("9") || testnet_addr.to_string().starts_with("A")
        );

        let stagenet_addr =
            derive_address(TEST_VECTOR_2_SEED, Network::Stagenet, "").expect("Failed for stagenet");
        assert!(stagenet_addr.to_string().starts_with("5"));

        assert_ne!(mainnet_addr, testnet_addr);
        assert_ne!(mainnet_addr, stagenet_addr);
        assert_ne!(testnet_addr, stagenet_addr);
    }

    #[test]
    fn test_derive_address_deterministic() {
        let address1 = derive_address(TEST_VECTOR_1_SEED, Network::Mainnet, "")
            .expect("Failed first derivation");
        let address2 = derive_address(TEST_VECTOR_1_SEED, Network::Mainnet, "")
            .expect("Failed second derivation");

        assert_eq!(address1, address2);
        assert_eq!(address1.to_string().len(), 95);
    }

    #[test]
    fn test_derive_address_invalid_seed() {
        let result = derive_address("invalid seed words", Network::Mainnet, "");
        assert!(result.is_err());
    }

    #[test]
    fn test_seed_generation_and_address_derivation() {
        let seed = generate_seed(SeedFormat::Classic).expect("Failed to generate seed");

        let mainnet_address = derive_address(&seed, Network::Mainnet, "")
            .expect("Failed to derive mainnet address from generated seed");
        assert!(mainnet_address.to_string().starts_with("4"));
        assert_eq!(mainnet_address.to_string().len(), 95);

        let testnet_address = derive_address(&seed, Network::Testnet, "")
            .expect("Failed to derive testnet address from generated seed");
        assert!(
            testnet_address.to_string().starts_with("9")
                || testnet_address.to_string().starts_with("A")
        );

        let stagenet_address = derive_address(&seed, Network::Stagenet, "")
            .expect("Failed to derive stagenet address from generated seed");
        assert!(stagenet_address.to_string().starts_with("5"));
    }

    #[test]
    fn test_derive_keys_test_vector_1() {
        let keys = derive_keys(TEST_VECTOR_1_SEED, Network::Mainnet, "")
            .expect("Failed to derive keys from test vector 1");

        assert_eq!(
            hex::encode(keys.secret_spend_key.as_deref().unwrap()),
            TEST_VECTOR_1_SPEND_KEY
        );
        assert_eq!(hex::encode(&*keys.secret_view_key), TEST_VECTOR_1_VIEW_KEY);
        assert_eq!(keys.address.to_string(), TEST_VECTOR_1_ADDRESS);
    }

    #[test]
    fn test_derive_keys_test_vector_2_stagenet() {
        let keys = derive_keys(TEST_VECTOR_2_SEED, Network::Stagenet, "")
            .expect("Failed to derive keys from test vector 2");

        assert_eq!(
            hex::encode(keys.secret_spend_key.as_deref().unwrap()),
            TEST_VECTOR_2_SPEND_KEY
        );
        assert_eq!(hex::encode(&*keys.secret_view_key), TEST_VECTOR_2_VIEW_KEY);
        assert_eq!(keys.address.to_string(), TEST_VECTOR_2_ADDRESS);
    }

    #[test]
    fn test_default_lookahead_indices() {
        let indices = build_subaddress_indices(DEFAULT_LOOKAHEAD);
        assert_eq!(indices.len(), 50 * 200 - 1);
        assert!(indices.contains(&SubaddressIndex::new(0, 1).unwrap()));
        assert!(indices.contains(&SubaddressIndex::new(0, 199).unwrap()));
        assert!(indices.contains(&SubaddressIndex::new(49, 0).unwrap()));
        assert!(!indices.contains(&SubaddressIndex::new(0, 200).unwrap()));
        assert!(!indices.contains(&SubaddressIndex::new(50, 0).unwrap()));
    }

    #[test]
    fn test_wallet_cli_lookahead_sizes() {
        let software = build_subaddress_indices(WALLET_CLI_SOFTWARE_LOOKAHEAD);
        let hardware = build_subaddress_indices(WALLET_CLI_HARDWARE_LOOKAHEAD);
        assert_eq!(software.len(), (51 * 201) - 1);
        assert_eq!(hardware.len(), (6 * 21) - 1);
    }

    #[test]
    fn test_lookahead_constants() {
        // Verify DEFAULT_LOOKAHEAD structure
        assert_eq!(DEFAULT_LOOKAHEAD.account, 49);
        assert_eq!(DEFAULT_LOOKAHEAD.subaddress, 199);

        // Verify WALLET_CLI_SOFTWARE_LOOKAHEAD
        assert_eq!(WALLET_CLI_SOFTWARE_LOOKAHEAD.account, 50);
        assert_eq!(WALLET_CLI_SOFTWARE_LOOKAHEAD.subaddress, 200);

        // Verify WALLET_CLI_HARDWARE_LOOKAHEAD
        assert_eq!(WALLET_CLI_HARDWARE_LOOKAHEAD.account, 5);
        assert_eq!(WALLET_CLI_HARDWARE_LOOKAHEAD.subaddress, 20);
    }

    #[test]
    fn test_build_subaddress_indices_zero() {
        let lookahead = Lookahead {
            account: 0,
            subaddress: 0,
        };
        let indices = build_subaddress_indices(lookahead);
        // Should be empty since we skip (0, 0) which is the main address
        assert_eq!(indices.len(), 0);
    }

    #[test]
    fn test_build_subaddress_indices_single() {
        let lookahead = Lookahead {
            account: 0,
            subaddress: 1,
        };
        let indices = build_subaddress_indices(lookahead);
        assert_eq!(indices.len(), 1);
        assert!(indices.contains(&SubaddressIndex::new(0, 1).unwrap()));
    }

    #[test]
    fn test_spend_key_from_seed_different_seeds() {
        let seed1 = Seed::from_string(Zeroizing::new(TEST_VECTOR_1_SEED.to_string())).unwrap();
        let seed2 = Seed::from_string(Zeroizing::new(TEST_VECTOR_2_SEED.to_string())).unwrap();

        let spend1 = spend_key_from_seed(&seed1, "");
        let spend2 = spend_key_from_seed(&seed2, "");

        // Different seeds should produce different spend keys
        assert_ne!(spend1.compress().to_bytes(), spend2.compress().to_bytes());
    }

    #[test]
    fn test_view_key_from_seed_different_seeds() {
        let seed1 = Seed::from_string(Zeroizing::new(TEST_VECTOR_1_SEED.to_string())).unwrap();
        let seed2 = Seed::from_string(Zeroizing::new(TEST_VECTOR_2_SEED.to_string())).unwrap();

        let view1 = view_key_from_seed(&seed1, "");
        let view2 = view_key_from_seed(&seed2, "");

        // Different seeds should produce different view keys
        assert_ne!(view1.to_bytes(), view2.to_bytes());
    }

    #[test]
    fn test_derive_keys_invalid_seed() {
        let result = derive_keys("invalid seed phrase", Network::Mainnet, "");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_keys_are_typed_bytes() {
        let keys = derive_keys(TEST_VECTOR_1_SEED, Network::Mainnet, "").unwrap();
        assert!(keys.secret_spend_key.is_some());
        assert_ne!(*keys.secret_view_key, [0; 32]);
        assert_ne!(keys.public_spend_key, [0; 32]);
        assert_ne!(keys.public_view_key, [0; 32]);
    }

    #[test]
    fn test_block_scan_result_serialization() {
        let result = BlockScanResult {
            previous_block_hash: String::new(),
            block_height: 12345,
            block_hash: "abc123".to_string(),
            block_timestamp: 1234567890,
            tx_count: 5,
            outputs: vec![],
            daemon_height: 12350,
            spent_key_images: vec!["key1".to_string(), "key2".to_string()],
            spent_key_image_tx_hashes: vec!["tx1".to_string(), "tx2".to_string()],
            spent_key_image_rings: vec![],
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: BlockScanResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.block_height, deserialized.block_height);
        assert_eq!(result.tx_count, deserialized.tx_count);
        assert_eq!(result.spent_key_images.len(), 2);
    }

    #[test]
    fn test_owned_output_info_serialization() {
        let output = WalletOutput {
            tx_hash: "deadbeef".to_string(),
            output_index: 0,
            amount: 1000000000000,
            key: "key123".to_string(),
            key_offset: "offset456".to_string(),
            commitment_mask: "mask789".to_string(),
            subaddress_index: Some((0, 1)),
            payment_id: None,
            received_output_bytes: "bytes".to_string(),
            global_index: Some(42),
            ring: None,
            block_height: 100,
            spent: false,
            spent_height: None,
            key_image: "keyimage".to_string(),
            is_coinbase: false,
            frozen: false,
            pre_ringct: false,
        };

        let json = serde_json::to_string(&output).unwrap();
        let deserialized: WalletOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output.tx_hash, deserialized.tx_hash);
        assert_eq!(output.amount, deserialized.amount);
        assert_eq!(output.subaddress_index, deserialized.subaddress_index);
    }

    #[test]
    fn test_mempool_scan_result_serialization() {
        let result = MempoolScanResult {
            tx_count: 3,
            outputs: vec![],
            spent_key_images: vec!["ki1".to_string()],
            spent_key_image_tx_hashes: vec!["tx1".to_string()],
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: MempoolScanResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.tx_count, deserialized.tx_count);
    }

    #[test]
    fn test_mempool_account_filter_keeps_selected_accounts() {
        fn output(tx_hash: &str, account: Option<u32>) -> WalletOutput {
            WalletOutput {
                tx_hash: tx_hash.to_string(),
                output_index: 0,
                amount: 1,
                key: "key".to_string(),
                key_offset: "offset".to_string(),
                commitment_mask: "mask".to_string(),
                subaddress_index: account.map(|account| (account, 0)),
                payment_id: None,
                received_output_bytes: "bytes".to_string(),
                global_index: None,
                ring: None,
                block_height: 0,
                spent: false,
                spent_height: None,
                key_image: String::new(),
                is_coinbase: false,
                frozen: false,
                pre_ringct: false,
            }
        }

        let mut outputs = vec![
            output("account_0_explicit", Some(0)),
            output("account_1", Some(1)),
            output("account_2", Some(2)),
            output("account_0_default", None),
        ];

        retain_mempool_outputs_for_accounts(&mut outputs, Some(&[0, 2]));
        let tx_hashes = outputs
            .iter()
            .map(|output| output.tx_hash.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            tx_hashes,
            vec!["account_0_explicit", "account_2", "account_0_default"]
        );
    }

    #[test]
    fn test_mempool_account_filter_none_keeps_all_outputs() {
        let mut outputs = vec![WalletOutput {
            tx_hash: "kept".to_string(),
            output_index: 0,
            amount: 1,
            key: "key".to_string(),
            key_offset: "offset".to_string(),
            commitment_mask: "mask".to_string(),
            subaddress_index: Some((1, 0)),
            payment_id: None,
            received_output_bytes: "bytes".to_string(),
            global_index: None,
            ring: None,
            block_height: 0,
            spent: false,
            spent_height: None,
            key_image: String::new(),
            is_coinbase: false,
            frozen: false,
            pre_ringct: false,
        }];

        retain_mempool_outputs_for_accounts(&mut outputs, None);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].tx_hash, "kept");
    }

    #[test]
    fn test_wallet_scan_config_clone() {
        let config = WalletScanConfig {
            mnemonic: TEST_VECTOR_1_SEED.to_string(),
            network: Network::Mainnet,
            lookahead: DEFAULT_LOOKAHEAD,
            passphrase: String::new(),
        };

        let cloned = config.clone();
        assert_eq!(config.mnemonic, cloned.mnemonic);
        assert_eq!(config.network, cloned.network);
        assert_eq!(config.lookahead.account, cloned.lookahead.account);
        assert_eq!(config.lookahead.subaddress, cloned.lookahead.subaddress);
    }

    #[test]
    fn test_lookahead_equality() {
        let lookahead1 = Lookahead {
            account: 0,
            subaddress: 20,
        };
        let lookahead2 = Lookahead {
            account: 0,
            subaddress: 20,
        };
        let lookahead3 = Lookahead {
            account: 1,
            subaddress: 20,
        };

        assert_eq!(lookahead1, lookahead2);
        assert_ne!(lookahead1, lookahead3);
    }

    #[test]
    fn test_watermark_new_normal_lookahead() {
        let la = Lookahead {
            account: 2,
            subaddress: 5,
        };
        let wm = SubaddressWatermark::new(la);
        assert_eq!(wm.max_minor_per_account.len(), 3); // accounts 0,1,2
        assert!(wm.max_minor_per_account.iter().all(|&v| v == 5));
    }

    #[test]
    fn test_watermark_new_zero_lookahead() {
        let la = Lookahead {
            account: 0,
            subaddress: 0,
        };
        let wm = SubaddressWatermark::new(la);
        assert!(wm.max_minor_per_account.is_empty());
    }

    #[test]
    fn test_expand_subaddress_within_account() {
        // Lookahead (2,5), discovery at (0,4) -> should expand minor to 9
        let la = Lookahead {
            account: 2,
            subaddress: 5,
        };
        let spend = Scalar::from(42u64);
        let spend_point = &spend * ED25519_BASEPOINT_TABLE;
        let view = Scalar::from(99u64);
        let mut scanner = oxide_scanner_from_keys(&spend_point, &view).unwrap();
        register_subaddresses(&mut scanner, la);
        let mut wm = SubaddressWatermark::new(la);

        let found = SubaddressIndex::new(0, 4).unwrap();
        let expanded = expand_subaddresses_if_needed(&mut scanner, &mut wm, la, found);
        assert!(expanded);
        assert_eq!(wm.max_minor_per_account[0], 9); // 4 + 5
        assert_eq!(wm.max_minor_per_account.len(), 3); // accounts unchanged
    }

    #[test]
    fn test_expand_account_and_subaddress() {
        // Lookahead (2,5), discovery at (1,3) -> accounts should expand to 3,
        // and account 1's minor should expand to 8
        let la = Lookahead {
            account: 2,
            subaddress: 5,
        };
        let spend = Scalar::from(42u64);
        let spend_point = &spend * ED25519_BASEPOINT_TABLE;
        let view = Scalar::from(99u64);
        let mut scanner = oxide_scanner_from_keys(&spend_point, &view).unwrap();
        register_subaddresses(&mut scanner, la);
        let mut wm = SubaddressWatermark::new(la);

        let found = SubaddressIndex::new(1, 3).unwrap();
        let expanded = expand_subaddresses_if_needed(&mut scanner, &mut wm, la, found);
        assert!(expanded);
        assert_eq!(wm.max_minor_per_account.len(), 4); // accounts 0,1,2,3
        assert_eq!(wm.max_minor_per_account[1], 8); // 3 + 5
        assert_eq!(wm.max_minor_per_account[3], 5); // new account gets default
    }

    #[test]
    fn test_expand_zero_lookahead_noop() {
        let la = Lookahead {
            account: 0,
            subaddress: 0,
        };
        let spend = Scalar::from(42u64);
        let spend_point = &spend * ED25519_BASEPOINT_TABLE;
        let view = Scalar::from(99u64);
        let mut scanner = oxide_scanner_from_keys(&spend_point, &view).unwrap();
        let mut wm = SubaddressWatermark::new(la);

        let found = SubaddressIndex::new(0, 1).unwrap();
        let expanded = expand_subaddresses_if_needed(&mut scanner, &mut wm, la, found);
        assert!(!expanded);
    }

    #[test]
    fn test_expand_idempotent() {
        let la = Lookahead {
            account: 2,
            subaddress: 5,
        };
        let spend = Scalar::from(42u64);
        let spend_point = &spend * ED25519_BASEPOINT_TABLE;
        let view = Scalar::from(99u64);
        let mut scanner = oxide_scanner_from_keys(&spend_point, &view).unwrap();
        register_subaddresses(&mut scanner, la);
        let mut wm = SubaddressWatermark::new(la);

        let found = SubaddressIndex::new(0, 4).unwrap();
        assert!(expand_subaddresses_if_needed(
            &mut scanner,
            &mut wm,
            la,
            found
        ));
        // Second call with same index should not expand further
        assert!(!expand_subaddresses_if_needed(
            &mut scanner,
            &mut wm,
            la,
            found
        ));
        assert_eq!(wm.max_minor_per_account[0], 9);
    }

    #[test]
    fn test_expand_no_expansion_within_window() {
        // Discovery at (0,2) with lookahead (2,5) -> already covered (max=5, 2+5=7>5 -> expands)
        // Actually 2+5=7 > 5, so it does expand.
        // Discovery at (0,0) with lookahead (2,5) -> 0+5=5 == current max 5, no expansion
        let la = Lookahead {
            account: 2,
            subaddress: 5,
        };
        let spend = Scalar::from(42u64);
        let spend_point = &spend * ED25519_BASEPOINT_TABLE;
        let view = Scalar::from(99u64);
        let mut scanner = oxide_scanner_from_keys(&spend_point, &view).unwrap();
        register_subaddresses(&mut scanner, la);
        let mut wm = SubaddressWatermark::new(la);

        // SubaddressIndex::new(0, 0) returns None (primary), use (0, 1)
        // Discovery at (0,0) would be None. Use (1,0):
        // needed_account = 0+2 = 2, current_max = 2 -> no account expansion
        // needed_address = 0+5 = 5, current_max = 5 -> no subaddress expansion
        // But SubaddressIndex::new(1,0) returns Some since it's not (0,0)
        let found = SubaddressIndex::new(1, 0).unwrap();
        let expanded = expand_subaddresses_if_needed(&mut scanner, &mut wm, la, found);
        // needed_account = 1+2 = 3 > 2 -> account expansion
        assert!(expanded);
    }

    #[test]
    fn output_indices_are_checked_against_the_block() {
        use crate::monero_rpc::TxOutputIndices;
        use monero_oxide::ed25519::CompressedPoint;
        use monero_oxide::transaction::Output;

        let basepoint = curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
        let output = |amount| Output {
            amount,
            key: CompressedPoint::from(basepoint.compress().to_bytes()),
            view_tag: None,
        };
        let prefix = |inputs, outputs| OxideTransactionPrefix {
            additional_timelock: OxideTimelock::None,
            inputs,
            outputs,
            extra: vec![],
        };
        let header = OxideBlockHeader {
            hardfork_version: 5,
            hardfork_signal: 0,
            timestamp: 0,
            previous: [0; 32],
            nonce: 0,
        };
        let tx = OxideTransaction::<Pruned>::V2 {
            prefix: prefix(vec![], vec![output(None), output(None)]),
            proofs: None,
        };
        let ringct_miner = OxideTransaction::<NotPruned>::V2 {
            prefix: prefix(vec![OxideInput::Gen(9)], vec![output(Some(1))]),
            proofs: None,
        };
        let old_miner = OxideTransaction::<NotPruned>::V1 {
            prefix: prefix(vec![OxideInput::Gen(9)], vec![output(Some(1))]),
            signatures: vec![],
        };
        let block = |miner| OxideBlock::new(header.clone(), miner, vec![[7; 32]]).unwrap();
        let indices = |entries: &[&[u64]]| BlockOutputIndices {
            indices: entries
                .iter()
                .map(|entry| TxOutputIndices {
                    indices: entry.to_vec(),
                })
                .collect(),
        };
        let first = |block: &OxideBlock, entries: &[&[u64]]| {
            first_ringct_index_from_rpc(block, std::slice::from_ref(&tx), Some(&indices(entries)))
        };

        let ringct = block(ringct_miner);
        assert_eq!(first(&ringct, &[&[39], &[40, 41]]), Ok(Some(39)));
        assert_eq!(first(&ringct, &[&[], &[40, 41]]), Ok(Some(39)));
        assert_eq!(first(&ringct, &[&[40, 41]]), Ok(Some(39)));
        assert!(first(&ringct, &[&[39], &[41, 42]]).is_err());
        assert!(first(&ringct, &[&[39], &[40]]).is_err());
        assert!(first(&ringct, &[&[39], &[40, 40]]).is_err());
        assert!(first(&ringct, &[&[39], &[40, 41], &[42]]).is_err());
        assert!(first(&ringct, &[&[39], &[]]).is_err());
        assert!(first_ringct_index_from_rpc(&ringct, std::slice::from_ref(&tx), None).is_err());

        let old = block(old_miner);
        assert_eq!(first(&old, &[&[5], &[40, 41]]), Ok(Some(40)));
        assert_eq!(first(&old, &[&[40, 41]]), Ok(Some(40)));
        assert_eq!(first_ringct_index_from_rpc(&old, &[], None), Ok(None));
    }

    #[test]
    fn version_1_output_indices_are_per_amount_and_checked() {
        use crate::monero_rpc::TxOutputIndices;
        use monero_oxide::ed25519::CompressedPoint;
        use monero_oxide::transaction::Output;

        let basepoint = curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
        let output = |amount| Output {
            amount,
            key: CompressedPoint::from(basepoint.compress().to_bytes()),
            view_tag: None,
        };
        let prefix = |inputs, outputs| OxideTransactionPrefix {
            additional_timelock: OxideTimelock::None,
            inputs,
            outputs,
            extra: vec![],
        };
        let header = OxideBlockHeader {
            hardfork_version: 4,
            hardfork_signal: 0,
            timestamp: 0,
            previous: [0; 32],
            nonce: 0,
        };
        let miner = OxideTransaction::<NotPruned>::V1 {
            prefix: prefix(vec![OxideInput::Gen(9)], vec![output(Some(300))]),
            signatures: vec![],
        };
        let miner_hash = miner.hash();
        let old = OxideTransaction::<Pruned>::V1 {
            prefix: prefix(
                vec![],
                vec![output(Some(100)), output(Some(200)), output(Some(100))],
            ),
            signatures: (),
        };
        let ringct = OxideTransaction::<Pruned>::V2 {
            prefix: prefix(vec![], vec![output(None)]),
            proofs: None,
        };
        let txs = vec![([1; 32], old), ([2; 32], ringct)];
        let block = OxideBlock::new(header, miner, vec![[1; 32], [2; 32]]).unwrap();
        let indices = |entries: &[&[u64]]| BlockOutputIndices {
            indices: entries
                .iter()
                .map(|entry| TxOutputIndices {
                    indices: entry.to_vec(),
                })
                .collect(),
        };
        let check = |entries: &[&[u64]]| {
            version_1_output_indices(&block, &txs, Some(&indices(entries))).map(|found| {
                let mut found: Vec<([u8; 32], Option<Vec<u64>>)> = found
                    .into_iter()
                    .map(|(hash, sent)| (hash, sent.map(<[u64]>::to_vec)))
                    .collect();
                found.sort();
                found
            })
        };
        let expect = |miner: Option<Vec<u64>>| {
            let mut expected = vec![([1; 32], Some(vec![7, 3, 8])), (miner_hash, miner)];
            expected.sort();
            expected
        };

        assert_eq!(
            check(&[&[12], &[7, 3, 8], &[40]]),
            Ok(expect(Some(vec![12])))
        );
        assert_eq!(check(&[&[], &[7, 3, 8], &[40]]), Ok(expect(None)));
        assert_eq!(check(&[&[7, 3, 8], &[40]]), Ok(expect(None)));
        assert!(check(&[&[12], &[7, 3], &[40]]).is_err());
        assert!(check(&[&[12], &[7, 3, 8, 9], &[40]]).is_err());
        assert!(check(&[&[12], &[7, 3, 9], &[40]]).is_err());
        assert!(check(&[&[12], &[7, 3, 7], &[40]]).is_err());
        assert!(check(&[&[12, 13], &[7, 3, 8], &[40]]).is_err());
        assert!(check(&[&[12], &[u64::MAX, 3, 8], &[40]]).is_err());
        assert!(check(&[&[7, 3, 8]]).is_err());
        assert!(version_1_output_indices(&block, &txs, None).is_err());

        let ringct_only = &txs[1..];
        let header = block.header.clone();
        let miner = block.miner_transaction().clone();
        let block = OxideBlock::new(header, miner, vec![[2; 32]]).unwrap();
        let found = version_1_output_indices(&block, ringct_only, None).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found.get(&miner_hash), Some(&None));
    }

    #[test]
    fn test_hex_to_hash_valid() {
        let hex = "0000000000000000000000000000000000000000000000000000000000000001";
        let result = hex_to_hash(hex).unwrap();
        assert_eq!(result[31], 1);
        assert_eq!(result[0], 0);
    }

    #[test]
    fn test_hex_to_hash_invalid_hex() {
        let result = hex_to_hash("not_hex");
        assert!(result.is_err());
    }

    #[test]
    fn test_hex_to_hash_wrong_length() {
        let result = hex_to_hash("0011");
        assert!(result.is_err());
    }

    /// Feather Wallet test vector: polyseed + passphrase "hunter2" through derive_keys.
    #[test]
    fn test_derive_keys_polyseed_passphrase_feather_vector() {
        let seed = "shoot exhibit rebuild laptop drive come off yard infant session subject tree steak antique liar hybrid";

        // Mainnet keys
        let keys = derive_keys(seed, Network::Mainnet, "hunter2").unwrap();
        assert_eq!(
            hex::encode(keys.secret_spend_key.as_deref().unwrap()),
            "08813258e8b396b2629ae9ecccd95ae33ec269ab0813754f43edfae937304909"
        );
        assert_eq!(
            hex::encode(&*keys.secret_view_key),
            "1be97a06de0e8e32952e41fa80023e5e0d96af4e61d6d1c2f521f94d12c3170b"
        );
        assert_eq!(
            hex::encode(keys.public_spend_key),
            "b4e3d0ed0ab2a22cb567d2edb7d33e402d0bf1a38b75fb1adb7cca6116f2b18e"
        );
        assert_eq!(
            hex::encode(keys.public_view_key),
            "c9efbf531801051471e29c7c2cc36458e10f5a4f2041842c818ed1654c938fde"
        );
        assert_eq!(keys.address.to_string(), "48UhC3g9s9T8UjKHj1GQsPBjb4To7AS585VYujqCfgEcQtEMx2CKgHA4RLmRYqjsG7FsErzHZbeUb8SmMihoYme6S4MBnaZ");

        // Stagenet keys (the Feather wallet was on Stagenet)
        let keys_stagenet = derive_keys(seed, Network::Stagenet, "hunter2").unwrap();
        assert_eq!(keys_stagenet.address.to_string(), "58gjGtb7WkZ8UjKHj1GQsPBjb4To7AS585VYujqCfgEcQtEMx2CKgHA4RLmRYqjsG7FsErzHZbeUb8SmMihoYme6S9wf8gy");

        // Without passphrase: different keys
        let keys_no_pass = derive_keys(seed, Network::Mainnet, "").unwrap();
        assert_ne!(keys_no_pass.address, keys.address);
    }

    /// Reject a short reply before `zip` can silently leave key images unchecked.
    #[test]
    fn spent_status_shorter_than_the_request_is_refused() {
        let err = check_spent_status(3, vec![0, 1]).expect_err("a short reply must be refused");
        assert!(err.contains("2 statuses for 3 key images"), "{err}");
    }

    #[test]
    fn spent_status_longer_than_the_request_is_refused() {
        assert!(check_spent_status(1, vec![0, 1]).is_err());
    }

    /// Accept a response with exactly one status per requested key image.
    #[test]
    fn spent_status_matching_the_request_is_accepted() {
        assert_eq!(check_spent_status(2, vec![0, 1]).unwrap(), vec![0, 1]);
        assert_eq!(check_spent_status(0, vec![]).unwrap(), Vec::<u32>::new());
    }
}
