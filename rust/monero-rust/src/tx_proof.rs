use monero_payment_proofs::OutProof;
use monero_wallet::address::{MoneroAddress, Network};
use curve25519_dalek::scalar::Scalar;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Result of generating an OutProof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutProofResult {
    /// The OutProofV2 signature string (base58 encoded)
    pub signature: String,
    /// The formatted proof in Feather Wallet style
    pub formatted: String,
}

fn parse_network(network_str: &str) -> Result<Network, String> {
    match network_str.to_lowercase().as_str() {
        "mainnet" => Ok(Network::Mainnet),
        "testnet" => Ok(Network::Testnet),
        "stagenet" => Ok(Network::Stagenet),
        _ => Err(format!("Invalid network: {}", network_str)),
    }
}

fn parse_32(hex_str: &str, what: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_str).map_err(|e| format!("Invalid {} hex: {}", what, e))?;
    bytes
        .try_into()
        .map_err(|_| format!("{} must be 32 bytes", what))
}

/// Concatenate `txid || message`; the proof implementation hashes this to form
/// Monero's prefix hash.
fn prefix_data(tx_id_bytes: &[u8; 32], message: &str) -> Vec<u8> {
    let mut data = tx_id_bytes.to_vec();
    data.extend_from_slice(message.as_bytes());
    data
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
    let network = parse_network(network_str)?;
    let tx_id_bytes = parse_32(tx_id, "tx_id")?;
    let r = Zeroizing::new(Scalar::from_bytes_mod_order(parse_32(tx_key_hex, "tx_key")?));

    let address = MoneroAddress::from_str(network, recipient_address)
        .map_err(|e| format!("Invalid address: {:?}", e))?;

    let proof = OutProof::prove(&mut OsRng, &address, &r, &prefix_data(&tx_id_bytes, message));
    let signature = proof.to_string();

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
    let network = parse_network(network_str)?;
    let tx_id_bytes = parse_32(tx_id, "tx_id")?;

    let r_point = curve25519_dalek::edwards::CompressedEdwardsY(parse_32(
        tx_public_key_hex,
        "tx_public_key",
    )?)
    .decompress()
    .ok_or("Invalid tx public key point")?;

    let address = MoneroAddress::from_str(network, recipient_address)
        .map_err(|e| format!("Invalid address: {:?}", e))?;

    let proofs = OutProof::read(signature).ok_or("Malformed OutProofV2 signature")?;
    let [proof] = proofs.as_slice() else {
        return Err(format!(
            "Expected a single OutProof, got {}",
            proofs.len()
        ));
    };

    Ok(proof.verify_signature(&address, &prefix_data(&tx_id_bytes, message), r_point))
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
    use sha3::{Digest, Keccak256};

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
        let r_point = &r * curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
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
    fn test_verify_out_proof_base58_addition_overflow() {
        let signature = format!("OutProofV2jpXCZedGfVR{}", "1".repeat(121));
        let tx_id = "aa".repeat(32);
        let tx_pub_key = hex::encode(
            curve25519_dalek::constants::ED25519_BASEPOINT_POINT
                .compress()
                .to_bytes(),
        );
        assert!(verify_out_proof_v2(
            &tx_id,
            "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt",
            "",
            &signature,
            "stagenet",
            &tx_pub_key,
        )
        .is_err());
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

    /// Monero hashes `s_comm_2` as raw struct memory in `crypto::generate_tx_proof`
    /// (monero src/crypto/crypto.cpp): msg || D || X || Y || sep || R || A || B, where
    /// `msg = keccak(txid || message)`, `sep = keccak("TXPROOF_V2")`, and B is zeroed for
    /// non-subaddress recipients. Rebuilt here so the challenge stays pinned to that layout.
    fn monero_challenge(
        tx_id: &str,
        message: &str,
        d: EdwardsPoint,
        x: EdwardsPoint,
        y: EdwardsPoint,
        tx_pub_key: EdwardsPoint,
        address: &MoneroAddress,
    ) -> Scalar {
        let mut msg = hex::decode(tx_id).unwrap();
        msg.extend_from_slice(message.as_bytes());

        let mut buf = Vec::with_capacity(8 * 32);
        buf.extend_from_slice(&<[u8; 32]>::from(Keccak256::digest(&msg)));
        buf.extend_from_slice(&d.compress().to_bytes());
        buf.extend_from_slice(&x.compress().to_bytes());
        buf.extend_from_slice(&y.compress().to_bytes());
        buf.extend_from_slice(&<[u8; 32]>::from(Keccak256::digest(b"TXPROOF_V2")));
        buf.extend_from_slice(&tx_pub_key.compress().to_bytes());
        buf.extend_from_slice(&point(address.view()).compress().to_bytes());
        if address.is_subaddress() {
            buf.extend_from_slice(&point(address.spend()).compress().to_bytes());
        } else {
            buf.extend_from_slice(&[0u8; 32]);
        }

        Scalar::from_bytes_mod_order(Keccak256::digest(&buf).into())
    }

    fn point(p: monero_wallet::ed25519::Point) -> EdwardsPoint {
        p.into()
    }

    /// Decode an OutProofV2 string into `(D, c, s)`.
    fn decode_proof(signature: &str) -> (EdwardsPoint, Scalar, Scalar) {
        let payload = signature.strip_prefix("OutProofV2").expect("prefix");
        let data = base58_monero::decode(payload).expect("base58");
        assert_eq!(data.len(), 96, "proof payload must be D || c || s");

        let d = CompressedEdwardsY(data[0 .. 32].try_into().unwrap())
            .decompress()
            .expect("D on curve");
        let c = Scalar::from_bytes_mod_order(data[32 .. 64].try_into().unwrap());
        let s = Scalar::from_bytes_mod_order(data[64 .. 96].try_into().unwrap());
        (d, c, s)
    }

    /// Check the challenge for standard addresses (zero B, generator G)
    /// and subaddresses (recipient B as generator).
    #[test]
    fn out_proof_matches_monero_challenge() {
        let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
        let message = "spec conformance";
        let tx_key = "0000000000000000000000000000000000000000000000000000000000000001";

        let r = Scalar::from_bytes_mod_order(parse_32(tx_key, "tx_key").unwrap());

        // Use this view pair to derive standard and subaddress recipients.
        let spend = &Scalar::from(7u64) * curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
        let view = Zeroizing::new(monero_wallet::ed25519::Scalar::from(Scalar::from(9u64)));
        let view_pair =
            monero_wallet::ViewPair::new(monero_wallet::ed25519::Point::from(spend), view).unwrap();

        let standard = view_pair.legacy_address(Network::Stagenet);
        let subaddress = view_pair.subaddress(
            Network::Stagenet,
            monero_wallet::address::SubaddressIndex::new(0, 1).unwrap(),
        );

        for address in [standard, subaddress] {
            let is_sub = address.is_subaddress();
            // Subaddresses use `R = r*B`; standard addresses use `R = r*G`.
            let tx_pub_key = if is_sub {
                point(address.spend()) * r
            } else {
                &r * curve25519_dalek::constants::ED25519_BASEPOINT_TABLE
            };

            let proof =
                generate_out_proof_v2(tx_id, tx_key, &address.to_string(), message, "stagenet")
                    .expect("generate");
            let (d, c, s) = decode_proof(&proof.signature);

            // Recover the nonce commitments from the signature.
            let generator = if is_sub {
                point(address.spend())
            } else {
                curve25519_dalek::constants::ED25519_BASEPOINT_POINT
            };
            let x = (generator * s) + (c * tx_pub_key);
            let y = (point(address.view()) * s) + (c * d);

            assert_eq!(
                c,
                monero_challenge(tx_id, message, d, x, y, tx_pub_key, &address),
                "challenge must match Monero's s_comm_2 layout (subaddress: {})",
                is_sub
            );

            // Check the public verification API too.
            assert!(verify_out_proof_v2(
                tx_id,
                &address.to_string(),
                message,
                &proof.signature,
                "stagenet",
                &hex::encode(tx_pub_key.compress().to_bytes()),
            )
            .unwrap());
        }
    }
}
