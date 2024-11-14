use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use monero_serai::wallet::{
    address::{AddressMeta, AddressType, MoneroAddress, Network},
    seed::{Language, Seed},
};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;
#[cfg(target_arch = "wasm32")]
use monero_serai::ringct::generate_key_image;
use getrandom::getrandom;
use serde::{Serialize, Deserialize};

#[cfg(target_arch = "wasm32")]
pub mod rpc_adapter;

#[cfg(not(target_arch = "wasm32"))]
pub mod rpc_serai;
#[cfg(not(target_arch = "wasm32"))]
pub mod scanner_native;

pub mod tx_builder;
pub mod spent_checker;

#[cfg(not(target_arch = "wasm32"))]
pub use scanner_native::scan_block_for_outputs_with_url;

pub use tx_builder::native;

pub fn test_integration() -> String {
    "monero-wasm works".to_string()
}

pub fn generate_seed() -> Result<String, String> {
    let mut entropy = [0u8; 32];
    getrandom(&mut entropy)
        .map_err(|e| format!("Failed to generate random bytes: {}", e))?;

    let mut rng = rand::rngs::OsRng;
    let seed = Seed::new(&mut rng, Language::English);

    Ok(Seed::to_string(&seed).to_string())
}

pub fn derive_address(mnemonic: &str, network_str: &str) -> Result<String, String> {
    let network = match network_str.to_lowercase().as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "stagenet" => Network::Stagenet,
        _ => return Err(format!("Invalid network: {}", network_str)),
    };

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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DerivedKeys {
    pub secret_spend_key: String,
    pub secret_view_key: String,
    pub public_spend_key: String,
    pub public_view_key: String,
    pub address: String,
}

pub fn derive_keys(mnemonic: &str, network_str: &str) -> Result<DerivedKeys, String> {
    let network = match network_str.to_lowercase().as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "stagenet" => Network::Stagenet,
        _ => return Err(format!("Invalid network: {}", network_str)),
    };

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
        use monero_serai::rpc::Rpc;
        use rpc_adapter::WasmRpcAdapter;
        let adapter = WasmRpcAdapter::new(node_url.to_string());
        let rpc = Rpc::new_with_connection(adapter);
        let height = rpc.get_height()
            .await
            .map_err(|e| format!("Failed to get height: {:?}", e))?;
        Ok(height as u64)
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockScanResult {
    pub block_height: u64,
    pub block_hash: String,
    pub block_timestamp: u64,
    pub tx_count: usize,
    pub outputs: Vec<OwnedOutputInfo>,
    pub daemon_height: u64,
}

#[cfg(target_arch = "wasm32")]
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
    pub block_height: u64,
    pub spent: bool,
    pub key_image: String,
}

#[cfg(target_arch = "wasm32")]
use std::collections::HashSet;

#[cfg(target_arch = "wasm32")]
use monero_serai::{
    rpc::Rpc,
    wallet::{
        ViewPair, Scanner,
    },
};

#[cfg(target_arch = "wasm32")]
use rpc_adapter::WasmRpcAdapter;

#[cfg(target_arch = "wasm32")]
fn spend_key_from_seed_wasm(seed: &monero_serai::wallet::seed::Seed) -> EdwardsPoint {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    &spend_scalar * &ED25519_BASEPOINT_TABLE
}

#[cfg(target_arch = "wasm32")]
fn view_key_from_seed_wasm(seed: &monero_serai::wallet::seed::Seed) -> Scalar {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);

    let view: [u8; 32] = Keccak256::digest(&spend_bytes).into();
    Scalar::from_bytes_mod_order(view)
}

#[cfg(target_arch = "wasm32")]
fn spend_key_scalar_from_seed_wasm(seed: &monero_serai::wallet::seed::Seed) -> Scalar {
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);
    Scalar::from_bytes_mod_order(spend_bytes)
}

#[cfg(target_arch = "wasm32")]
fn calculate_key_image_wasm(spend_scalar: &Scalar, key_offset: &Scalar) -> EdwardsPoint {
    let one_time_key_scalar = Zeroizing::new(spend_scalar + key_offset);
    generate_key_image(&one_time_key_scalar)
}

#[cfg(target_arch = "wasm32")]
pub async fn scan_block_for_outputs_with_url(
    node_url: &str,
    block_height: u64,
    mnemonic: &str,
    network_str: &str,
) -> Result<BlockScanResult, String> {
    let _network = match network_str.to_lowercase().as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "stagenet" => Network::Stagenet,
        _ => return Err(format!("Invalid network: {}", network_str)),
    };

    let seed = monero_serai::wallet::seed::Seed::from_string(Zeroizing::new(mnemonic.to_string()))
        .map_err(|e| format!("Invalid mnemonic: {:?}", e))?;

    let spend_point = spend_key_from_seed_wasm(&seed);
    let view_scalar = view_key_from_seed_wasm(&seed);
    let spend_scalar = spend_key_scalar_from_seed_wasm(&seed);

    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));
    let mut scanner = Scanner::from_view(view_pair, Some(HashSet::new()));

    let adapter = WasmRpcAdapter::new(node_url.to_string());
    let rpc = Rpc::new_with_connection(adapter);

    let block_hash_bytes = rpc.get_block_hash(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block hash: {:?}", e))?;
    let block_hash = hex::encode(block_hash_bytes);

    let daemon_height = rpc.get_height()
        .await
        .map_err(|e| format!("Failed to fetch daemon height: {:?}", e))? as u64;

    let block = rpc.get_block_by_number(block_height as usize)
        .await
        .map_err(|e| format!("Failed to fetch block: {:?}", e))?;

    let block_timestamp = block.header.timestamp;
    let tx_hashes = block.txs.clone();
    let mut all_transactions = vec![block.miner_tx];

    if !tx_hashes.is_empty() {
        let fetched_txs = rpc.get_transactions(&tx_hashes)
            .await
            .map_err(|e| format!("Failed to fetch transactions: {:?}", e))?;
        all_transactions.extend(fetched_txs);
    }

    let tx_count = all_transactions.len();
    let mut outputs = Vec::new();

    for tx in all_transactions.iter() {
        let tx_hash = hex::encode(tx.hash());
        let scan_result = scanner.scan_transaction(&tx);
        let owned_outputs = scan_result.ignore_timelock();

        for output in owned_outputs {
            let amount = output.data.commitment.amount;
            let amount_xmr = format!("{:.12}", amount as f64 / 1_000_000_000_000.0);
            let output_index = output.absolute.o;
            let key = hex::encode(output.data.key.compress().to_bytes());
            let key_offset = hex::encode(output.data.key_offset.to_bytes());
            let subaddress_index = output.metadata.subaddress.map(|idx| (idx.account(), idx.address()));
            let payment_id = if output.metadata.payment_id != [0u8; 8] {
                Some(hex::encode(output.metadata.payment_id))
            } else {
                None
            };

            let commitment_mask = hex::encode(output.data.commitment.mask.to_bytes());
            let received_output_bytes = hex::encode(output.serialize());

            let key_image_point = calculate_key_image_wasm(&spend_scalar, &output.data.key_offset);
            let key_image = hex::encode(key_image_point.compress().to_bytes());

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
                block_height,
                spent: false,
                key_image,
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
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    // Test vectors from https://xmrtests.llcoins.net/addresstests.html
    const TEST_VECTOR_1_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
    const TEST_VECTOR_1_SPEND_KEY: &str = "29adefc8f67515b4b4bf48031780ab9d071d24f8a674b879ce7f245c37523807";
    const TEST_VECTOR_1_VIEW_KEY: &str = "3bc0b202cde92fe5719c3cc0a16aa94f88a5d19f8c515d4e35fae361f6f2120e";
    const TEST_VECTOR_1_ADDRESS: &str = "45wsWad9EwZgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU8K2Dhi";

    // Stagenet test vector from https://monero.stackexchange.com/a/8767
    const TEST_VECTOR_2_SEED: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";
    const TEST_VECTOR_2_SPEND_KEY: &str = "722bbfcf99a9b2c9e700ce857850dd8c4c94c73dca8d914c603f5fee0e365803";
    const TEST_VECTOR_2_VIEW_KEY: &str = "0a1a38f6d246e894600a3e27238a064bf5e8d91801df47a17107596b1378e501";
    const TEST_VECTOR_2_ADDRESS: &str = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

    const TEST_VECTOR_3_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
    const TEST_VECTOR_3_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";

    // Additional test vectors from monero-serai test suite
    const TEST_VECTOR_4_SEED: &str = "washing thirsty occur lectures tuesday fainted toxic adapt abnormal memoir nylon mostly building shrugged online ember northern ruby woes dauntless boil family illness inroads northern";
    const TEST_VECTOR_4_SPEND_KEY: &str = "c0af65c0dd837e666b9d0dfed62745f4df35aed7ea619b2798a709f0fe545403";
    const TEST_VECTOR_4_VIEW_KEY: &str = "513ba91c538a5a9069e0094de90e927c0cd147fa10428ce3ac1afd49f63e3b01";

    const TEST_VECTOR_5_SEED: &str = "minero ocupar mirar evadir octubre cal logro miope opaco disco ancla litio clase cuello nasal clase fiar avance deseo mente grumo negro cordón croqueta clase";
    const TEST_VECTOR_5_SPEND_KEY: &str = "ae2c9bebdddac067d73ec0180147fc92bdf9ac7337f1bcafbbe57dd13558eb02";
    const TEST_VECTOR_5_VIEW_KEY: &str = "18deafb34d55b7a43cae2c1c1c206a3c80c12cc9d1f84640b484b95b7fec3e05";

    const TEST_VECTOR_6_SEED: &str = "poids vaseux tarte bazar poivre effet entier nuance sensuel ennui pacte osselet poudre battre alibi mouton stade paquet pliage gibier type question position projet pliage";
    const TEST_VECTOR_6_SPEND_KEY: &str = "2dd39ff1a4628a94b5c2ec3e42fb3dfe15c2b2f010154dc3b3de6791e805b904";
    const TEST_VECTOR_6_VIEW_KEY: &str = "6725b32230400a1032f31d622b44c3a227f88258939b14a7c72e00939e7bdf0e";

    const TEST_VECTOR_7_SEED: &str = "Kaliber Gabelung Tapir Liveband Favorit Specht Enklave Nabel Jupiter Foliant Chronik nisten löten Vase Aussage Rekord Yeti Gesetz Eleganz Alraune Künstler Almweide Jahr Kastanie Almweide";
    const TEST_VECTOR_7_SPEND_KEY: &str = "79801b7a1b9796856e2397d862a113862e1fdc289a205e79d8d70995b276db06";
    const TEST_VECTOR_7_VIEW_KEY: &str = "99f0ec556643bd9c038a4ed86edcb9c6c16032c4622ed2e000299d527a792701";

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
    fn test_derive_address_test_vector_4_english() {
        let address = derive_address(TEST_VECTOR_4_SEED, "mainnet")
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
        let address = derive_address(TEST_VECTOR_5_SEED, "mainnet")
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
        let address = derive_address(TEST_VECTOR_6_SEED, "mainnet")
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
        let address = derive_address(TEST_VECTOR_7_SEED, "mainnet")
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
        let mainnet_addr = derive_address(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed for mainnet");
        assert!(mainnet_addr.starts_with("4"));

        let testnet_addr = derive_address(TEST_VECTOR_1_SEED, "testnet")
            .expect("Failed for testnet");
        assert!(testnet_addr.starts_with("9") || testnet_addr.starts_with("A"));

        let stagenet_addr = derive_address(TEST_VECTOR_2_SEED, "stagenet")
            .expect("Failed for stagenet");
        assert!(stagenet_addr.starts_with("5"));

        assert_ne!(mainnet_addr, testnet_addr);
        assert_ne!(mainnet_addr, stagenet_addr);
        assert_ne!(testnet_addr, stagenet_addr);
    }

    #[test]
    fn test_derive_address_deterministic() {
        let address1 = derive_address(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed first derivation");
        let address2 = derive_address(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed second derivation");

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

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    // Test vectors from https://xmrtests.llcoins.net/addresstests.html
    const TEST_VECTOR_1_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";
    const TEST_VECTOR_1_SPEND_KEY: &str = "29adefc8f67515b4b4bf48031780ab9d071d24f8a674b879ce7f245c37523807";
    const TEST_VECTOR_1_VIEW_KEY: &str = "3bc0b202cde92fe5719c3cc0a16aa94f88a5d19f8c515d4e35fae361f6f2120e";
    const TEST_VECTOR_1_ADDRESS: &str = "45wsWad9EwZgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU8K2Dhi";

    const TEST_VECTOR_2_SEED: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";
    const TEST_VECTOR_2_SPEND_KEY: &str = "722bbfcf99a9b2c9e700ce857850dd8c4c94c73dca8d914c603f5fee0e365803";
    const TEST_VECTOR_2_VIEW_KEY: &str = "0a1a38f6d246e894600a3e27238a064bf5e8d91801df47a17107596b1378e501";
    const TEST_VECTOR_2_ADDRESS: &str = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

    const TEST_VECTOR_4_SEED: &str = "washing thirsty occur lectures tuesday fainted toxic adapt abnormal memoir nylon mostly building shrugged online ember northern ruby woes dauntless boil family illness inroads northern";
    const TEST_VECTOR_4_SPEND_KEY: &str = "c0af65c0dd837e666b9d0dfed62745f4df35aed7ea619b2798a709f0fe545403";
    const TEST_VECTOR_4_VIEW_KEY: &str = "513ba91c538a5a9069e0094de90e927c0cd147fa10428ce3ac1afd49f63e3b01";

    #[wasm_bindgen_test]
    fn wasm_test_generate_seed() {
        let seed = generate_seed().expect("Failed to generate seed in WASM");
        let words: Vec<&str> = seed.split_whitespace().collect();
        assert_eq!(words.len(), 25);
    }

    #[wasm_bindgen_test]
    fn wasm_test_derive_address_test_vector_1_mainnet() {
        let address = derive_address(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed to derive address from test vector 1 in WASM");

        assert_eq!(address, TEST_VECTOR_1_ADDRESS);

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_1_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_1_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_1_VIEW_KEY);
    }

    #[wasm_bindgen_test]
    fn wasm_test_derive_address_test_vector_2_stagenet() {
        let address = derive_address(TEST_VECTOR_2_SEED, "stagenet")
            .expect("Failed to derive address from test vector 2 in WASM");

        assert_eq!(address, TEST_VECTOR_2_ADDRESS);

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_2_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_2_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_2_VIEW_KEY);
    }

    #[wasm_bindgen_test]
    fn wasm_test_derive_address_test_vector_4_english() {
        let address = derive_address(TEST_VECTOR_4_SEED, "mainnet")
            .expect("Failed to derive address from test vector 4 in WASM");

        let seed = Seed::from_string(Zeroizing::new(TEST_VECTOR_4_SEED.to_string())).unwrap();
        let spend: [u8; 32] = *seed.entropy();
        assert_eq!(hex::encode(spend), TEST_VECTOR_4_SPEND_KEY);

        let view: [u8; 32] = Keccak256::digest(&spend).into();
        let view_scalar = Scalar::from_bytes_mod_order(view);
        assert_eq!(hex::encode(view_scalar.to_bytes()), TEST_VECTOR_4_VIEW_KEY);
    }

    #[wasm_bindgen_test]
    fn wasm_test_derive_address_networks() {
        let mainnet_addr = derive_address(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed for mainnet in WASM");
        assert!(mainnet_addr.starts_with("4"));

        let testnet_addr = derive_address(TEST_VECTOR_1_SEED, "testnet")
            .expect("Failed for testnet in WASM");
        assert!(testnet_addr.starts_with("9") || testnet_addr.starts_with("A"));

        let stagenet_addr = derive_address(TEST_VECTOR_2_SEED, "stagenet")
            .expect("Failed for stagenet in WASM");
        assert!(stagenet_addr.starts_with("5"));

        assert_ne!(mainnet_addr, testnet_addr);
        assert_ne!(mainnet_addr, stagenet_addr);
        assert_ne!(testnet_addr, stagenet_addr);
    }

    #[wasm_bindgen_test]
    fn wasm_test_derive_address_deterministic() {
        let address1 = derive_address(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed first derivation in WASM");
        let address2 = derive_address(TEST_VECTOR_1_SEED, "mainnet")
            .expect("Failed second derivation in WASM");

        assert_eq!(address1, address2);
        assert_eq!(address1.len(), 95);
    }

    #[wasm_bindgen_test]
    fn wasm_test_seed_generation_and_address_derivation() {
        let seed = generate_seed().expect("Failed to generate seed in WASM");

        let mainnet_address = derive_address(&seed, "mainnet")
            .expect("Failed to derive mainnet address from generated seed in WASM");
        assert!(mainnet_address.starts_with("4"));
        assert_eq!(mainnet_address.len(), 95);

        let testnet_address = derive_address(&seed, "testnet")
            .expect("Failed to derive testnet address from generated seed in WASM");
        assert!(testnet_address.starts_with("9") || testnet_address.starts_with("A"));

        let stagenet_address = derive_address(&seed, "stagenet")
            .expect("Failed to derive stagenet address from generated seed in WASM");
        assert!(stagenet_address.starts_with("5"));
    }
}
