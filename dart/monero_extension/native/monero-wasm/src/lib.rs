use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use monero_serai::wallet::{
    address::{AddressMeta, AddressType, MoneroAddress, Network},
    seed::{Language, Seed},
};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;
use getrandom::getrandom;

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

    // Standard Monero key derivation: spend_key = entropy, view_key = H(spend_key)
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

        println!("Test vector 1 mainnet address: {}", address);
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

        println!("Test vector 2 stagenet address: {}", address);
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

        println!("Test vector 3 stagenet address: {}", address);
        assert_eq!(address, TEST_VECTOR_3_ADDRESS);
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
}
