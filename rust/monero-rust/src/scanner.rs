//! Unified scanning implementation for Monero wallets.
//!
//! This module provides wallet scanning functionality that works across
//! both native and WASM targets through generic RpcConnection support.

use crate::monero_rpc::{BlockOutputIndices, GetBlocksFastResponse, Rpc, RpcConnection};
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
use std::collections::{HashMap, HashSet};
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
                for _ in 0..num_offsets {
                    if read_varint(&mut cursor).is_none() {
                        break;
                    }
                }
                let mut ki = [0u8; 32];
                if cursor.read_exact(&mut ki).is_err() {
                    break;
                }
                key_images.push(hex::encode(ki));
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
    pub block_timestamp: u64,
    pub tx_count: usize,
    pub daemon_height: u64,
    pub spent_key_images: Vec<String>,
    pub spent_key_image_tx_hashes: Vec<String>,
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
#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct WalletScanConfig {
    pub mnemonic: String,
    #[zeroize(skip)]
    pub network: Network,
    pub lookahead: Lookahead,
    pub passphrase: String,
}

#[derive(Debug, Clone)]
pub struct DerivedKeys {
    /// `None` for view-only wallets.
    pub secret_spend_key: Option<Zeroizing<[u8; 32]>>,
    pub secret_view_key: Zeroizing<[u8; 32]>,
    pub public_spend_key: [u8; 32],
    pub public_view_key: [u8; 32],
    pub address: MoneroAddress,
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
    use crate::monero_rpc::HttpRpc;
    let rpc = HttpRpc::new(node_url.to_string())
        .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
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
    use crate::monero_rpc::HttpRpc;
    let rpc = HttpRpc::new(node_url.to_string())
        .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
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
    use crate::monero_rpc::HttpRpc;
    let rpc =
        HttpRpc::new(node_url.to_string()).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
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

/// The global index of a block's first RingCT output, from its `/getblocks.bin`
/// output indices; `None` when they give none.
fn first_ringct_index_from_rpc(
    block: &OxideBlock,
    txs: &[OxideTransaction<Pruned>],
    indices: Option<&BlockOutputIndices>,
) -> Option<u64> {
    let Some(block_indices) = indices else {
        return None;
    };
    let entries = block_indices.indices.as_slice();
    let miner = block.miner_transaction();
    let miner_is_v2 = miner.version() == 2;

    // Layout A: the miner transaction is the first entry.
    if entries.len() == 1 + block.transactions.len() {
        if miner_is_v2 {
            if let Some(&first) = entries[0].indices.first() {
                return Some(first);
            }
        }
        return first_ringct_index_from_tx_entries(&entries[1..], txs, 0);
    }

    // Layout B: entries parallel the non-miner transactions only.
    if entries.len() == block.transactions.len() {
        let miner_outputs = if miner_is_v2 {
            miner.prefix().outputs.len() as u64
        } else {
            0
        };
        return first_ringct_index_from_tx_entries(entries, txs, miner_outputs);
    }

    None
}

/// Walk non-miner transactions until one carries daemon output indices and
/// derive the block's first RingCT index from it.
fn first_ringct_index_from_tx_entries(
    entries: &[crate::monero_rpc::TxOutputIndices],
    txs: &[OxideTransaction<Pruned>],
    mut ringct_outputs_before: u64,
) -> Option<u64> {
    for (entry, tx) in entries.iter().zip(txs) {
        if !matches!(tx, OxideTransaction::V2 { .. }) {
            // v1 outputs don't occupy RingCT indices and their daemon indices
            // are per-amount, not global.
            continue;
        }
        if let Some(&first) = entry.indices.first() {
            return Some(first.saturating_sub(ringct_outputs_before));
        }
        ringct_outputs_before += tx.prefix().outputs.len() as u64;
    }
    None
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

/// Scan a single transaction outside its block context (mempool, or blocks
/// where a sibling transaction blob couldn't be parsed).
///
/// `monero-wallet` only scans whole blocks, so the transaction is wrapped in
/// a synthetic single-transaction block. The RingCT indices embedded in the
/// returned outputs are relative to the transaction, not the chain.
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

/// Scan already-parsed transactions one at a time, expanding the subaddress
/// window like `scan_parsed_block_expanding`. Used when a block contains a
/// transaction blob the parser rejected, so the whole block can't be scanned.
fn scan_parsed_txs_individually(
    scanner: &mut Scanner,
    watermark: &mut SubaddressWatermark,
    lookahead: Lookahead,
    txs_with_hashes: &[([u8; 32], OxideTransaction<Pruned>)],
) -> Result<Vec<OxideWalletOutput>, String> {
    loop {
        let mut outputs = Vec::new();
        for (hash, tx) in txs_with_hashes {
            outputs.extend(scan_single_transaction(scanner, *hash, tx)?);
        }
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

/// Map a `monero-wallet` payment ID into the hex representation the previous
/// scanner reported: encrypted IDs of all zeroes mean "no payment ID".
fn oxide_payment_id_hex(payment_id: PaymentId) -> Option<String> {
    match payment_id {
        PaymentId::Unencrypted(id) => Some(hex::encode(id)),
        PaymentId::Encrypted(id) if id == [0; 8] => None,
        PaymentId::Encrypted(id) => Some(hex::encode(id)),
    }
}

/// Convert a scanned `monero-wallet` output into the wallet output model.
///
/// `key_image_scalar` carries the private spend key when key images should be
/// filled at scan time (WASM full wallets); `None` leaves them empty.
fn map_oxide_output(
    output: &OxideWalletOutput,
    block_height: u64,
    miner_tx_hash: [u8; 32],
    key_image_scalar: Option<&Scalar>,
    global_index_is_authoritative: bool,
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
        global_index: global_index_is_authoritative.then(|| output.index_on_blockchain()),
        ring: None,
        block_height,
        spent: false,
        spent_height: None,
        key_image,
        is_coinbase: output.transaction() == miner_tx_hash,
        frozen: false,
    })
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

/// Check that consecutive parsed blocks link through their parent hashes.
///
/// Height checks alone do not prevent blocks from different chains being combined
/// in one batch. Pre-RingCT entries are not parsed, so they interrupt this check.
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
///
/// `/getblocks.bin` supplies blobs separately from the block. Matching the counts
/// does not prevent substitution: each parsed transaction must hash to the value
/// committed by the block before its outputs can be credited.
///
/// Return parsed transactions and key images extracted from unparseable blobs.
/// The latter cannot be hash-checked and can only mark outputs spent; they never
/// credit outputs. A hostile node can therefore still cause false spent reports
/// through this fallback.
fn parse_block_transaction_blobs(
    block: &OxideBlock,
    blobs: &[Vec<u8>],
    pruned: bool,
    block_height: u64,
) -> Result<(Vec<([u8; 32], OxideTransaction<Pruned>)>, Vec<String>), String> {
    let mut parsed = Vec::with_capacity(blobs.len());
    let mut skipped_key_images = Vec::new();

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
            None => skipped_key_images.extend(extract_key_images_from_raw_tx(blob)),
        }
    }

    Ok((parsed, skipped_key_images))
}

/// Scan a batch of blocks fetched via `/getblocks.bin` for outputs belonging to the wallet.
///
/// This is dramatically faster than scanning one block at a time because it fetches
/// up to ~1000 blocks in a single RPC call instead of 4 calls per block.
///
/// The scanner is set up once and reused across all blocks in the batch.
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
    // Track the previous parsed block's hash. Clear it after a pre-RingCT entry,
    // which cannot participate in the parent-hash check.
    let mut previous_block_hash: Option<[u8; 32]> = None;

    #[cfg(target_arch = "wasm32")]
    let mut last_yield_ms = js_sys::Date::now();

    for (block_idx, block_entry) in response.blocks.iter().enumerate() {
        let block_height = response.start_height + block_idx as u64;

        // Pre-RingCT blocks can't be parsed by monero-serai
        let major_version = block_entry.block.first().copied().unwrap_or(0);
        if major_version < 4 {
            // Pre-RingCT block: monero-serai can't parse it, so we can't
            // compute the proper block ID. Use raw blob hash as a
            // placeholder; these blocks are deep in history and won't
            // appear in reorg detection.
            previous_block_hash = None;
            let block_hash = hex::encode(Keccak256::digest(&block_entry.block));
            results.push(BlockScanResult {
                previous_block_hash: String::new(),
                block_height,
                block_hash,
                block_timestamp: 0,
                daemon_height,
                outputs: vec![],
                spent_key_images: vec![],
                spent_key_image_tx_hashes: vec![],
                tx_count: 0,
            });
            continue;
        }

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
            let (parsed_txs, skipped_key_images) = parse_block_transaction_blobs(
                &block,
                &block_entry.txs,
                block_entry.pruned,
                block_height,
            )?;

            let skipped_count = skipped_key_images.len();
            let mut spent_key_images = skipped_key_images;
            let mut spent_key_image_tx_hashes: Vec<String> = vec![String::new(); skipped_count];
            for (tx_hash, tx) in std::iter::once((miner_tx_hash, &miner_tx))
                .chain(parsed_txs.iter().map(|(hash, tx)| (*hash, tx)))
            {
                let tx_hash_hex = hex::encode(tx_hash);
                for input in &tx.prefix().inputs {
                    if let OxideInput::ToKey { key_image, .. } = input {
                        spent_key_images.push(hex::encode(key_image.to_bytes()));
                        spent_key_image_tx_hashes.push(tx_hash_hex.clone());
                    }
                }
            }

            let (scanned, global_index_is_authoritative) =
                if parsed_txs.len() == block.transactions.len() {
                    let scan_txs: Vec<OxideTransaction<Pruned>> =
                        parsed_txs.iter().map(|(_, tx)| tx.clone()).collect();
                    let first_ringct_index = first_ringct_index_from_rpc(
                        &block,
                        &scan_txs,
                        response.output_indices.get(block_idx),
                    );
                    let scanned = scan_parsed_block_expanding(
                        &mut scanner.scanner,
                        &mut scanner.watermark,
                        scanner.lookahead,
                        &block,
                        &scan_txs,
                        first_ringct_index.unwrap_or(0),
                    )?;
                    (scanned, first_ringct_index.is_some())
                } else {
                    // A transaction blob was rejected by the parser: the whole
                    // block can't be scanned, so scan the parseable transactions
                    // individually.
                    let mut txs_with_hashes = vec![(miner_tx_hash, miner_tx.clone())];
                    txs_with_hashes.extend(parsed_txs.iter().cloned());
                    let scanned = scan_parsed_txs_individually(
                        &mut scanner.scanner,
                        &mut scanner.watermark,
                        scanner.lookahead,
                        &txs_with_hashes,
                    )?;
                    (scanned, false)
                };

            let key_image_scalar = scanner.spend_scalar.as_ref().map(|scalar| &**scalar);

            let mut outputs = Vec::with_capacity(scanned.len());
            for output in &scanned {
                outputs.push(map_oxide_output(
                    output,
                    block_height,
                    miner_tx_hash,
                    key_image_scalar,
                    global_index_is_authoritative,
                )?);
            }
            if outputs.iter().any(|output| output.global_index.is_none()) {
                return Err(format!(
                    "Block {block_height} contains a wallet output but no authoritative global \
                     output index; refuse to store an output that cannot be spent privately"
                ));
            }

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
    use crate::monero_rpc::HttpRpc;
    let rpc =
        HttpRpc::new(node_url.to_string()).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
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

        // Pre-RingCT blocks can't be parsed by monero-serai
        let major_version = block_entry.block.first().copied().unwrap_or(0);
        if major_version < 4 {
            // Pre-RingCT block: monero-serai can't parse it, so we can't
            // compute the proper block ID. Use raw blob hash as a
            // placeholder; these blocks are deep in history and won't
            // appear in reorg detection.
            previous_block_hash = None;
            let block_hash = hex::encode(Keccak256::digest(&block_entry.block));
            let mut wallet_results = HashMap::new();
            for entry in &cached_scanners.entries {
                wallet_results.insert(
                    entry.address.clone(),
                    WalletScanData {
                        address: entry.address.clone(),
                        outputs: vec![],
                    },
                );
            }
            results.push(MultiWalletScanResult {
                block_height,
                block_hash,
                block_timestamp: 0,
                tx_count: 0,
                daemon_height: response.current_height,
                spent_key_images: vec![],
                spent_key_image_tx_hashes: vec![],
                wallet_results,
            });
            continue;
        }

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
            let (parsed_txs, skipped_key_images) = parse_block_transaction_blobs(
                &block,
                &block_entry.txs,
                block_entry.pruned,
                block_height,
            )?;

            let skipped_count = skipped_key_images.len();
            let mut spent_key_images = skipped_key_images;
            let mut spent_key_image_tx_hashes: Vec<String> = vec![String::new(); skipped_count];
            for (tx_hash, tx) in std::iter::once((miner_tx_hash, &miner_tx))
                .chain(parsed_txs.iter().map(|(hash, tx)| (*hash, tx)))
            {
                let tx_hash_hex = hex::encode(tx_hash);
                for input in &tx.prefix().inputs {
                    if let OxideInput::ToKey { key_image, .. } = input {
                        spent_key_images.push(hex::encode(key_image.to_bytes()));
                        spent_key_image_tx_hashes.push(tx_hash_hex.clone());
                    }
                }
            }

            let whole_block = parsed_txs.len() == block.transactions.len();
            let scan_txs: Vec<OxideTransaction<Pruned>> =
                parsed_txs.iter().map(|(_, tx)| tx.clone()).collect();
            let first_ringct_index = first_ringct_index_from_rpc(
                &block,
                &scan_txs,
                response.output_indices.get(block_idx),
            );

            let mut wallet_results = HashMap::new();
            for entry in &mut cached_scanners.entries {
                let scanned = if whole_block {
                    scan_parsed_block_expanding(
                        &mut entry.scanner,
                        &mut entry.watermark,
                        entry.lookahead,
                        &block,
                        &scan_txs,
                        first_ringct_index.unwrap_or(0),
                    )?
                } else {
                    // A transaction blob was rejected by the parser: the whole
                    // block can't be scanned, so scan the parseable
                    // transactions individually.
                    let mut txs_with_hashes = vec![(miner_tx_hash, miner_tx.clone())];
                    txs_with_hashes.extend(parsed_txs.iter().cloned());
                    scan_parsed_txs_individually(
                        &mut entry.scanner,
                        &mut entry.watermark,
                        entry.lookahead,
                        &txs_with_hashes,
                    )?
                };

                let key_image_scalar = entry.spend_scalar.as_ref().map(|scalar| &**scalar);

                let mut outputs = Vec::with_capacity(scanned.len());
                for output in &scanned {
                    outputs.push(map_oxide_output(
                        output,
                        block_height,
                        miner_tx_hash,
                        key_image_scalar,
                        whole_block && first_ringct_index.is_some(),
                    )?);
                }
                if outputs.iter().any(|output| output.global_index.is_none()) {
                    return Err(format!(
                        "Block {block_height} contains a wallet output but no authoritative global \
                         output index; refuse to store an output that cannot be spent privately"
                    ));
                }
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
                block_timestamp,
                tx_count,
                daemon_height,
                spent_key_images,
                spent_key_image_tx_hashes,
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
    use crate::monero_rpc::HttpRpc;
    let rpc =
        HttpRpc::new(node_url.to_string()).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    scan_blocks_batch_multi_wallet(&rpc, start_height, wallet_configs, prune).await
}

/// Opaque wrapper around `GetBlocksFastResponse` for double-buffered pipelining.
///
/// The hub crate depends on `monero-rust` but not `monero-serai`, so it cannot
/// use `GetBlocksFastResponse` directly.
pub struct FetchedBlocks {
    response: GetBlocksFastResponse,
    /// Requested start height, retained for validation against the daemon's response.
    requested_start_height: u64,
    /// Wallet-supplied anchors for the zero-height history negotiation sentinel.
    requested_history: Vec<(u64, [u8; 32])>,
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
}

/// Fetch a batch of blocks without scanning them.
///
/// This is the fetch-only half of `scan_blocks_batch_with_url`, designed for
/// double-buffered pipelining: fetch the next batch while processing the current one.
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
        requested_start_height: start_height,
        requested_history: Vec::new(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_blocks_batch_with_url(
    node_url: &str,
    start_height: u64,
    prune: bool,
) -> Result<FetchedBlocks, String> {
    use crate::monero_rpc::HttpRpc;
    let rpc =
        HttpRpc::new(node_url.to_string()).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
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
    let (results, _cached) = process_batch_response(
        fetched.response,
        validated_start_height,
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
    process_batch_response(
        fetched.response,
        validated_start_height,
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

/// Scan a batch of blocks using a history of known hashes for fork-point detection.
///
/// Unlike `scan_blocks_batch` which sends only one hash, this sends a sparse
/// exponential history of hashes (matching wallet2's algorithm) so the daemon
/// can detect forks further back in the chain.
///
/// Returns `(results, actual_start_height)`; the daemon may return blocks
/// starting earlier than `start_height` if a fork was detected. With nonempty
/// history, `start_height = 0` asks the daemon to select a common starting block;
/// its hash and height must match an entry in `known_hashes`. Empty history keeps
/// zero as a literal genesis request.
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
    use crate::monero_rpc::HttpRpc;
    let rpc =
        HttpRpc::new(node_url.to_string()).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
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

/// Fetch a batch of blocks using known hash history, without scanning.
///
/// This is the fetch-only half for double-buffered pipelining with reorg awareness.
pub async fn fetch_blocks_batch_with_history<R: RpcConnection>(
    rpc: &Rpc<R>,
    start_height: u64,
    known_hashes: &[(u64, String)],
    prune: bool,
) -> Result<(FetchedBlocks, u64), String> {
    let requested_history = known_hashes
        .iter()
        .map(|(height, hash)| Ok((*height, hex_to_hash(hash)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let block_ids: Vec<[u8; 32]> = if requested_history.is_empty() {
        let anchor_height = start_height.saturating_sub(1);
        let hash = rpc
            .get_block_hash(anchor_height as usize)
            .await
            .map_err(|e| format!("Failed to get block hash at {}: {:?}", anchor_height, e))?;
        vec![hash]
    } else {
        requested_history.iter().map(|(_, hash)| *hash).collect()
    };
    let response = rpc
        .get_blocks_fast(&block_ids, start_height, prune)
        .await
        .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;
    let actual_start = response.start_height;
    let fetched = FetchedBlocks {
        response,
        requested_start_height: start_height,
        requested_history,
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
    use crate::monero_rpc::HttpRpc;
    let rpc =
        HttpRpc::new(node_url.to_string()).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
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
    use crate::monero_rpc::HttpRpc;
    let rpc =
        HttpRpc::new(node_url.to_string()).map_err(|e| format!("Failed to create RPC: {:?}", e))?;
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
    use crate::monero_rpc::HttpRpc;
    let rpc = HttpRpc::new(node_url.to_string())
        .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
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
    use crate::monero_rpc::HttpRpc;
    let rpc = HttpRpc::new(node_url.to_string())
        .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
    scan_mempool(&rpc, mnemonic, _network, lookahead, None, passphrase).await
}

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

    let (mempool_txs, mempool_spent_key_images) = rpc
        .get_transaction_pool()
        .await
        .map_err(|e| format!("Failed to fetch mempool: {:?}", e))?;

    let tx_count = mempool_txs.len();
    let mut outputs = Vec::new();
    let mut spent_key_images_map: HashMap<String, String> = mempool_spent_key_images
        .iter()
        .map(|ki| (hex::encode(ki), String::new()))
        .collect();

    #[cfg(target_arch = "wasm32")]
    let key_image_scalar: Option<&Scalar> = spend_scalar.as_deref();
    #[cfg(not(target_arch = "wasm32"))]
    let key_image_scalar: Option<&Scalar> = None;

    for tx in mempool_txs.iter() {
        let (tx_hash, parsed) = parse_full_tx_blob(&tx.serialize())
            .ok_or_else(|| format!("Failed to parse transaction {}", hex::encode(tx.hash())))?;
        let tx_hash_hex = hex::encode(tx_hash);

        for input in &parsed.prefix().inputs {
            if let OxideInput::ToKey { key_image, .. } = input {
                let ki_hex = hex::encode(key_image.to_bytes());
                spent_key_images_map.insert(ki_hex, tx_hash_hex.clone());
            }
        }

        let scanned = scan_single_transaction(&mut scanner, tx_hash, &parsed)?;
        for output in &scanned {
            outputs.push(map_oxide_output(
                output,
                0,         // unconfirmed: still in the mempool
                [0u8; 32], // mempool transactions are never coinbase
                key_image_scalar,
                false,
            )?);
        }
    }

    let (spent_key_images, spent_key_image_tx_hashes): (Vec<String>, Vec<String>) =
        spent_key_images_map.into_iter().unzip();

    let mut result = MempoolScanResult {
        tx_count,
        outputs,
        spent_key_images,
        spent_key_image_tx_hashes,
    };
    retain_mempool_outputs_for_accounts(&mut result.outputs, accounts_to_scan);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

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
