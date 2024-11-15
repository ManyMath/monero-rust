//! Unified scanning implementation for Monero wallets.
//!
//! This module provides wallet scanning functionality that works across
//! both native and WASM targets through generic RpcConnection support.

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use monero_serai::{
    rpc::{Rpc, RpcConnection},
    wallet::{
        address::{AddressMeta, AddressType, MoneroAddress, Network, SubaddressIndex},
        seed::{Language, Seed},
        Scanner, ViewPair,
    },
};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::HashSet;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockScanResult {
    pub block_height: u64,
    pub block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: usize,
    pub outputs: Vec<OwnedOutputInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedOutputInfo {
    pub tx_hash: String,
    pub output_index: u8,
    pub amount: u64,
    pub amount_xmr: String,
    pub key: String,
    pub key_offset: String,
    pub commitment_mask: String,
    pub subaddress_index: Option<(u32, u32)>,
    pub payment_id: Option<String>,
    pub received_output_bytes: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DerivedKeys {
    pub secret_spend_key: String,
    pub secret_view_key: String,
    pub public_spend_key: String,
    pub public_view_key: String,
    pub address: String,
}

fn parse_network(network_str: &str) -> Result<Network, String> {
    match network_str.to_lowercase().as_str() {
        "mainnet" => Ok(Network::Mainnet),
        "testnet" => Ok(Network::Testnet),
        "stagenet" => Ok(Network::Stagenet),
        _ => Err(format!("Invalid network: {}", network_str)),
    }
}

fn spend_key_from_seed(seed: &Seed) -> EdwardsPoint {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    &spend_scalar * &ED25519_BASEPOINT_TABLE
}

fn view_key_from_seed(seed: &Seed) -> Scalar {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let view: [u8; 32] = Keccak256::digest(&spend_bytes).into();
    Scalar::from_bytes_mod_order(view)
}

fn register_default_subaddresses(scanner: &mut Scanner) {
    const DEFAULT_ACCOUNT: u32 = 0;
    const SUBADDRESS_LOOKAHEAD: u32 = 50;

    for address in 0..=SUBADDRESS_LOOKAHEAD {
        if let Some(index) = SubaddressIndex::new(DEFAULT_ACCOUNT, address) {
            scanner.register_subaddress(index);
        }
    }
}

pub fn generate_seed() -> Result<String, String> {
    let mut rng = rand::rngs::OsRng;
    let seed = Seed::new(&mut rng, Language::English);
    Ok(Seed::to_string(&seed).to_string())
}

pub fn derive_address(mnemonic: &str, network_str: &str) -> Result<String, String> {
    let network = parse_network(network_str)?;

    let seed = Seed::from_string(Zeroizing::new(mnemonic.to_string()))
        .map_err(|e| format!("Failed to parse seed: {:?}", e))?;

    let spend: [u8; 32] = *seed.entropy();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * &ED25519_BASEPOINT_TABLE;

    let view: [u8; 32] = Keccak256::digest(&spend).into();
    let view_scalar = Scalar::from_bytes_mod_order(view);
    let view_point: EdwardsPoint = &view_scalar * &ED25519_BASEPOINT_TABLE;

    let address = MoneroAddress::new(
        AddressMeta::new(network, AddressType::Standard),
        spend_point,
        view_point,
    );

    Ok(address.to_string())
}

pub fn derive_keys(mnemonic: &str, network_str: &str) -> Result<DerivedKeys, String> {
    let network = parse_network(network_str)?;

    let seed = Seed::from_string(Zeroizing::new(mnemonic.to_string()))
        .map_err(|e| format!("Failed to parse seed: {:?}", e))?;

    let spend: [u8; 32] = *seed.entropy();
    let spend_scalar = Scalar::from_bytes_mod_order(spend);
    let spend_point: EdwardsPoint = &spend_scalar * &ED25519_BASEPOINT_TABLE;

    let view: [u8; 32] = Keccak256::digest(&spend).into();
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

/// Scan a single block for outputs belonging to the given wallet.
///
/// This function is generic over the RPC connection type, allowing it to work
/// with both native HTTP clients and browser-based fetch implementations.
pub async fn scan_block_for_outputs_with_url(
    node_url: &str,
    block_height: u64,
    mnemonic: &str,
    network_str: &str,
) -> Result<BlockScanResult, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use monero_serai::rpc::HttpRpc;
        let rpc = HttpRpc::new(node_url.to_string())
            .map_err(|e| format!("Failed to create RPC: {:?}", e))?;
        scan_block_for_outputs(&rpc, block_height, mnemonic, network_str).await
    }

    #[cfg(target_arch = "wasm32")]
    {
        use crate::rpc_serai::WasmRpcConnection;
        use monero_serai::rpc::Rpc;
        let rpc = Rpc::new_with_connection(WasmRpcConnection::new(node_url.to_string()));
        scan_block_for_outputs(&rpc, block_height, mnemonic, network_str).await
    }
}

pub async fn scan_block_for_outputs<R: RpcConnection>(
    rpc: &Rpc<R>,
    block_height: u64,
    mnemonic: &str,
    network_str: &str,
) -> Result<BlockScanResult, String> {
    let _network = parse_network(network_str)?;

    let seed = Seed::from_string(Zeroizing::new(mnemonic.to_string()))
        .map_err(|e| format!("Invalid mnemonic: {:?}", e))?;

    let spend_point = spend_key_from_seed(&seed);
    let view_scalar = view_key_from_seed(&seed);

    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
    let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));
    register_default_subaddresses(&mut scanner);

    let block_hash_bytes = rpc
        .get_block_hash(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block hash: {:?}", e))?;
    let block_hash = hex::encode(block_hash_bytes);

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

    for tx in all_transactions.iter() {
        let tx_hash = hex::encode(tx.hash());
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

            outputs.push(OwnedOutputInfo {
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
            });
        }
    }

    Ok(BlockScanResult {
        block_height,
        block_hash,
        block_timestamp,
        tx_count,
        outputs,
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

    #[test]
    fn test_generate_seed() {
        let seed = generate_seed().expect("Failed to generate seed");
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
        let seed = generate_seed().expect("Failed to generate seed");

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
}
