//! Key image import and verification for view-only wallets.
//!
//! View-only wallets can scan for incoming outputs but cannot detect spends
//! (since computing a key image requires the secret spend key).  A full wallet
//! can export its key images so they can be imported into a view-only wallet,
//! allowing accurate balance calculation.
//!
//! This module supports the **monero-wallet-rpc** JSON export format
//! (`export_key_images` RPC method).
//!
//! Each exported key image carries a ring-signature proof (ring size 1,
//! Schnorr-like) that it was computed correctly from the output's one-time
//! public key.  We verify that proof before accepting the import.

use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE,
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
    traits::IsIdentity,
};
use sha3::{Digest, Keccak256};

/// Monero's biased hash-to-point, `H_p(key)`.
fn hash_to_point(key: EdwardsPoint) -> EdwardsPoint {
    monero_wallet::ed25519::Point::biased_hash(key.compress().to_bytes()).into()
}

/// A single key image with its signature, as returned by monero-wallet-rpc.
#[derive(Debug, Clone)]
pub struct SignedKeyImage {
    /// The key image bytes (32 bytes, compressed Edwards point).
    pub key_image: [u8; 32],
    /// The signature bytes (64 bytes: c || r, each 32 bytes little-endian scalar).
    pub signature: [u8; 64],
}

/// Signed key images and their starting position in Monero export order.
///
/// Entry `n` belongs to output `n + offset`. A partial export, such as one made by
/// `monero-wallet-cli export_key_images` without `all`, may skip leading outputs.
/// Keep the offset with the entries so they can be assigned to the correct outputs.
#[derive(Debug, Clone)]
pub struct SignedKeyImageImport {
    /// Position of the first entry in Monero export order.
    pub offset: u32,
    /// Key images and their signatures, in export order.
    pub key_images: Vec<SignedKeyImage>,
}

impl SignedKeyImageImport {
    /// Return the key images as hex, in export order. The caller must retain
    /// `self.offset` separately to assign them to outputs.
    pub fn key_images_hex(&self) -> Vec<String> {
        self.key_images
            .iter()
            .map(|ski| hex::encode(ski.key_image))
            .collect()
    }
}

/// Result of importing key images into a set of outputs.
#[derive(Debug, Clone)]
pub struct KeyImageImportResult {
    /// Number of outputs that had their key image updated.
    pub updated: usize,
    /// Number of key images that failed verification (skipped).
    pub failed: usize,
}

/// Parse a monero-wallet-rpc `export_key_images` JSON response.
///
/// Accepts either the full RPC response
/// `{"result": {"offset": N, "signed_key_images": [...]}}` or the bare result
/// body `{"offset": N, "signed_key_images": [...]}`.
/// Returns `(offset, signed_key_images)`.
pub fn parse_rpc_export(json: &str) -> Result<(usize, Vec<SignedKeyImage>), String> {
    #[derive(serde::Deserialize)]
    struct Resp {
        result: RpcResult,
    }
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum RpcExport {
        Wrapped(Resp),
        Bare(RpcResult),
    }
    #[derive(serde::Deserialize)]
    struct RpcResult {
        offset: usize,
        signed_key_images: Vec<RpcSignedKeyImage>,
    }
    #[derive(serde::Deserialize)]
    struct RpcSignedKeyImage {
        key_image: String,
        signature: String,
    }

    let export: RpcExport = serde_json::from_str(json)
        .map_err(|e| format!("failed to parse key image export JSON: {e}"))?;
    let result = match export {
        RpcExport::Wrapped(resp) => resp.result,
        RpcExport::Bare(result) => result,
    };

    let mut out = Vec::with_capacity(result.signed_key_images.len());
    for (i, entry) in result.signed_key_images.iter().enumerate() {
        let ki_bytes: [u8; 32] = hex::decode(&entry.key_image)
            .map_err(|e| format!("key image {i}: bad hex: {e}"))?
            .try_into()
            .map_err(|_| format!("key image {i}: expected 32 bytes"))?;
        let sig_bytes: [u8; 64] = hex::decode(&entry.signature)
            .map_err(|e| format!("signature {i}: bad hex: {e}"))?
            .try_into()
            .map_err(|_| format!("signature {i}: expected 64 bytes"))?;
        out.push(SignedKeyImage {
            key_image: ki_bytes,
            signature: sig_bytes,
        });
    }
    Ok((result.offset, out))
}

/// Verify a key image export signature (ring size 1). Refuses a key image that
/// is the identity or has torsion, and non-canonical `c` or `r`.
pub fn verify_key_image_signature(
    output_key: &EdwardsPoint,
    key_image: &EdwardsPoint,
    signature: &[u8; 64],
) -> bool {
    // Require a key image in the prime-order subgroup, as Monero does with
    // `l * I == identity`. An identity check alone would accept `x*Hp(P) + T`
    // for nonzero torsion `T` whenever the challenge cancels `T`. A signer can
    // choose nonces until that happens, producing multiple key images for one output.
    if key_image.is_identity() || !key_image.is_torsion_free() {
        return false;
    }

    // Parse c and r from the signature.
    let mut c_bytes = [0u8; 32];
    let mut r_bytes = [0u8; 32];
    c_bytes.copy_from_slice(&signature[..32]);
    r_bytes.copy_from_slice(&signature[32..]);
    let (Some(c), Some(r)) = (
        Option::<Scalar>::from(Scalar::from_canonical_bytes(c_bytes)),
        Option::<Scalar>::from(Scalar::from_canonical_bytes(r_bytes)),
    ) else {
        return false;
    };

    // L = c*P + r*G
    let l_point = (c * output_key) + (&r * ED25519_BASEPOINT_TABLE);

    // R = r*Hp(P) + c*I
    let hp = hash_to_point(*output_key);
    let r_point = (r * hp) + (c * key_image);

    // prefix_hash = raw key_image bytes (compressed)
    let ki_compressed = key_image.compress().to_bytes();

    let mut hasher = Keccak256::new();
    hasher.update(ki_compressed);
    hasher.update(l_point.compress().to_bytes());
    hasher.update(r_point.compress().to_bytes());
    let hash: [u8; 32] = hasher.finalize().into();

    let expected_c = Scalar::from_bytes_mod_order(hash);
    c == expected_c
}

/// Verify and import key images into wallet outputs.
///
/// `output_keys` provides the one-time public key (hex, 32 bytes compressed)
/// for each output starting at index 0.  `signed_key_images` are matched
/// positionally: `signed_key_images[0]` corresponds to `output_keys[offset]`.
///
/// Returns hex-encoded key image strings for outputs that pass verification,
/// empty strings for those that fail or are out of range.
pub fn verify_and_extract(
    offset: usize,
    signed_key_images: &[SignedKeyImage],
    output_keys: &[String],
) -> KeyImageImportResult {
    let mut result = KeyImageImportResult {
        updated: 0,
        failed: 0,
    };

    for (i, ski) in signed_key_images.iter().enumerate() {
        let idx = offset + i;
        if idx >= output_keys.len() {
            break;
        }

        let key_hex = &output_keys[idx];
        if key_hex.is_empty() {
            result.failed += 1;
            continue;
        }

        let key_bytes = match hex::decode(key_hex) {
            Ok(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&b);
                arr
            }
            _ => {
                result.failed += 1;
                continue;
            }
        };

        let output_key = match CompressedEdwardsY(key_bytes).decompress() {
            Some(p) => p,
            None => {
                result.failed += 1;
                continue;
            }
        };

        let key_image = match CompressedEdwardsY(ski.key_image).decompress() {
            Some(p) => p,
            None => {
                result.failed += 1;
                continue;
            }
        };

        if verify_key_image_signature(&output_key, &key_image, &ski.signature) {
            result.updated += 1;
        } else {
            result.failed += 1;
        }
    }

    result
}

/// Extract hex key image strings from signed key images (no verification).
/// Use this when you trust the source (e.g. your own full wallet).
pub fn extract_key_image_hex(signed_key_images: &[SignedKeyImage]) -> Vec<String> {
    signed_key_images
        .iter()
        .map(|ski| hex::encode(ski.key_image))
        .collect()
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn parse_rpc_export_wasm() {
        let json = r#"{
            "result": {
                "offset": 2,
                "signed_key_images": [
                    {
                        "key_image": "0000000000000000000000000000000000000000000000000000000000000001",
                        "signature": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
                    }
                ]
            }
        }"#;
        let (offset, skis) = parse_rpc_export(json).unwrap();
        assert_eq!(offset, 2);
        assert_eq!(skis.len(), 1);
        assert_eq!(skis[0].key_image[31], 0x01);
    }

    #[wasm_bindgen_test]
    fn extract_key_image_hex_wasm() {
        let ski = SignedKeyImage {
            key_image: [0xab; 32],
            signature: [0u8; 64],
        };
        let hexes = extract_key_image_hex(&[ski]);
        assert_eq!(hexes.len(), 1);
        assert!(hexes[0].starts_with("abab"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rpc_export() {
        let json = r#"{
            "result": {
                "offset": 0,
                "signed_key_images": [
                    {
                        "key_image": "0000000000000000000000000000000000000000000000000000000000000001",
                        "signature": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
                    }
                ]
            }
        }"#;
        let (offset, skis) = parse_rpc_export(json).unwrap();
        assert_eq!(offset, 0);
        assert_eq!(skis.len(), 1);
        assert_eq!(skis[0].key_image[31], 0x01);
    }

    #[test]
    fn test_parse_rpc_export_bare_result() {
        let json = r#"{
            "offset": 2,
            "signed_key_images": [
                {
                    "key_image": "0000000000000000000000000000000000000000000000000000000000000001",
                    "signature": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
                }
            ]
        }"#;
        let (offset, skis) = parse_rpc_export(json).unwrap();
        assert_eq!(offset, 2);
        assert_eq!(skis.len(), 1);
        assert_eq!(skis[0].key_image[31], 0x01);
    }

    #[test]
    fn test_parse_rpc_export_bad_json() {
        assert!(parse_rpc_export("not json").is_err());
    }

    #[test]
    fn test_parse_rpc_export_bad_hex() {
        let json =
            r#"{"result":{"offset":0,"signed_key_images":[{"key_image":"zz","signature":"00"}]}}"#;
        assert!(parse_rpc_export(json).is_err());
    }

    /// Reject a key image with torsion even if its signature verifies.
    ///
    /// For `I' = x*Hp(P) + T` with order-8 `T`, the verifier computes
    /// `R' = r*Hp + c*I' = k*Hp + c*T`. The extra term vanishes when `8 | c`.
    /// Trying nonces finds such a challenge in about eight attempts, so signature
    /// verification alone does not enforce one key image per output.
    #[test]
    fn test_verify_rejects_a_torsioned_key_image_whose_signature_verifies() {
        // Use an order-8 torsion point.
        let torsion = CompressedEdwardsY(
            hex::decode("c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a")
                .unwrap()
                .try_into()
                .unwrap(),
        )
        .decompress()
        .expect("small-order point must decompress");
        assert!(!torsion.is_identity());
        assert!(!torsion.is_torsion_free(), "fixture must carry torsion");

        let x = Scalar::from(1234567u64);
        let output_key = &x * ED25519_BASEPOINT_TABLE;
        let hp = hash_to_point(output_key);
        let forged = (x * hp) + torsion;
        assert!(
            !forged.is_torsion_free(),
            "the forged key image must be torsioned"
        );

        // Try nonces until `8 | c`, making `c*T` the identity.
        let mut signature = [0u8; 64];
        let mut found = false;
        for nonce in 1u64..256 {
            let k = Scalar::from(nonce);
            let l_point = &k * ED25519_BASEPOINT_TABLE;
            let r_point = k * hp;

            let mut hasher = Keccak256::new();
            hasher.update(forged.compress().to_bytes());
            hasher.update(l_point.compress().to_bytes());
            hasher.update(r_point.compress().to_bytes());
            let c = Scalar::from_bytes_mod_order(hasher.finalize().into());

            // The low three bits of the little-endian scalar give `c mod 8`.
            if (c.to_bytes()[0] & 7) != 0 {
                continue;
            }
            let r = k - (c * x);
            signature[..32].copy_from_slice(&c.to_bytes());
            signature[32..].copy_from_slice(&r.to_bytes());
            found = true;
            break;
        }
        assert!(
            found,
            "a divisible-by-8 challenge should turn up within 256 nonces"
        );

        assert!(
            !verify_key_image_signature(&output_key, &forged, &signature),
            "a key image outside the prime-order subgroup must be rejected"
        );
    }

    #[test]
    fn test_verify_rejects_identity_key_image() {
        let g = &Scalar::ONE * ED25519_BASEPOINT_TABLE;
        let identity = EdwardsPoint::default();
        let sig = [0u8; 64];
        assert!(!verify_key_image_signature(&g, &identity, &sig));
    }
}
