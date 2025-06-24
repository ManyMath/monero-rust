//! Unified scanning implementation for Monero wallets.
//!
//! This module provides wallet scanning functionality that works across
//! both native and WASM targets through generic RpcConnection support.

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use monero_serai::{
    block::Block,
    rpc::{GetBlocksFastResponse, Rpc, RpcConnection},
    transaction::{Input, Transaction},
    wallet::{
        address::{AddressMeta, AddressType, MoneroAddress, Network, SubaddressIndex},
        seed::{Language, Seed},
        Scanner, ViewPair,
    },
};
#[cfg(target_arch = "wasm32")]
use monero_serai::ringct::generate_key_image;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

use crate::wallet_output::WalletOutput;
use std::collections::{HashMap, HashSet};
use zeroize::Zeroizing;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinSet;

/// How often to yield to the browser event loop during batch processing.
const YIELD_EVERY_N_BLOCKS: usize = 50;

/// Yield to other tasks on wasm; do nothing on native targets.
#[inline]
async fn yield_to_event_loop() {
    #[cfg(target_arch = "wasm32")]
    {
        gloo_timers::future::TimeoutFuture::new(0).await;
    }
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
            if bits > 64 { return None; }
            if b & 0x80 == 0 { return Some(res); }
        }
    }

    let mut key_images = Vec::new();
    let mut cursor = Cursor::new(tx_blob);

    let Some(_version) = read_varint(&mut cursor) else { return key_images };
    let Some(_timelock) = read_varint(&mut cursor) else { return key_images };
    let Some(num_inputs) = read_varint(&mut cursor) else { return key_images };

    for _ in 0..num_inputs {
        let mut type_byte = [0u8; 1];
        if cursor.read_exact(&mut type_byte).is_err() { break; }

        match type_byte[0] {
            0xff => {
                // Gen input: varint height
                if read_varint(&mut cursor).is_none() { break; }
            }
            0x02 => {
                // ToKey input: amount, key_offsets, 32-byte key_image
                let Some(_amount) = read_varint(&mut cursor) else { break };
                let Some(num_offsets) = read_varint(&mut cursor) else { break };
                for _ in 0..num_offsets {
                    if read_varint(&mut cursor).is_none() { break; }
                }
                let mut ki = [0u8; 32];
                if cursor.read_exact(&mut ki).is_err() { break; }
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
    pub block_timestamp: u64,
    pub tx_count: usize,
    pub outputs: Vec<WalletOutput>,
    pub daemon_height: u64,
    pub spent_key_images: Vec<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolScanResult {
    pub tx_count: usize,
    pub outputs: Vec<WalletOutput>,
    pub spent_key_images: Vec<String>,
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
#[derive(Debug, Clone)]
pub struct WalletScanConfig {
    pub mnemonic: String,
    pub network: String,
    pub lookahead: Lookahead,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DerivedKeys {
    pub secret_spend_key: String,
    pub secret_view_key: String,
    pub public_spend_key: String,
    pub public_view_key: String,
    pub address: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lookahead {
    pub account: u32,
    pub subaddress: u32,
}

pub const DEFAULT_LOOKAHEAD: Lookahead = Lookahead {
    account: 0,
    subaddress: 20,
};

pub const WALLET_CLI_SOFTWARE_LOOKAHEAD: Lookahead = Lookahead {
    account: 50,
    subaddress: 200,
};

pub const WALLET_CLI_HARDWARE_LOOKAHEAD: Lookahead = Lookahead {
    account: 5,
    subaddress: 20,
};

fn parse_network(network_str: &str) -> Result<Network, String> {
    match network_str.to_lowercase().as_str() {
        "mainnet" => Ok(Network::Mainnet),
        "testnet" => Ok(Network::Testnet),
        "stagenet" => Ok(Network::Stagenet),
        _ => Err(format!("Invalid network: {}", network_str)),
    }
}

fn spend_key_from_seed(seed: &Seed) -> EdwardsPoint {
    let key_bytes = seed.key_bytes();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&key_bytes[..]);

    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    &spend_scalar * &ED25519_BASEPOINT_TABLE
}

fn view_key_from_seed(seed: &Seed) -> Scalar {
    let key_bytes = seed.key_bytes();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&key_bytes[..]);

    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    let view: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    Scalar::from_bytes_mod_order(view)
}

#[cfg(target_arch = "wasm32")]
fn spend_key_scalar_from_seed(seed: &Seed) -> Scalar {
    let key_bytes = seed.key_bytes();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&key_bytes[..]);
    Scalar::from_bytes_mod_order(spend_bytes)
}

#[cfg(target_arch = "wasm32")]
fn calculate_key_image(spend_scalar: &Scalar, key_offset: &Scalar) -> EdwardsPoint {
    let one_time_key_scalar = Zeroizing::new(spend_scalar + key_offset);
    generate_key_image(&one_time_key_scalar)
}

fn build_subaddress_indices(lookahead: Lookahead) -> Vec<SubaddressIndex> {
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

/// Resolve a mnemonic with explicit BIP39 passphrase and account index.
/// If 12 words, convert from BIP39 first using the given passphrase and account index.
/// Non-BIP39 seeds (16-word polyseed, 25-word classic) pass through unchanged.
pub fn resolve_seed_bip39(mnemonic: &str, passphrase: &str, account_index: u32) -> Result<Seed, String> {
    let word_count = mnemonic.split_whitespace().count();
    if word_count == 12 {
        let legacy = crate::bip39_conv::bip39_to_legacy_mnemonic(mnemonic, passphrase, account_index)?;
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

pub fn generate_seed(seed_type: &str) -> Result<String, String> {
    // Use thread_rng which works in both native and WASM contexts
    let mut rng = rand::thread_rng();

    let seed = match seed_type {
        "polyseed" => Seed::new_polyseed(&mut rng),
        "bip39" => return crate::bip39_conv::generate_bip39(),
        _ => Seed::new(&mut rng, Language::English),
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

fn address_from_seed(seed: &Seed, network: Network) -> String {
    let spend_point = spend_key_from_seed(seed);
    let view_scalar = view_key_from_seed(seed);
    let view_point: EdwardsPoint = &view_scalar * &ED25519_BASEPOINT_TABLE;

    MoneroAddress::new(
        AddressMeta::new(network, AddressType::Standard),
        spend_point,
        view_point,
    )
    .to_string()
}

pub fn derive_address(mnemonic: &str, network_str: &str) -> Result<String, String> {
    let network = parse_network(network_str)?;
    let seed = resolve_seed(mnemonic)?;
    Ok(address_from_seed(&seed, network))
}

pub fn derive_subaddress(
    mnemonic: &str,
    network_str: &str,
    account: u32,
    address_index: u32,
) -> Result<String, String> {
    use monero_serai::wallet::address::AddressSpec;

    let network = parse_network(network_str)?;

    let seed = resolve_seed(mnemonic)?;

    let spend: [u8; 32] = *seed.key_bytes();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * &ED25519_BASEPOINT_TABLE;

    let view: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    let view_scalar = Scalar::from_bytes_mod_order(view);

    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));

    if account == 0 && address_index == 0 {
        let address = view_pair.address(network, AddressSpec::Standard);
        return Ok(address.to_string());
    }

    let subaddress_index = SubaddressIndex::new(account, address_index)
        .ok_or_else(|| {
            format!(
                "Invalid subaddress index: ({}, {}). Note: (0, 0) should use derive_address() instead.",
                account, address_index
            )
        })?;

    let address = view_pair.address(network, AddressSpec::Subaddress(subaddress_index));
    Ok(address.to_string())
}

pub fn derive_keys(mnemonic: &str, network_str: &str) -> Result<DerivedKeys, String> {
    let network = parse_network(network_str)?;

    let seed = resolve_seed(mnemonic)?;

    let spend: [u8; 32] = *seed.key_bytes();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * &ED25519_BASEPOINT_TABLE;

    let view: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    let view_scalar = Scalar::from_bytes_mod_order(view);
    let view_point: EdwardsPoint = &view_scalar * &ED25519_BASEPOINT_TABLE;

    let address = MoneroAddress::new(
        AddressMeta::new(network, AddressType::Standard),
        spend_point,
        view_point,
    );

    Ok(DerivedKeys {
        secret_spend_key: hex::encode(spend_scalar.to_bytes()),
        secret_view_key: hex::encode(view_scalar.to_bytes()),
        public_spend_key: hex::encode(spend_point.compress().to_bytes()),
        public_view_key: hex::encode(view_point.compress().to_bytes()),
        address: address.to_string(),
    })
}

pub async fn get_daemon_height(node_url: &str) -> Result<u64, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use monero_serai::rpc::HttpRpc;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?;
        let height = rpc.get_height()
            .await
            .map_err(|e| format!("Failed to get height: {:?}", e))?;
        Ok(height as u64)
    }

    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc_serai::WasmRpcConnection;
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
        let height = rpc.get_height()
            .await
            .map_err(|e| format!("Failed to get height: {:?}", e))?;
        Ok(height as u64)
    }
}

/// Scan a single block for outputs belonging to the given wallet.
///
/// This function is generic over the RPC connection type, allowing it to work
/// with native or wasm RPC connections.
pub async fn scan_block_for_outputs_with_url(
    node_url: &str,
    block_height: u64,
    mnemonic: &str,
    network_str: &str,
) -> Result<BlockScanResult, String> {
    scan_block_for_outputs_with_url_and_lookahead(node_url, block_height, mnemonic, network_str, DEFAULT_LOOKAHEAD).await
}

pub async fn scan_block_for_outputs_with_url_and_lookahead(
    node_url: &str,
    block_height: u64,
    mnemonic: &str,
    network_str: &str,
    lookahead: Lookahead,
) -> Result<BlockScanResult, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use monero_serai::rpc::HttpRpc;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC: {:?}", e))?;
        scan_block_for_outputs_with_lookahead(
            &rpc,
            block_height,
            mnemonic,
            network_str,
            lookahead,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc_serai::WasmRpcConnection;
        use monero_serai::rpc::Rpc;
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
        scan_block_for_outputs_with_lookahead(
            &rpc,
            block_height,
            mnemonic,
            network_str,
            lookahead,
        )
        .await
    }
}

pub async fn scan_block_for_outputs<R: RpcConnection>(
    rpc: &Rpc<R>,
    block_height: u64,
    mnemonic: &str,
    network_str: &str,
) -> Result<BlockScanResult, String> {
    scan_block_for_outputs_with_lookahead(
        rpc,
        block_height,
        mnemonic,
        network_str,
        DEFAULT_LOOKAHEAD,
    )
    .await
}

pub async fn scan_block_for_outputs_with_lookahead<R: RpcConnection>(
    rpc: &Rpc<R>,
    block_height: u64,
    mnemonic: &str,
    network_str: &str,
    lookahead: Lookahead,
) -> Result<BlockScanResult, String> {
    let _network = parse_network(network_str)?;

    let seed = resolve_seed(mnemonic)?;

    let spend_point = spend_key_from_seed(&seed);
    let view_scalar = view_key_from_seed(&seed);
    #[cfg(target_arch = "wasm32")]
    let spend_scalar = spend_key_scalar_from_seed(&seed);

    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
    let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
    register_subaddresses(&mut scanner, lookahead);

    let block_hash_bytes = rpc
        .get_block_hash(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block hash: {:?}", e))?;
    let block_hash = hex::encode(block_hash_bytes);

    let daemon_height = rpc
        .get_height()
        .await
        .map_err(|e| format!("Failed to fetch daemon height: {:?}", e))? as u64;

    let block = rpc
        .get_block_by_number(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block: {:?}", e))?;

    let block_timestamp = block.header.timestamp;
    let tx_hashes = block.txs.clone();
    let mut all_transactions = vec![block.miner_tx];

    if !tx_hashes.is_empty() {
        let fetched_txs = rpc
            .get_transactions(&tx_hashes)
            .await
            .map_err(|e| format!("Failed to fetch transactions: {:?}", e))?;
        all_transactions.extend(fetched_txs);
    }

    let tx_count = all_transactions.len();
    let mut outputs = Vec::new();
    let mut spent_key_images = Vec::new();

    for tx in all_transactions.iter() {
        let tx_hash = hex::encode(tx.hash());
        let is_coinbase = matches!(tx.prefix.inputs.get(0), Some(Input::Gen(_)));

        // Extract spent key images from transaction inputs
        for input in &tx.prefix.inputs {
            if let Input::ToKey { key_image, .. } = input {
                let ki_hex = hex::encode(key_image.compress().to_bytes());
                spent_key_images.push(ki_hex);
            }
        }

        let scan_result = scanner.scan_transaction(tx);
        let owned_outputs = scan_result.ignore_timelock();

        for output in owned_outputs {
            let amount = output.data.commitment.amount;
            let amount_xmr = format!("{:.12}", amount as f64 / 1_000_000_000_000.0);
            let output_index = output.absolute.o;
            let key = hex::encode(output.data.key.compress().to_bytes());
            let key_offset = hex::encode(output.data.key_offset.to_bytes());
            let commitment_mask = hex::encode(output.data.commitment.mask.to_bytes());
            let subaddress_index = output
                .metadata
                .subaddress
                .map(|idx| (idx.account(), idx.address()));
            let payment_id = if output.metadata.payment_id != [0u8; 8] {
                Some(hex::encode(output.metadata.payment_id))
            } else {
                None
            };
            let received_output_bytes = hex::encode(output.serialize());

            // Calculate key image (WASM only due to spend scalar requirement)
            #[cfg(target_arch = "wasm32")]
            let key_image = {
                let key_image_point = calculate_key_image(&spend_scalar, &output.data.key_offset);
                hex::encode(key_image_point.compress().to_bytes())
            };
            #[cfg(not(target_arch = "wasm32"))]
            let key_image = String::new();

            outputs.push(WalletOutput {
                tx_hash: tx_hash.clone(),
                output_index,
                amount,
                amount_xmr,
                key,
                key_offset,
                commitment_mask,
                subaddress_index,
                payment_id,
                received_output_bytes,
                block_height,
                spent: false,
                spent_height: None,
                key_image,
                is_coinbase,
                frozen: false,
            });
        }
    }

    Ok(BlockScanResult {
        block_height,
        block_hash,
        block_timestamp,
        tx_count,
        outputs,
        daemon_height,
        spent_key_images,
    })
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
    network_str: &str,
    lookahead: Lookahead,
) -> Result<Vec<BlockScanResult>, String> {
    // Get a known block hash so the daemon can find the fork point.
    let known_hash = rpc
        .get_block_hash(start_height as usize)
        .await
        .map_err(|e| format!("Failed to get block hash at {}: {:?}", start_height, e))?;

    // Fetch batch of blocks via binary RPC (up to ~1000 blocks per call)
    let response = rpc
        .get_blocks_fast(&[known_hash], start_height)
        .await
        .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;

    process_batch_response(response, mnemonic, network_str, lookahead).await
}

/// Process a batch of blocks fetched via `/getblocks.bin` and scan them for outputs.
///
/// This is the core scanning logic, separated from the RPC layer for testability.
pub async fn process_batch_response(
    response: GetBlocksFastResponse,
    mnemonic: &str,
    network_str: &str,
    lookahead: Lookahead,
) -> Result<Vec<BlockScanResult>, String> {
    let _network = parse_network(network_str)?;

    let seed = resolve_seed(mnemonic)?;
    let spend_point = spend_key_from_seed(&seed);
    let view_scalar = view_key_from_seed(&seed);
    #[cfg(target_arch = "wasm32")]
    let spend_scalar = spend_key_scalar_from_seed(&seed);
    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
    let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
    register_subaddresses(&mut scanner, lookahead);

    let daemon_height = response.current_height;
    let mut results = Vec::with_capacity(response.blocks.len());

    for (block_idx, block_entry) in response.blocks.iter().enumerate() {
        let block_height = response.start_height + block_idx as u64;

        // Pre-RingCT blocks can't be parsed by monero-serai
        let major_version = block_entry.block.first().copied().unwrap_or(0);
        if major_version < 4 {
            let block_hash = hex::encode(Keccak256::digest(&block_entry.block));
            results.push(BlockScanResult {
                block_height,
                block_hash,
                block_timestamp: 0,
                daemon_height,
                outputs: vec![],
                spent_key_images: vec![],
                tx_count: 0,
            });
            continue;
        }

        let block = Block::read::<&[u8]>(&mut block_entry.block.as_ref())
            .map_err(|e| format!("Failed to parse block at height {}: {:?}", block_height, e))?;

        if block.number() != block_height as usize {
            return Err(format!(
                "Block height mismatch: expected {}, got {}",
                block_height,
                block.number()
            ));
        }

        let block_timestamp = block.header.timestamp;
        let block_hash = hex::encode(Keccak256::digest(&block_entry.block));

        let miner_tx = block.miner_tx;
        let mut parsed_txs = Vec::with_capacity(block_entry.txs.len());
        let mut skipped_key_images = Vec::new();
        for tx_blob in &block_entry.txs {
            match Transaction::read::<&[u8]>(&mut tx_blob.as_ref()) {
                Ok(tx) => parsed_txs.push(tx),
                Err(_) => {
                    skipped_key_images.extend(extract_key_images_from_raw_tx(tx_blob));
                }
            }
        }

        let all_transactions: Vec<&Transaction> = std::iter::once(&miner_tx)
            .chain(parsed_txs.iter())
            .collect();

        let tx_count = 1 + block_entry.txs.len();
        let mut outputs = Vec::new();
        let mut spent_key_images = skipped_key_images;

        for tx in &all_transactions {
            let tx_hash = hex::encode(tx.hash());
            let is_coinbase = matches!(tx.prefix.inputs.get(0), Some(Input::Gen(_)));

            for input in &tx.prefix.inputs {
                if let Input::ToKey { key_image, .. } = input {
                    let ki_hex = hex::encode(key_image.compress().to_bytes());
                    spent_key_images.push(ki_hex);
                }
            }

            let scan_result = scanner.scan_transaction(tx);
            let owned_outputs = scan_result.ignore_timelock();

            for output in owned_outputs {
                let amount = output.data.commitment.amount;
                let amount_xmr = format!("{:.12}", amount as f64 / 1_000_000_000_000.0);
                let output_index = output.absolute.o;
                let key = hex::encode(output.data.key.compress().to_bytes());
                let key_offset = hex::encode(output.data.key_offset.to_bytes());
                let commitment_mask = hex::encode(output.data.commitment.mask.to_bytes());
                let subaddress_index = output
                    .metadata
                    .subaddress
                    .map(|idx| (idx.account(), idx.address()));
                let payment_id = if output.metadata.payment_id != [0u8; 8] {
                    Some(hex::encode(output.metadata.payment_id))
                } else {
                    None
                };
                let received_output_bytes = hex::encode(output.serialize());

                #[cfg(target_arch = "wasm32")]
                let key_image = {
                    let key_image_point =
                        calculate_key_image(&spend_scalar, &output.data.key_offset);
                    hex::encode(key_image_point.compress().to_bytes())
                };
                #[cfg(not(target_arch = "wasm32"))]
                let key_image = String::new();

                outputs.push(WalletOutput {
                    tx_hash: tx_hash.clone(),
                    output_index,
                    amount,
                    amount_xmr,
                    key,
                    key_offset,
                    commitment_mask,
                    subaddress_index,
                    payment_id,
                    received_output_bytes,
                    block_height,
                    spent: false,
                    spent_height: None,
                    key_image,
                    is_coinbase,
                    frozen: false,
                });
            }
        }

        results.push(BlockScanResult {
            block_height,
            block_hash,
            block_timestamp,
            tx_count,
            outputs,
            daemon_height,
            spent_key_images,
        });

        if block_idx % YIELD_EVERY_N_BLOCKS == YIELD_EVERY_N_BLOCKS - 1 {
            yield_to_event_loop().await;
        }
    }

    Ok(results)
}

/// Convenience wrapper for batch scanning with URL-based RPC creation.
pub async fn scan_blocks_batch_with_url(
    node_url: &str,
    start_height: u64,
    mnemonic: &str,
    network_str: &str,
    lookahead: Lookahead,
) -> Result<Vec<BlockScanResult>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use monero_serai::rpc::HttpRpc;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC: {:?}", e))?;
        scan_blocks_batch(&rpc, start_height, mnemonic, network_str, lookahead).await
    }

    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc_serai::WasmRpcConnection;
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
        scan_blocks_batch(&rpc, start_height, mnemonic, network_str, lookahead).await
    }
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
) -> Result<Vec<MultiWalletScanResult>, String> {
    if wallet_configs.is_empty() {
        return Err("No wallet configurations provided".to_string());
    }

    // Get a known block hash for the daemon to find the fork point
    let known_hash = rpc
        .get_block_hash(start_height as usize)
        .await
        .map_err(|e| format!("Failed to get block hash at {}: {:?}", start_height, e))?;

    // Fetch batch of blocks via binary RPC
    let response = rpc
        .get_blocks_fast(&[known_hash], start_height)
        .await
        .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;

    process_batch_multi_wallet_response(response, wallet_configs).await
}

/// Process a batch of blocks fetched via `/getblocks.bin` and scan for multiple wallets.
///
/// This is the core multi-wallet scanning logic, separated from the RPC layer for testability.
pub async fn process_batch_multi_wallet_response(
    response: GetBlocksFastResponse,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<Vec<MultiWalletScanResult>, String> {
    if wallet_configs.is_empty() {
        return Err("No wallet configurations provided".to_string());
    }

    let daemon_height = response.current_height;

    struct WalletScanner {
        address: String,
        scanner: Scanner,
        #[cfg(target_arch = "wasm32")]
        spend_scalar: Scalar,
    }

    let mut wallet_scanners = Vec::with_capacity(wallet_configs.len());
    for config in &wallet_configs {
        let network = parse_network(&config.network)?;
        let seed = resolve_seed(&config.mnemonic)?;
        let address = address_from_seed(&seed, network);
        let spend_point = spend_key_from_seed(&seed);
        let view_scalar = view_key_from_seed(&seed);
        #[cfg(target_arch = "wasm32")]
        let spend_scalar = spend_key_scalar_from_seed(&seed);
        let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
        let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
        register_subaddresses(&mut scanner, config.lookahead);

        wallet_scanners.push(WalletScanner {
            address,
            scanner,
            #[cfg(target_arch = "wasm32")]
            spend_scalar,
        });
    }

    let mut results = Vec::with_capacity(response.blocks.len());

    for (block_idx, block_entry) in response.blocks.iter().enumerate() {
        let block_height = response.start_height + block_idx as u64;

        // Pre-RingCT blocks can't be parsed by monero-serai
        let major_version = block_entry.block.first().copied().unwrap_or(0);
        if major_version < 4 {
            let block_hash = hex::encode(Keccak256::digest(&block_entry.block));
            let mut wallet_results = HashMap::new();
            for ws in &wallet_scanners {
                wallet_results.insert(
                    ws.address.clone(),
                    WalletScanData {
                        address: ws.address.clone(),
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
                wallet_results,
            });
            continue;
        }

        let block = Block::read::<&[u8]>(&mut block_entry.block.as_ref())
            .map_err(|e| format!("Failed to parse block at height {}: {:?}", block_height, e))?;

        if block.number() != block_height as usize {
            return Err(format!(
                "Block height mismatch: expected {}, got {}",
                block_height,
                block.number()
            ));
        }

        let block_timestamp = block.header.timestamp;
        let block_hash = hex::encode(Keccak256::digest(&block_entry.block));

        let miner_tx = block.miner_tx;
        let mut parsed_txs = Vec::with_capacity(block_entry.txs.len());
        let mut skipped_key_images = Vec::new();
        for tx_blob in &block_entry.txs {
            match Transaction::read::<&[u8]>(&mut tx_blob.as_ref()) {
                Ok(tx) => parsed_txs.push(tx),
                Err(_) => {
                    skipped_key_images.extend(extract_key_images_from_raw_tx(tx_blob));
                }
            }
        }

        let all_transactions: Vec<&Transaction> = std::iter::once(&miner_tx)
            .chain(parsed_txs.iter())
            .collect();
        let tx_count = 1 + block_entry.txs.len();

        let mut spent_key_images = skipped_key_images;
        for tx in &all_transactions {
            for input in &tx.prefix.inputs {
                if let Input::ToKey { key_image, .. } = input {
                    spent_key_images.push(hex::encode(key_image.compress().to_bytes()));
                }
            }
        }

        let mut wallet_results = HashMap::new();
        for ws in &mut wallet_scanners {
            let mut outputs = Vec::new();
            for tx in &all_transactions {
                let tx_hash = hex::encode(tx.hash());
                let is_coinbase = matches!(tx.prefix.inputs.get(0), Some(Input::Gen(_)));

                let scan_result = ws.scanner.scan_transaction(tx);
                let owned_outputs = scan_result.ignore_timelock();

                for output in owned_outputs {
                    let amount = output.data.commitment.amount;
                    let amount_xmr = format!("{:.12}", amount as f64 / 1_000_000_000_000.0);
                    let output_index = output.absolute.o;
                    let key = hex::encode(output.data.key.compress().to_bytes());
                    let key_offset = hex::encode(output.data.key_offset.to_bytes());
                    let commitment_mask = hex::encode(output.data.commitment.mask.to_bytes());
                    let subaddress_index = output
                        .metadata
                        .subaddress
                        .map(|idx| (idx.account(), idx.address()));
                    let payment_id = if output.metadata.payment_id != [0u8; 8] {
                        Some(hex::encode(output.metadata.payment_id))
                    } else {
                        None
                    };
                    let received_output_bytes = hex::encode(output.serialize());

                    #[cfg(target_arch = "wasm32")]
                    let key_image = {
                        let key_image_point =
                            calculate_key_image(&ws.spend_scalar, &output.data.key_offset);
                        hex::encode(key_image_point.compress().to_bytes())
                    };
                    #[cfg(not(target_arch = "wasm32"))]
                    let key_image = String::new();

                    outputs.push(WalletOutput {
                        tx_hash: tx_hash.clone(),
                        output_index,
                        amount,
                        amount_xmr,
                        key,
                        key_offset,
                        commitment_mask,
                        subaddress_index,
                        payment_id,
                        received_output_bytes,
                        block_height,
                        spent: false,
                        spent_height: None,
                        key_image,
                        is_coinbase,
                        frozen: false,
                    });
                }
            }
            wallet_results.insert(
                ws.address.clone(),
                WalletScanData {
                    address: ws.address.clone(),
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
            wallet_results,
        });

        if block_idx % YIELD_EVERY_N_BLOCKS == YIELD_EVERY_N_BLOCKS - 1 {
            yield_to_event_loop().await;
        }
    }

    Ok(results)
}

/// Convenience wrapper for multi-wallet batch scanning with URL-based RPC creation.
pub async fn scan_blocks_batch_multi_wallet_with_url(
    node_url: &str,
    start_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<Vec<MultiWalletScanResult>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use monero_serai::rpc::HttpRpc;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC: {:?}", e))?;
        scan_blocks_batch_multi_wallet(&rpc, start_height, wallet_configs).await
    }

    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc_serai::WasmRpcConnection;
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
        scan_blocks_batch_multi_wallet(&rpc, start_height, wallet_configs).await
    }
}

/// Opaque wrapper around `GetBlocksFastResponse` for double-buffered pipelining.
///
/// The hub crate depends on `monero-rust` but not `monero-serai`, so it cannot
/// use `GetBlocksFastResponse` directly.
pub struct FetchedBlocks {
    response: GetBlocksFastResponse,
}

impl FetchedBlocks {
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
pub async fn fetch_blocks_batch_with_url(
    node_url: &str,
    start_height: u64,
) -> Result<FetchedBlocks, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use monero_serai::rpc::HttpRpc;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC: {:?}", e))?;
        let known_hash = rpc
            .get_block_hash(start_height as usize)
            .await
            .map_err(|e| format!("Failed to get block hash at {}: {:?}", start_height, e))?;
        let response = rpc
            .get_blocks_fast(&[known_hash], start_height)
            .await
            .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;
        Ok(FetchedBlocks { response })
    }

    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc_serai::WasmRpcConnection;
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
        let known_hash = rpc
            .get_block_hash(start_height as usize)
            .await
            .map_err(|e| format!("Failed to get block hash at {}: {:?}", start_height, e))?;
        let response = rpc
            .get_blocks_fast(&[known_hash], start_height)
            .await
            .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;
        Ok(FetchedBlocks { response })
    }
}

/// Process a previously fetched batch for a single wallet.
pub async fn process_fetched_batch(
    fetched: FetchedBlocks,
    mnemonic: &str,
    network_str: &str,
    lookahead: Lookahead,
) -> Result<Vec<BlockScanResult>, String> {
    process_batch_response(fetched.response, mnemonic, network_str, lookahead).await
}

fn hex_to_hash(hex_str: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_str)
        .map_err(|e| format!("Invalid hex hash '{}': {}", hex_str, e))?;
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
/// Returns `(results, actual_start_height)` — the daemon may return blocks
/// starting earlier than `start_height` if a fork was detected.
pub async fn scan_blocks_batch_with_history<R: RpcConnection>(
    rpc: &Rpc<R>,
    start_height: u64,
    known_hashes: &[(u64, String)],
    mnemonic: &str,
    network_str: &str,
    lookahead: Lookahead,
) -> Result<(Vec<BlockScanResult>, u64), String> {
    let block_ids: Vec<[u8; 32]> = if known_hashes.is_empty() {
        // Fallback: single hash like the old behavior
        let hash = rpc
            .get_block_hash(start_height as usize)
            .await
            .map_err(|e| format!("Failed to get block hash at {}: {:?}", start_height, e))?;
        vec![hash]
    } else {
        known_hashes
            .iter()
            .map(|(_, h)| hex_to_hash(h))
            .collect::<Result<Vec<_>, _>>()?
    };

    let response = rpc
        .get_blocks_fast(&block_ids, start_height)
        .await
        .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;

    let actual_start = response.start_height;
    let results = process_batch_response(response, mnemonic, network_str, lookahead).await?;
    Ok((results, actual_start))
}

/// Convenience wrapper for history-aware batch scanning with URL-based RPC.
pub async fn scan_blocks_batch_with_history_url(
    node_url: &str,
    start_height: u64,
    known_hashes: &[(u64, String)],
    mnemonic: &str,
    network_str: &str,
    lookahead: Lookahead,
) -> Result<(Vec<BlockScanResult>, u64), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use monero_serai::rpc::HttpRpc;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC: {:?}", e))?;
        scan_blocks_batch_with_history(&rpc, start_height, known_hashes, mnemonic, network_str, lookahead).await
    }

    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc_serai::WasmRpcConnection;
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
        scan_blocks_batch_with_history(&rpc, start_height, known_hashes, mnemonic, network_str, lookahead).await
    }
}

/// Fetch a batch of blocks using known hash history, without scanning.
///
/// This is the fetch-only half for double-buffered pipelining with reorg awareness.
pub async fn fetch_blocks_batch_with_history_url(
    node_url: &str,
    start_height: u64,
    known_hashes: &[(u64, String)],
) -> Result<(FetchedBlocks, u64), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use monero_serai::rpc::HttpRpc;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC: {:?}", e))?;
        let block_ids: Vec<[u8; 32]> = if known_hashes.is_empty() {
            let hash = rpc
                .get_block_hash(start_height as usize)
                .await
                .map_err(|e| format!("Failed to get block hash at {}: {:?}", start_height, e))?;
            vec![hash]
        } else {
            known_hashes
                .iter()
                .map(|(_, h)| hex_to_hash(h))
                .collect::<Result<Vec<_>, _>>()?
        };
        let response = rpc
            .get_blocks_fast(&block_ids, start_height)
            .await
            .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;
        let actual_start = response.start_height;
        Ok((FetchedBlocks { response }, actual_start))
    }

    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc_serai::WasmRpcConnection;
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
        let block_ids: Vec<[u8; 32]> = if known_hashes.is_empty() {
            let hash = rpc
                .get_block_hash(start_height as usize)
                .await
                .map_err(|e| format!("Failed to get block hash at {}: {:?}", start_height, e))?;
            vec![hash]
        } else {
            known_hashes
                .iter()
                .map(|(_, h)| hex_to_hash(h))
                .collect::<Result<Vec<_>, _>>()?
        };
        let response = rpc
            .get_blocks_fast(&block_ids, start_height)
            .await
            .map_err(|e| format!("Failed to fetch blocks batch: {:?}", e))?;
        let actual_start = response.start_height;
        Ok((FetchedBlocks { response }, actual_start))
    }
}

/// Process a previously fetched batch for multiple wallets.
pub async fn process_fetched_batch_multi_wallet(
    fetched: FetchedBlocks,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<Vec<MultiWalletScanResult>, String> {
    process_batch_multi_wallet_response(fetched.response, wallet_configs).await
}

/// Scan a single block for outputs belonging to multiple wallets simultaneously.
/// This is more efficient than scanning each wallet separately as it fetches
/// block data only once and processes all wallets in parallel.
///
/// # Arguments
/// * `rpc` - RPC connection to use for fetching block data
/// * `block_height` - Height of the block to scan
/// * `wallet_configs` - Vector of wallet configurations to scan for
///
/// # Returns
/// A `MultiWalletScanResult` containing outputs for each wallet
#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_block_multi_wallet<R: RpcConnection + Send + Sync + Clone + 'static>(
    rpc: &Rpc<R>,
    block_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<MultiWalletScanResult, String> {
    if wallet_configs.is_empty() {
        return Err("No wallet configurations provided".to_string());
    }

    // Step 1: Fetch block data once (shared across all wallets)
    let block_hash_bytes = rpc
        .get_block_hash(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block hash: {:?}", e))?;
    let block_hash = hex::encode(block_hash_bytes);

    let daemon_height = rpc
        .get_height()
        .await
        .map_err(|e| format!("Failed to fetch daemon height: {:?}", e))? as u64;

    let block = rpc
        .get_block_by_number(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block: {:?}", e))?;

    let block_timestamp = block.header.timestamp;
    let tx_hashes = block.txs.clone();
    let mut all_transactions = vec![block.miner_tx.clone()];

    if !tx_hashes.is_empty() {
        let fetched_txs = rpc
            .get_transactions(&tx_hashes)
            .await
            .map_err(|e| format!("Failed to fetch transactions: {:?}", e))?;
        all_transactions.extend(fetched_txs);
    }

    let tx_count = all_transactions.len();

    // Extract spent key images (shared across all wallets)
    let mut spent_key_images = Vec::new();
    for tx in all_transactions.iter() {
        for input in &tx.prefix.inputs {
            if let Input::ToKey { key_image, .. } = input {
                let ki_hex = hex::encode(key_image.compress().to_bytes());
                spent_key_images.push(ki_hex);
            }
        }
    }

    // Step 2: Spawn parallel scanning tasks for each wallet
    let mut join_set = JoinSet::new();
    let txs = Arc::new(all_transactions);

    for wallet_config in wallet_configs {
        let txs = Arc::clone(&txs);

        join_set.spawn(async move {
            // Parse seed once — used for both address derivation and scanner setup
            let network = parse_network(&wallet_config.network)?;
            let seed = resolve_seed(&wallet_config.mnemonic)?;
            let address = address_from_seed(&seed, network);

            let spend_point = spend_key_from_seed(&seed);
            let view_scalar = view_key_from_seed(&seed);
            #[cfg(target_arch = "wasm32")]
            let spend_scalar = spend_key_scalar_from_seed(&seed);

            let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
            let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
            register_subaddresses(&mut scanner, wallet_config.lookahead);

            // Scan all transactions for this wallet
            let mut outputs = Vec::new();

            for tx in txs.iter() {
                let tx_hash = hex::encode(tx.hash());
                let is_coinbase = matches!(tx.prefix.inputs.get(0), Some(Input::Gen(_)));

                let scan_result = scanner.scan_transaction(tx);
                let owned_outputs = scan_result.ignore_timelock();

                for output in owned_outputs {
                    let amount = output.data.commitment.amount;
                    let amount_xmr = format!("{:.12}", amount as f64 / 1_000_000_000_000.0);
                    let output_index = output.absolute.o;
                    let key = hex::encode(output.data.key.compress().to_bytes());
                    let key_offset = hex::encode(output.data.key_offset.to_bytes());
                    let commitment_mask = hex::encode(output.data.commitment.mask.to_bytes());
                    let subaddress_index = output
                        .metadata
                        .subaddress
                        .map(|idx| (idx.account(), idx.address()));
                    let payment_id = if output.metadata.payment_id != [0u8; 8] {
                        Some(hex::encode(output.metadata.payment_id))
                    } else {
                        None
                    };
                    let received_output_bytes = hex::encode(output.serialize());

                    // Calculate key image (WASM only due to spend scalar requirement)
                    #[cfg(target_arch = "wasm32")]
                    let key_image = {
                        let key_image_point = calculate_key_image(&spend_scalar, &output.data.key_offset);
                        hex::encode(key_image_point.compress().to_bytes())
                    };
                    #[cfg(not(target_arch = "wasm32"))]
                    let key_image = String::new();

                    outputs.push(WalletOutput {
                        tx_hash: tx_hash.clone(),
                        output_index,
                        amount,
                        amount_xmr,
                        key,
                        key_offset,
                        commitment_mask,
                        subaddress_index,
                        payment_id,
                        received_output_bytes,
                        block_height,
                        spent: false,
                        spent_height: None,
                        key_image,
                        is_coinbase,
                        frozen: false,
                    });
                }
            }

            Ok::<(String, WalletScanData), String>((
                address.clone(),
                WalletScanData { address, outputs }
            ))
        });
    }

    // Step 3: Collect results from all wallet scanning tasks
    let mut wallet_results = HashMap::new();

    while let Some(result) = join_set.join_next().await {
        let join_result = result.map_err(|e| format!("Task join error: {:?}", e))?;
        let (address, wallet_data) = join_result?;
        wallet_results.insert(address, wallet_data);
    }

    Ok(MultiWalletScanResult {
        block_height,
        block_hash,
        block_timestamp,
        tx_count,
        daemon_height,
        spent_key_images,
        wallet_results,
    })
}

/// Convenience wrapper for multi-wallet scanning with URL-based RPC creation
#[cfg(not(target_arch = "wasm32"))]
pub async fn scan_block_multi_wallet_with_url(
    node_url: &str,
    block_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<MultiWalletScanResult, String> {
    use monero_serai::rpc::HttpRpc;
    let rpc = HttpRpc::new(node_url.to_string())
        .map_err(|e| format!("Failed to create RPC: {:?}", e))?;
    scan_block_multi_wallet(&rpc, block_height, wallet_configs).await
}

/// WASM-compatible multi-wallet scanning (sequential, not parallel)
#[cfg(target_arch = "wasm32")]
pub async fn scan_block_multi_wallet_wasm<R: RpcConnection>(
    rpc: &Rpc<R>,
    block_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<MultiWalletScanResult, String> {
    if wallet_configs.is_empty() {
        return Err("No wallet configurations provided".to_string());
    }

    // Step 1: Fetch block data once (shared across all wallets)
    let block_hash_bytes = rpc
        .get_block_hash(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block hash: {:?}", e))?;
    let block_hash = hex::encode(block_hash_bytes);

    let daemon_height = rpc
        .get_height()
        .await
        .map_err(|e| format!("Failed to fetch daemon height: {:?}", e))? as u64;

    let block = rpc
        .get_block_by_number(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block: {:?}", e))?;

    let block_timestamp = block.header.timestamp;
    let tx_hashes = block.txs.clone();
    let mut all_transactions = vec![block.miner_tx.clone()];

    if !tx_hashes.is_empty() {
        let fetched_txs = rpc
            .get_transactions(&tx_hashes)
            .await
            .map_err(|e| format!("Failed to fetch transactions: {:?}", e))?;
        all_transactions.extend(fetched_txs);
    }

    let tx_count = all_transactions.len();

    // Extract spent key images (shared across all wallets)
    let mut spent_key_images = Vec::new();
    for tx in all_transactions.iter() {
        for input in &tx.prefix.inputs {
            if let monero_serai::transaction::Input::ToKey { key_image, .. } = input {
                let ki_hex = hex::encode(key_image.compress().to_bytes());
                spent_key_images.push(ki_hex);
            }
        }
    }

    // Step 2: Scan sequentially for each wallet (WASM is single-threaded)
    let mut wallet_results = HashMap::new();

    for wallet_config in wallet_configs {
        // Parse seed once — used for both address derivation and scanner setup
        let network = parse_network(&wallet_config.network)?;
        let seed = resolve_seed(&wallet_config.mnemonic)?;
        let address = address_from_seed(&seed, network);

        let spend_point = spend_key_from_seed(&seed);
        let view_scalar = view_key_from_seed(&seed);
        #[cfg(target_arch = "wasm32")]
        let spend_scalar = spend_key_scalar_from_seed(&seed);

        let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
        let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
        register_subaddresses(&mut scanner, wallet_config.lookahead);

        // Scan all transactions for this wallet
        let mut outputs = Vec::new();

        for tx in all_transactions.iter() {
            let tx_hash = hex::encode(tx.hash());
            let is_coinbase = matches!(tx.prefix.inputs.get(0), Some(Input::Gen(_)));

            let scan_result = scanner.scan_transaction(tx);
            let owned_outputs = scan_result.ignore_timelock();

            for output in owned_outputs {
                let amount = output.data.commitment.amount;
                let amount_xmr = format!("{:.12}", amount as f64 / 1_000_000_000_000.0);
                let output_index = output.absolute.o;
                let key = hex::encode(output.data.key.compress().to_bytes());
                let key_offset = hex::encode(output.data.key_offset.to_bytes());
                let commitment_mask = hex::encode(output.data.commitment.mask.to_bytes());
                let subaddress_index = output
                    .metadata
                    .subaddress
                    .map(|idx| (idx.account(), idx.address()));
                let payment_id = if output.metadata.payment_id != [0u8; 8] {
                    Some(hex::encode(output.metadata.payment_id))
                } else {
                    None
                };
                let received_output_bytes = hex::encode(output.serialize());

                // Calculate key image (WASM only due to spend scalar requirement)
                #[cfg(target_arch = "wasm32")]
                let key_image = {
                    let key_image_point = calculate_key_image(&spend_scalar, &output.data.key_offset);
                    hex::encode(key_image_point.compress().to_bytes())
                };
                #[cfg(not(target_arch = "wasm32"))]
                let key_image = String::new();

                outputs.push(WalletOutput {
                    tx_hash: tx_hash.clone(),
                    output_index,
                    amount,
                    amount_xmr,
                    key,
                    key_offset,
                    commitment_mask,
                    subaddress_index,
                    payment_id,
                    received_output_bytes,
                    block_height,
                    spent: false,
                    spent_height: None,
                    key_image,
                    is_coinbase,
                    frozen: false,
                });
            }
        }

        wallet_results.insert(
            address.clone(),
            WalletScanData { address, outputs }
        );
    }

    Ok(MultiWalletScanResult {
        block_height,
        block_hash,
        block_timestamp,
        tx_count,
        daemon_height,
        spent_key_images,
        wallet_results,
    })
}

/// WASM convenience wrapper
#[cfg(target_arch = "wasm32")]
pub async fn scan_block_multi_wallet_with_url(
    node_url: &str,
    block_height: u64,
    wallet_configs: Vec<WalletScanConfig>,
) -> Result<MultiWalletScanResult, String> {
    use crate::rpc_serai::WasmRpcConnection;
    let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
    scan_block_multi_wallet_wasm(&rpc, block_height, wallet_configs).await
}

pub async fn scan_mempool_for_outputs(
    node_url: &str,
    mnemonic: &str,
    network_str: &str,
) -> Result<MempoolScanResult, String> {
    scan_mempool_for_outputs_with_lookahead(node_url, mnemonic, network_str, DEFAULT_LOOKAHEAD).await
}

pub async fn scan_mempool_for_outputs_with_account_lookahead(
    node_url: &str,
    mnemonic: &str,
    network_str: &str,
    account_lookahead: u32,
) -> Result<MempoolScanResult, String> {
    let lookahead = Lookahead {
        account: account_lookahead,
        subaddress: DEFAULT_LOOKAHEAD.subaddress,
    };
    scan_mempool_for_outputs_with_lookahead(node_url, mnemonic, network_str, lookahead).await
}

pub async fn scan_mempool_for_outputs_with_lookahead(
    node_url: &str,
    mnemonic: &str,
    _network_str: &str,
    lookahead: Lookahead,
) -> Result<MempoolScanResult, String> {
    let seed = resolve_seed(mnemonic)?;

    let spend_point = spend_key_from_seed(&seed);
    let view_scalar = view_key_from_seed(&seed);
    #[cfg(target_arch = "wasm32")]
    let spend_scalar = spend_key_scalar_from_seed(&seed);

    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
    let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
    register_subaddresses(&mut scanner, lookahead);

    #[cfg(not(target_arch = "wasm32"))]
    let rpc = {
        use monero_serai::rpc::HttpRpc;
        HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC client: {:?}", e))?
    };

    #[cfg(target_arch = "wasm32")]
    let rpc = {
        use crate::rpc_serai::WasmRpcConnection;
        Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()))
    };

    let (mempool_txs, mempool_spent_key_images) = rpc
        .get_transaction_pool()
        .await
        .map_err(|e| format!("Failed to fetch mempool: {:?}", e))?;

    let tx_count = mempool_txs.len();
    let mut outputs = Vec::new();
    let mut spent_key_images_set: HashSet<String> = mempool_spent_key_images
        .iter()
        .map(hex::encode)
        .collect();

    for tx in mempool_txs.iter() {
        let tx_hash = hex::encode(tx.hash());
        let is_coinbase = matches!(tx.prefix.inputs.get(0), Some(Input::Gen(_)));

        for input in &tx.prefix.inputs {
            if let Input::ToKey { key_image, .. } = input {
                let ki_hex = hex::encode(key_image.compress().to_bytes());
                spent_key_images_set.insert(ki_hex);
            }
        }

        let scan_result = scanner.scan_transaction(tx);
        let owned_outputs = scan_result.ignore_timelock();

        for output in owned_outputs {
            let amount = output.data.commitment.amount;
            let amount_xmr = format!("{:.12}", amount as f64 / 1_000_000_000_000.0);
            let output_index = output.absolute.o;
            let key = hex::encode(output.data.key.compress().to_bytes());
            let key_offset = hex::encode(output.data.key_offset.to_bytes());
            let commitment_mask = hex::encode(output.data.commitment.mask.to_bytes());
            let subaddress_index = output
                .metadata
                .subaddress
                .map(|idx| (idx.account(), idx.address()));
            let payment_id = if output.metadata.payment_id != [0u8; 8] {
                Some(hex::encode(output.metadata.payment_id))
            } else {
                None
            };
            let received_output_bytes = hex::encode(output.serialize());

            // Calculate key image (WASM only due to spend scalar requirement)
            #[cfg(target_arch = "wasm32")]
            let key_image = {
                let key_image_point = calculate_key_image(&spend_scalar, &output.data.key_offset);
                hex::encode(key_image_point.compress().to_bytes())
            };
            #[cfg(not(target_arch = "wasm32"))]
            let key_image = String::new();

            outputs.push(WalletOutput {
                tx_hash: tx_hash.clone(),
                output_index,
                amount,
                amount_xmr,
                key,
                key_offset,
                commitment_mask,
                subaddress_index,
                payment_id,
                received_output_bytes,
                block_height: 0, // Unconfirmed - in mempool
                spent: false,
                spent_height: None,
                key_image,
                is_coinbase,
                frozen: false,
            });
        }
    }

    Ok(MempoolScanResult {
        tx_count,
        outputs,
        spent_key_images: spent_key_images_set.into_iter().collect(),
    })
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
        let seed = generate_seed("classic").expect("Failed to generate seed");
        let words: Vec<&str> = seed.split_whitespace().collect();
        assert_eq!(words.len(), 25);
    }

    #[test]
    fn test_derive_address_test_vector_1_mainnet() {
        let address = derive_address(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed to derive address from test vector 1");

        assert_eq!(address, TEST_VECTOR_1_ADDRESS);

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_1_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_1_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_1_VIEW_KEY);
    }

    #[test]
    fn test_derive_address_test_vector_2_stagenet() {
        let address = derive_address(TEST_VECTOR_2_SEED, "stagenet")
            .expect("Failed to derive address from test vector 2");

        assert_eq!(address, TEST_VECTOR_2_ADDRESS);

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_2_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_2_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_2_VIEW_KEY);
    }

    #[test]
    fn test_derive_address_test_vector_3_stagenet() {
        let address = derive_address(TEST_VECTOR_3_SEED, "stagenet")
            .expect("Failed to derive address from test vector 3");

        assert_eq!(address, TEST_VECTOR_3_ADDRESS);
    }

    #[test]
    fn test_derive_address_test_vector_4_english() {
        let _address = derive_address(TEST_VECTOR_4_SEED, "mainnet")
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
        let _address = derive_address(TEST_VECTOR_5_SEED, "mainnet")
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
        let _address = derive_address(TEST_VECTOR_6_SEED, "mainnet")
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
        let _address = derive_address(TEST_VECTOR_7_SEED, "mainnet")
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
            derive_address(TEST_VECTOR_1_SEED, "mainnet").expect("Failed for mainnet");
        assert!(mainnet_addr.starts_with("4"));

        let testnet_addr =
            derive_address(TEST_VECTOR_1_SEED, "testnet").expect("Failed for testnet");
        assert!(testnet_addr.starts_with("9") || testnet_addr.starts_with("A"));

        let stagenet_addr =
            derive_address(TEST_VECTOR_2_SEED, "stagenet").expect("Failed for stagenet");
        assert!(stagenet_addr.starts_with("5"));

        assert_ne!(mainnet_addr, testnet_addr);
        assert_ne!(mainnet_addr, stagenet_addr);
        assert_ne!(testnet_addr, stagenet_addr);
    }

    #[test]
    fn test_derive_address_deterministic() {
        let address1 =
            derive_address(TEST_VECTOR_1_SEED, "mainnet").expect("Failed first derivation");
        let address2 =
            derive_address(TEST_VECTOR_1_SEED, "mainnet").expect("Failed second derivation");

        assert_eq!(address1, address2);
        assert_eq!(address1.len(), 95);
    }

    #[test]
    fn test_derive_address_invalid_seed() {
        let result = derive_address("invalid seed words", "mainnet");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_address_invalid_network() {
        let result = derive_address(TEST_VECTOR_1_SEED, "invalidnet");
        assert!(result.is_err());
    }

    #[test]
    fn test_seed_generation_and_address_derivation() {
        let seed = generate_seed("classic").expect("Failed to generate seed");

        let mainnet_address = derive_address(&seed, "mainnet")
            .expect("Failed to derive mainnet address from generated seed");
        assert!(mainnet_address.starts_with("4"));
        assert_eq!(mainnet_address.len(), 95);

        let testnet_address = derive_address(&seed, "testnet")
            .expect("Failed to derive testnet address from generated seed");
        assert!(testnet_address.starts_with("9") || testnet_address.starts_with("A"));

        let stagenet_address = derive_address(&seed, "stagenet")
            .expect("Failed to derive stagenet address from generated seed");
        assert!(stagenet_address.starts_with("5"));
    }

    #[test]
    fn test_derive_keys_test_vector_1() {
        let keys = derive_keys(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed to derive keys from test vector 1");

        assert_eq!(keys.secret_spend_key, TEST_VECTOR_1_SPEND_KEY);
        assert_eq!(keys.secret_view_key, TEST_VECTOR_1_VIEW_KEY);
        assert_eq!(keys.address, TEST_VECTOR_1_ADDRESS);
        assert_eq!(keys.secret_spend_key.len(), 64);
        assert_eq!(keys.secret_view_key.len(), 64);
        assert_eq!(keys.public_spend_key.len(), 64);
        assert_eq!(keys.public_view_key.len(), 64);
    }

    #[test]
    fn test_derive_keys_test_vector_2_stagenet() {
        let keys = derive_keys(TEST_VECTOR_2_SEED, "stagenet")
            .expect("Failed to derive keys from test vector 2");

        assert_eq!(keys.secret_spend_key, TEST_VECTOR_2_SPEND_KEY);
        assert_eq!(keys.secret_view_key, TEST_VECTOR_2_VIEW_KEY);
        assert_eq!(keys.address, TEST_VECTOR_2_ADDRESS);
    }

    #[test]
    fn test_default_lookahead_indices() {
        let indices = build_subaddress_indices(DEFAULT_LOOKAHEAD);
        assert_eq!(indices.len(), 20);
        assert!(indices.contains(&SubaddressIndex::new(0, 1).unwrap()));
        assert!(indices.contains(&SubaddressIndex::new(0, 20).unwrap()));
        assert!(!indices.contains(&SubaddressIndex::new(1, 0).unwrap()));
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
        assert_eq!(DEFAULT_LOOKAHEAD.account, 0);
        assert_eq!(DEFAULT_LOOKAHEAD.subaddress, 20);

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
    fn test_parse_network_case_insensitive() {
        // Already tested in other tests, but let's be explicit
        assert!(matches!(parse_network("MAINNET"), Ok(Network::Mainnet)));
        assert!(matches!(parse_network("mainnet"), Ok(Network::Mainnet)));
        assert!(matches!(parse_network("Mainnet"), Ok(Network::Mainnet)));
        assert!(matches!(parse_network("MaInNeT"), Ok(Network::Mainnet)));
    }

    #[test]
    fn test_spend_key_from_seed_different_seeds() {
        let seed1 = Seed::from_string(Zeroizing::new(TEST_VECTOR_1_SEED.to_string())).unwrap();
        let seed2 = Seed::from_string(Zeroizing::new(TEST_VECTOR_2_SEED.to_string())).unwrap();

        let spend1 = spend_key_from_seed(&seed1);
        let spend2 = spend_key_from_seed(&seed2);

        // Different seeds should produce different spend keys
        assert_ne!(spend1.compress().to_bytes(), spend2.compress().to_bytes());
    }

    #[test]
    fn test_view_key_from_seed_different_seeds() {
        let seed1 = Seed::from_string(Zeroizing::new(TEST_VECTOR_1_SEED.to_string())).unwrap();
        let seed2 = Seed::from_string(Zeroizing::new(TEST_VECTOR_2_SEED.to_string())).unwrap();

        let view1 = view_key_from_seed(&seed1);
        let view2 = view_key_from_seed(&seed2);

        // Different seeds should produce different view keys
        assert_ne!(view1.to_bytes(), view2.to_bytes());
    }

    #[test]
    fn test_derive_keys_invalid_seed() {
        let result = derive_keys("invalid seed phrase", "mainnet");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_keys_invalid_network() {
        let result = derive_keys(TEST_VECTOR_1_SEED, "invalidnet");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_keys_hex_format() {
        let keys = derive_keys(TEST_VECTOR_1_SEED, "mainnet").unwrap();

        // All hex strings should be 64 characters (32 bytes)
        assert_eq!(keys.secret_spend_key.len(), 64);
        assert_eq!(keys.secret_view_key.len(), 64);
        assert_eq!(keys.public_spend_key.len(), 64);
        assert_eq!(keys.public_view_key.len(), 64);

        // Should be valid hex
        assert!(hex::decode(&keys.secret_spend_key).is_ok());
        assert!(hex::decode(&keys.secret_view_key).is_ok());
        assert!(hex::decode(&keys.public_spend_key).is_ok());
        assert!(hex::decode(&keys.public_view_key).is_ok());
    }

    #[test]
    fn test_block_scan_result_serialization() {
        let result = BlockScanResult {
            block_height: 12345,
            block_hash: "abc123".to_string(),
            block_timestamp: 1234567890,
            tx_count: 5,
            outputs: vec![],
            daemon_height: 12350,
            spent_key_images: vec!["key1".to_string(), "key2".to_string()],
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
            amount_xmr: "1.000000000000".to_string(),
            key: "key123".to_string(),
            key_offset: "offset456".to_string(),
            commitment_mask: "mask789".to_string(),
            subaddress_index: Some((0, 1)),
            payment_id: None,
            received_output_bytes: "bytes".to_string(),
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
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: MempoolScanResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.tx_count, deserialized.tx_count);
    }

    #[test]
    fn test_wallet_scan_config_clone() {
        let config = WalletScanConfig {
            mnemonic: TEST_VECTOR_1_SEED.to_string(),
            network: "mainnet".to_string(),
            lookahead: DEFAULT_LOOKAHEAD,
        };

        let cloned = config.clone();
        assert_eq!(config.mnemonic, cloned.mnemonic);
        assert_eq!(config.network, cloned.network);
        assert_eq!(config.lookahead.account, cloned.lookahead.account);
        assert_eq!(config.lookahead.subaddress, cloned.lookahead.subaddress);
    }

    #[test]
    fn test_derived_keys_serialization() {
        let keys = DerivedKeys {
            secret_spend_key: "abc123".to_string(),
            secret_view_key: "def456".to_string(),
            public_spend_key: "ghi789".to_string(),
            public_view_key: "jkl012".to_string(),
            address: "test_address".to_string(),
        };

        let json = serde_json::to_string(&keys).unwrap();
        let deserialized: DerivedKeys = serde_json::from_str(&json).unwrap();
        assert_eq!(keys.secret_spend_key, deserialized.secret_spend_key);
        assert_eq!(keys.address, deserialized.address);
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
}
