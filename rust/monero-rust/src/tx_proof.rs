use monero_wallet::address::{MoneroAddress, Network};
use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar};
use getrandom::getrandom;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::ops::Deref;
use zeroize::Zeroizing;

/// Domain separator for OutProofV2 (from Monero's config.cpp)
const HASH_KEY_TXPROOF_V2: &[u8] = b"TXPROOF_V2";

/// Result of generating an OutProof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutProofResult {
    /// The OutProofV2 signature string (base58 encoded)
    pub signature: String,
    /// The formatted proof in Feather Wallet style
    pub formatted: String,
}

fn hash_to_scalar(data: &[u8]) -> Scalar {
    let hash: [u8; 32] = Keccak256::digest(data).into();
    Scalar::from_bytes_mod_order(hash)
}

/// Generate OutProofV2 signature
///
/// This proves that the signer knows the tx_key (r) such that:
/// - R = r*G (the public tx key in the transaction)
/// - D = r*A (the ECDH derivation with recipient's view key)
///
/// Without revealing r itself.
pub fn generate_out_proof_v2(
    tx_id: &str,
    tx_key_hex: &str,
    recipient_address: &str,
    message: &str,
    network_str: &str,
) -> Result<OutProofResult, String> {
    let network = match network_str.to_lowercase().as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "stagenet" => Network::Stagenet,
        _ => return Err(format!("Invalid network: {}", network_str)),
    };
    // Parse tx_key (r scalar)
    let tx_key_bytes = hex::decode(tx_key_hex).map_err(|e| format!("Invalid tx_key hex: {}", e))?;
    if tx_key_bytes.len() != 32 {
        return Err("tx_key must be 32 bytes".to_string());
    }
    let mut r_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&tx_key_bytes);
    let r = Zeroizing::new(Scalar::from_bytes_mod_order(r_bytes));

    // Parse recipient address
    let address = MoneroAddress::from_str(network, recipient_address)
        .map_err(|e| format!("Invalid address: {:?}", e))?;

    // R = r*G (public tx key)
    let r_point: EdwardsPoint = r.deref() * ED25519_BASEPOINT_TABLE;

    // A = recipient's view public key
    let a_point: EdwardsPoint = address.view().into();

    // B = recipient's spend public key (for standard addresses, optional for subaddresses)
    let b_point: EdwardsPoint = address.spend().into();

    // D = r*A (ECDH shared secret / key derivation)
    let d_point: EdwardsPoint = r.deref() * a_point;

    // Parse tx_id as the message prefix
    let tx_id_bytes = hex::decode(tx_id).map_err(|e| format!("Invalid tx_id hex: {}", e))?;
    if tx_id_bytes.len() != 32 {
        return Err("tx_id must be 32 bytes".to_string());
    }

    // Generate random k for Schnorr signature
    let mut k_bytes = [0u8; 32];
    getrandom(&mut k_bytes).map_err(|e| format!("RNG failed: {}", e))?;
    let k = Zeroizing::new(Scalar::from_bytes_mod_order(k_bytes));

    // X = k*G
    let x_point: EdwardsPoint = k.deref() * ED25519_BASEPOINT_TABLE;

    // Y = k*A
    let y_point: EdwardsPoint = k.deref() * a_point;

    // Build message hash for V2 proof
    // H(domain_sep || msg || D || X || Y || R || A || B)
    let msg_hash = if message.is_empty() {
        tx_id_bytes.clone()
    } else {
        // Hash the message with tx_id
        let mut hasher = Keccak256::new();
        hasher.update(&tx_id_bytes);
        hasher.update(message.as_bytes());
        hasher.finalize().to_vec()
    };

    let mut challenge_data = Vec::new();
    challenge_data.extend_from_slice(&msg_hash);
    challenge_data.extend_from_slice(&d_point.compress().to_bytes());
    challenge_data.extend_from_slice(&x_point.compress().to_bytes());
    challenge_data.extend_from_slice(&y_point.compress().to_bytes());
    challenge_data.extend_from_slice(&r_point.compress().to_bytes());
    challenge_data.extend_from_slice(&a_point.compress().to_bytes());
    challenge_data.extend_from_slice(&b_point.compress().to_bytes());
    challenge_data.extend_from_slice(HASH_KEY_TXPROOF_V2);

    let c = hash_to_scalar(&challenge_data);

    // s = k - c*r (Monero uses subtraction form)
    let s = k.deref() - (c * r.deref());

    // Encode signature as OutProofV2
    // Format: "OutProofV2" + base58(D || c || s)
    let mut sig_data = Vec::with_capacity(32 + 32 + 32);
    sig_data.extend_from_slice(&d_point.compress().to_bytes());
    sig_data.extend_from_slice(&c.to_bytes());
    sig_data.extend_from_slice(&s.to_bytes());

    let signature = format!(
        "OutProofV2{}",
        base58_monero::encode(&sig_data).map_err(|e| format!("Base58 encode failed: {:?}", e))?
    );

    // Build formatted proof (Feather Wallet style)
    let network_name = match network {
        Network::Mainnet => "Monero Mainnet",
        Network::Testnet => "Monero Testnet",
        Network::Stagenet => "Monero Stagenet",
    };

    let formatted = if message.is_empty() {
        format!(
            "-----BEGIN OUTPROOF-----\n\
             Network: {}\n\
             Txid: {}\n\
             Address: {}\n\
             -----BEGIN OUTPROOF SIGNATURE-----\n\
             {}\n\
             -----END OUTPROOF SIGNATURE-----",
            network_name, tx_id, recipient_address, signature
        )
    } else {
        format!(
            "-----BEGIN OUTPROOF-----\n\
             Network: {}\n\
             Txid: {}\n\
             Address: {}\n\
             \n\
             {}\n\
             -----BEGIN OUTPROOF SIGNATURE-----\n\
             {}\n\
             -----END OUTPROOF SIGNATURE-----",
            network_name, tx_id, recipient_address, message, signature
        )
    };

    Ok(OutProofResult {
        signature,
        formatted,
    })
}

/// Verify an OutProofV2 signature.
///
/// `tx_public_key_hex` is the transaction public key R from the tx extra field.
pub fn verify_out_proof_v2(
    tx_id: &str,
    recipient_address: &str,
    message: &str,
    signature: &str,
    network_str: &str,
    tx_public_key_hex: &str,
) -> Result<bool, String> {
    let network = match network_str.to_lowercase().as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "stagenet" => Network::Stagenet,
        _ => return Err(format!("Invalid network: {}", network_str)),
    };

    // Parse tx public key R
    let r_bytes =
        hex::decode(tx_public_key_hex).map_err(|e| format!("Invalid tx_public_key hex: {}", e))?;
    if r_bytes.len() != 32 {
        return Err("tx_public_key must be 32 bytes".to_string());
    }
    let mut r_arr = [0u8; 32];
    r_arr.copy_from_slice(&r_bytes);
    let r_point = curve25519_dalek::edwards::CompressedEdwardsY(r_arr)
        .decompress()
        .ok_or("Invalid tx public key point")?;

    // Strip "OutProofV2" prefix
    let sig_str = signature
        .strip_prefix("OutProofV2")
        .ok_or("Signature must start with OutProofV2")?;

    // Decode base58
    let sig_data =
        base58_monero::decode(sig_str).map_err(|e| format!("Invalid base58: {:?}", e))?;

    if sig_data.len() != 96 {
        return Err(format!(
            "Invalid signature length: expected 96, got {}",
            sig_data.len()
        ));
    }

    // Parse D, c, s from signature
    let d_bytes: [u8; 32] = sig_data[0..32].try_into().unwrap();
    let c_bytes: [u8; 32] = sig_data[32..64].try_into().unwrap();
    let s_bytes: [u8; 32] = sig_data[64..96].try_into().unwrap();

    let d_point = curve25519_dalek::edwards::CompressedEdwardsY(d_bytes)
        .decompress()
        .ok_or("Invalid D point")?;
    let c = Scalar::from_bytes_mod_order(c_bytes);
    let s = Scalar::from_bytes_mod_order(s_bytes);

    // Parse recipient address
    let address = MoneroAddress::from_str(network, recipient_address)
        .map_err(|e| format!("Invalid address: {:?}", e))?;

    let a_point: EdwardsPoint = address.view().into();
    let b_point: EdwardsPoint = address.spend().into();

    // Parse tx_id
    let tx_id_bytes = hex::decode(tx_id).map_err(|e| format!("Invalid tx_id hex: {}", e))?;
    if tx_id_bytes.len() != 32 {
        return Err("tx_id must be 32 bytes".to_string());
    }

    // Reconstruct X' = s*G + c*R
    let x_prime = &s * ED25519_BASEPOINT_TABLE + c * r_point;

    // Reconstruct Y' = s*A + c*D
    let y_prime = s * a_point + c * d_point;

    // Recompute challenge hash
    let msg_hash = if message.is_empty() {
        tx_id_bytes.clone()
    } else {
        let mut hasher = Keccak256::new();
        hasher.update(&tx_id_bytes);
        hasher.update(message.as_bytes());
        hasher.finalize().to_vec()
    };

    let mut challenge_data = Vec::new();
    challenge_data.extend_from_slice(&msg_hash);
    challenge_data.extend_from_slice(&d_point.compress().to_bytes());
    challenge_data.extend_from_slice(&x_prime.compress().to_bytes());
    challenge_data.extend_from_slice(&y_prime.compress().to_bytes());
    challenge_data.extend_from_slice(&r_point.compress().to_bytes());
    challenge_data.extend_from_slice(&a_point.compress().to_bytes());
    challenge_data.extend_from_slice(&b_point.compress().to_bytes());
    challenge_data.extend_from_slice(HASH_KEY_TXPROOF_V2);

    let c_prime = hash_to_scalar(&challenge_data);

    Ok(c == c_prime)
}

// TODO: Add deterministic OutProofV2 test using a monero-wallet-cli test
// vector.  Generate a proof with `get_tx_proof` in monero-wallet-cli, then
// verify our implementation produces the same signature given the same inputs
// and nonce (k):
// - Make the random k deterministic via a seeded RNG for testing
// - Verify our proof against monero-wallet-cli's `check_tx_proof`

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_to_scalar() {
        let data = b"test data";
        let scalar = hash_to_scalar(data);

        // Same input should produce same scalar
        let scalar2 = hash_to_scalar(data);
        assert_eq!(scalar.to_bytes(), scalar2.to_bytes());

        // Different input should produce different scalar
        let scalar3 = hash_to_scalar(b"different data");
        assert_ne!(scalar.to_bytes(), scalar3.to_bytes());
    }

    #[test]
    fn test_generate_out_proof() {
        // Test with known values
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

        let result = generate_out_proof_v2(tx_id, tx_key, address, "", "stagenet");
        assert!(result.is_ok());

        let proof = result.unwrap();
        assert!(proof.signature.starts_with("OutProofV2"));
        assert!(proof.formatted.contains("BEGIN OUTPROOF"));
        assert!(proof.formatted.contains("Monero Stagenet"));
    }

    #[test]
    fn test_generate_out_proof_with_message() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";
        let message = "Payment for services";

        let result = generate_out_proof_v2(tx_id, tx_key, address, message, "stagenet");
        assert!(result.is_ok());

        let proof = result.unwrap();
        assert!(proof.formatted.contains(message));
        assert!(proof.signature.starts_with("OutProofV2"));
    }

    #[test]
    fn test_generate_out_proof_mainnet() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let address = "45wsWad9EwZgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU8K2Dhi";

        let result = generate_out_proof_v2(tx_id, tx_key, address, "", "mainnet");
        assert!(result.is_ok());

        let proof = result.unwrap();
        assert!(proof.formatted.contains("Monero Mainnet"));
    }

    #[test]
    fn test_generate_out_proof_invalid_tx_id() {
        let tx_id = "invalid";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

        let result = generate_out_proof_v2(tx_id, tx_key, address, "", "stagenet");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_out_proof_invalid_tx_key() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let tx_key = "invalid";
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

        let result = generate_out_proof_v2(tx_id, tx_key, address, "", "stagenet");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_out_proof_invalid_address() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let address = "invalid_address";

        let result = generate_out_proof_v2(tx_id, tx_key, address, "", "stagenet");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_out_proof_wrong_network() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";
        // This is a stagenet address
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

        // Should fail when trying to parse stagenet address as mainnet
        let result = generate_out_proof_v2(tx_id, tx_key, address, "", "mainnet");
        assert!(result.is_err());
    }

    /// Compute R = r*G for a given tx_key hex and return R as hex.
    fn tx_public_key(tx_key_hex: &str) -> String {
        let tx_key_bytes = hex::decode(tx_key_hex).unwrap();
        let mut r_bytes = [0u8; 32];
        r_bytes.copy_from_slice(&tx_key_bytes);
        let r = Scalar::from_bytes_mod_order(r_bytes);
        let r_point = &r * ED25519_BASEPOINT_TABLE;
        hex::encode(r_point.compress().to_bytes())
    }

    #[test]
    fn test_verify_out_proof_invalid_prefix() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";
        let signature = "InvalidPrefix123";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";

        let result = verify_out_proof_v2(
            tx_id,
            address,
            "",
            signature,
            "stagenet",
            &tx_public_key(tx_key),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_out_proof_invalid_base58() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";
        let signature = "OutProofV2!@#$%^&*()";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";

        let result = verify_out_proof_v2(
            tx_id,
            address,
            "",
            signature,
            "stagenet",
            &tx_public_key(tx_key),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_out_proof_roundtrip() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";
        let r_pub = tx_public_key(tx_key);

        // Generate a valid proof
        let proof = generate_out_proof_v2(tx_id, tx_key, address, "", "stagenet").unwrap();

        // Verify should succeed with correct R
        let result = verify_out_proof_v2(tx_id, address, "", &proof.signature, "stagenet", &r_pub);
        assert!(result.is_ok());
        assert!(result.unwrap(), "valid proof must verify as true");
    }

    #[test]
    fn test_verify_out_proof_wrong_tx_key_fails() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let wrong_key = "0000000000000000000000000000000000000000000000000000000000000002";
        let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

        let proof = generate_out_proof_v2(tx_id, tx_key, address, "", "stagenet").unwrap();

        // Verify with wrong R should fail
        let result = verify_out_proof_v2(
            tx_id,
            address,
            "",
            &proof.signature,
            "stagenet",
            &tx_public_key(wrong_key),
        );
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "proof with wrong tx key must verify as false"
        );
    }

    #[test]
    fn test_out_proof_result_serialization() {
        let result = OutProofResult {
            signature: "OutProofV2abc123".to_string(),
            formatted: "formatted proof".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: OutProofResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.signature, deserialized.signature);
        assert_eq!(result.formatted, deserialized.formatted);
    }
}
