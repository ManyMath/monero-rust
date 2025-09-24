//! Monero v3 key image export: per-key-image ring signatures and ChaCha20 view-key encryption.
//!
//! Implements the exact binary format expected by Feather Wallet and monero-wallet-cli.
//! The format is: magic bytes (plaintext) + ChaCha20-encrypted payload containing
//! per-key-image ring signatures.
//!
//! Reference: Monero `src/wallet/wallet2.cpp` lines 13190-13325 and
//! `src/crypto/crypto.cpp` lines 645-709.

use chacha20::ChaCha20Legacy;
use cipher::{KeyIvInit, StreamCipher};
use cuprate_cryptonight::cryptonight_hash_v0;
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE,
    edwards::EdwardsPoint,
    scalar::Scalar,
    traits::VartimeMultiscalarMul,
};
use getrandom::getrandom;
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use crate::epee_compat::KEY_IMAGES_MAGIC;
use crate::wallet_output::WalletOutput;

/// A single key image entry for v3 export.
#[derive(Debug, Clone)]
pub struct KeyImageExportEntry {
    /// 32-byte key image
    pub key_image: [u8; 32],
    /// One-time output public key
    pub pub_key: EdwardsPoint,
    /// Ephemeral key offset scalar (spend_scalar + key_offset = ephemeral secret)
    pub key_offset: Scalar,
}

/// 1-of-1 ring signature matching Monero's `generate_ring_signature` (crypto.cpp).
///
/// Returns `(c, r)` each as 32 bytes.
pub fn generate_key_image_ring_signature(
    key_image: &[u8; 32],
    pub_key: &EdwardsPoint,
    sec_key: &Scalar,
) -> ([u8; 32], [u8; 32]) {
    // Random scalar k
    let mut k_bytes = [0u8; 32];
    getrandom(&mut k_bytes).expect("RNG failed");
    let k = Zeroizing::new(Scalar::from_bytes_mod_order(k_bytes));

    // a = k * G
    let a = (&*k * &ED25519_BASEPOINT_TABLE).compress().to_bytes();

    // Hp = hash_to_point(pub_key)
    let hp = monero_serai::ringct::hash_to_point(*pub_key);

    // b = k * Hp
    let b = (&*k * hp).compress().to_bytes();

    // Build rs_comm buffer: [prefix_hash: 32 || a: 32 || b: 32]
    let mut buf = [0u8; 96];
    buf[..32].copy_from_slice(key_image); // prefix_hash = key_image
    buf[32..64].copy_from_slice(&a);
    buf[64..96].copy_from_slice(&b);

    // h = hash_to_scalar(buf) = Keccak256 + reduce mod l
    let hash: [u8; 32] = Keccak256::digest(&buf).into();
    let h = Scalar::from_bytes_mod_order(hash);

    // For ring size 1: c = h
    let c = h;

    // r = k - c * sec_key
    let r = &*k - (c * sec_key);

    (c.to_bytes(), r.to_bytes())
}

/// Verifies a 1-of-1 ring signature produced by `generate_key_image_ring_signature`.
pub fn verify_key_image_ring_signature(
    key_image: &[u8; 32],
    pub_key: &EdwardsPoint,
    key_image_point: &EdwardsPoint,
    c: &[u8; 32],
    r: &[u8; 32],
) -> bool {
    let c_scalar = Scalar::from_bytes_mod_order(*c);
    let r_scalar = Scalar::from_bytes_mod_order(*r);

    // a' = r * G + c * pub_key
    let a_prime = EdwardsPoint::vartime_double_scalar_mul_basepoint(&c_scalar, pub_key, &r_scalar);

    // Hp = hash_to_point(pub_key)
    let hp = monero_serai::ringct::hash_to_point(*pub_key);

    // b' = r * Hp + c * key_image_point
    let b_prime =
        EdwardsPoint::vartime_multiscalar_mul(&[r_scalar, c_scalar], &[hp, *key_image_point]);

    // Build buffer: [key_image: 32 || a': 32 || b': 32]
    let mut buf = [0u8; 96];
    buf[..32].copy_from_slice(key_image);
    buf[32..64].copy_from_slice(&a_prime.compress().to_bytes());
    buf[64..96].copy_from_slice(&b_prime.compress().to_bytes());

    // h' = hash_to_scalar(buf)
    let hash: [u8; 32] = Keccak256::digest(&buf).into();
    let h_prime = Scalar::from_bytes_mod_order(hash);

    h_prime == c_scalar
}

/// Schnorr signature (Monero's `generate_signature`) over `msg_hash`.
fn generate_schnorr_signature(
    msg_hash: &[u8; 32],
    pub_key_bytes: &[u8; 32],
    sec_key: &Scalar,
) -> ([u8; 32], [u8; 32]) {
    let mut k_bytes = [0u8; 32];
    getrandom(&mut k_bytes).expect("RNG failed");
    let k = Zeroizing::new(Scalar::from_bytes_mod_order(k_bytes));

    let tmp = (&*k * &ED25519_BASEPOINT_TABLE).compress().to_bytes();

    // Build buffer: [msg_hash || pub_key || tmp]
    let mut buf = [0u8; 96];
    buf[..32].copy_from_slice(msg_hash);
    buf[32..64].copy_from_slice(pub_key_bytes);
    buf[64..96].copy_from_slice(&tmp);

    let hash: [u8; 32] = Keccak256::digest(&buf).into();
    let c = Scalar::from_bytes_mod_order(hash);

    let r = &*k - (c * sec_key);

    (c.to_bytes(), r.to_bytes())
}

/// Verify a standard Schnorr signature (Monero's `check_signature`).
fn verify_schnorr_signature(
    msg_hash: &[u8; 32],
    pub_key: &EdwardsPoint,
    pub_key_bytes: &[u8; 32],
    c: &[u8; 32],
    r: &[u8; 32],
) -> bool {
    let c_scalar = Scalar::from_bytes_mod_order(*c);
    let r_scalar = Scalar::from_bytes_mod_order(*r);

    // tmp' = r * G + c * pub_key
    let tmp_prime = EdwardsPoint::vartime_double_scalar_mul_basepoint(&c_scalar, pub_key, &r_scalar);

    let mut buf = [0u8; 96];
    buf[..32].copy_from_slice(msg_hash);
    buf[32..64].copy_from_slice(pub_key_bytes);
    buf[64..96].copy_from_slice(&tmp_prime.compress().to_bytes());

    let hash: [u8; 32] = Keccak256::digest(&buf).into();
    let c_prime = Scalar::from_bytes_mod_order(hash);

    c_prime == c_scalar
}

/// Encrypts data with ChaCha20-legacy and appends an authentication signature.
///
/// Matches `wallet2::encrypt` with `authenticated=true`.
/// Output: `[iv: 8][ciphertext][sig_c: 32][sig_r: 32]`
pub fn encrypt_with_view_key(plaintext: &[u8], view_secret_key: &[u8; 32]) -> Vec<u8> {
    // Derive ChaCha key via CryptoNight
    let chacha_key: [u8; 32] = cryptonight_hash_v0(view_secret_key);

    // Random 8-byte IV
    let mut iv = [0u8; 8];
    getrandom(&mut iv).expect("RNG failed");

    // Encrypt
    let mut ciphertext = plaintext.to_vec();
    let mut cipher = ChaCha20Legacy::new((&chacha_key).into(), (&iv).into());
    cipher.apply_keystream(&mut ciphertext);

    // Assemble: [iv: 8][ciphertext]
    let mut iv_and_ciphertext = Vec::with_capacity(8 + ciphertext.len());
    iv_and_ciphertext.extend_from_slice(&iv);
    iv_and_ciphertext.extend_from_slice(&ciphertext);

    // Authentication signature
    // hash = Keccak256(iv || ciphertext)
    let hash: [u8; 32] = Keccak256::digest(&iv_and_ciphertext).into();

    // Ed25519 Schnorr signature over hash using view key
    let view_scalar = Scalar::from_bytes_mod_order(*view_secret_key);
    let view_pub = (&view_scalar * &ED25519_BASEPOINT_TABLE).compress().to_bytes();
    let (sig_c, sig_r) = generate_schnorr_signature(&hash, &view_pub, &view_scalar);

    // Final result: [iv: 8][ciphertext][sig_c: 32][sig_r: 32]
    let mut result = Vec::with_capacity(iv_and_ciphertext.len() + 64);
    result.extend_from_slice(&iv_and_ciphertext);
    result.extend_from_slice(&sig_c);
    result.extend_from_slice(&sig_r);

    result
}

/// Decrypts data produced by `encrypt_with_view_key`, verifying the auth signature.
pub fn decrypt_with_view_key(encrypted: &[u8], view_secret_key: &[u8; 32]) -> Result<Vec<u8>, String> {
    // Minimum length: 8-byte IV + 64-byte auth signature
    if encrypted.len() < 72 {
        return Err("Encrypted data too short (need at least IV + auth signature)".to_string());
    }

    // Split: [iv: 8][ciphertext: ...][sig_c: 32][sig_r: 32]
    let iv = &encrypted[..8];
    let sig_start = encrypted.len() - 64;
    let ciphertext_data = &encrypted[8..sig_start];
    let sig_c: [u8; 32] = encrypted[sig_start..sig_start + 32]
        .try_into()
        .map_err(|_| "sig_c slice error")?;
    let sig_r: [u8; 32] = encrypted[sig_start + 32..sig_start + 64]
        .try_into()
        .map_err(|_| "sig_r slice error")?;

    // Verify auth signature
    let iv_and_ciphertext = &encrypted[..sig_start];
    let hash: [u8; 32] = Keccak256::digest(iv_and_ciphertext).into();
    let view_scalar = Scalar::from_bytes_mod_order(*view_secret_key);
    let view_pub_point = &view_scalar * &ED25519_BASEPOINT_TABLE;
    let view_pub_bytes = view_pub_point.compress().to_bytes();

    if !verify_schnorr_signature(&hash, &view_pub_point, &view_pub_bytes, &sig_c, &sig_r) {
        return Err("Authentication signature verification failed".to_string());
    }

    // Decrypt
    let chacha_key: [u8; 32] = cryptonight_hash_v0(view_secret_key);
    let mut iv_arr = [0u8; 8];
    iv_arr.copy_from_slice(iv);
    let mut plaintext = ciphertext_data.to_vec();
    let mut cipher = ChaCha20Legacy::new((&chacha_key).into(), (&iv_arr).into());
    cipher.apply_keystream(&mut plaintext);

    Ok(plaintext)
}

/// Exports key images in Monero's v3 binary format.
///
/// Output: `[magic: 24][enc([offset: 4 LE][pub_spend: 32][pub_view: 32][N * (ki:32 + c:32 + r:32)])]`
pub fn export_key_images_v3(
    key_images: &[KeyImageExportEntry],
    spend_scalar: &Scalar,
    pub_spend_key: &[u8; 32],
    pub_view_key: &[u8; 32],
    view_secret_key: &[u8; 32],
    offset: u32,
) -> Result<Vec<u8>, String> {
    // Build plaintext payload
    let payload_size = 4 + 32 + 32 + key_images.len() * 96;
    let mut payload = Vec::with_capacity(payload_size);

    // 4-byte LE offset
    payload.extend_from_slice(&offset.to_le_bytes());

    // Public keys
    payload.extend_from_slice(pub_spend_key);
    payload.extend_from_slice(pub_view_key);

    // Per-key-image: key_image + ring_signature
    for entry in key_images {
        let ephemeral_sec = Zeroizing::new(spend_scalar + &entry.key_offset);
        let (c, r) = generate_key_image_ring_signature(&entry.key_image, &entry.pub_key, &ephemeral_sec);

        payload.extend_from_slice(&entry.key_image);
        payload.extend_from_slice(&c);
        payload.extend_from_slice(&r);
    }

    // Encrypt with view key
    let ciphertext = encrypt_with_view_key(&payload, view_secret_key);

    // Prepend magic (plaintext)
    let mut result = Vec::with_capacity(KEY_IMAGES_MAGIC.len() + ciphertext.len());
    result.extend_from_slice(KEY_IMAGES_MAGIC);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Imports key images from Monero's v3 binary format.
///
/// Returns `(key_images, pub_spend_key, pub_view_key)`. Ring signatures are not verified.
pub fn import_key_images_v3(
    data: &[u8],
    view_secret_key: &[u8; 32],
) -> Result<(Vec<[u8; 32]>, [u8; 32], [u8; 32]), String> {
    // Strip magic bytes
    let payload = crate::epee_compat::strip_magic(KEY_IMAGES_MAGIC, data)?;

    // Decrypt
    let plaintext = decrypt_with_view_key(payload, view_secret_key)?;

    // Parse header: offset (4) + pub_spend (32) + pub_view (32) = 68 bytes minimum
    if plaintext.len() < 68 {
        return Err("Decrypted payload too short for header".to_string());
    }

    // offset (skip, we don't use it currently)
    let _offset = u32::from_le_bytes(
        plaintext[..4]
            .try_into()
            .map_err(|_| "offset parse error")?,
    );

    let mut pub_spend = [0u8; 32];
    pub_spend.copy_from_slice(&plaintext[4..36]);

    let mut pub_view = [0u8; 32];
    pub_view.copy_from_slice(&plaintext[36..68]);

    // Remaining bytes: N * 96 (32 ki + 64 sig per entry)
    let records_data = &plaintext[68..];
    if records_data.len() % 96 != 0 {
        return Err(format!(
            "Key image record section has invalid length {} (must be divisible by 96)",
            records_data.len()
        ));
    }

    let n = records_data.len() / 96;
    let mut key_images = Vec::with_capacity(n);

    for i in 0..n {
        let base = i * 96;
        let mut ki = [0u8; 32];
        ki.copy_from_slice(&records_data[base..base + 32]);
        // Skip signature bytes (base+32..base+96) -- not verified on import
        key_images.push(ki);
    }

    Ok((key_images, pub_spend, pub_view))
}

/// Derives keys from `seed_phrase`/`passphrase` and exports wallet outputs as v3 key images.
pub fn export_key_images_from_outputs(
    seed_phrase: &str,
    passphrase: &str,
    sorted_outputs: &[WalletOutput],
) -> Result<Vec<u8>, String> {
    use curve25519_dalek::edwards::CompressedEdwardsY;

    // Resolve seed
    let seed = crate::scanner::resolve_seed(seed_phrase)?;

    // Derive spend scalar with passphrase (Monero seed offset)
    let key_bytes = seed.key_bytes_with_passphrase(passphrase);
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&key_bytes[..]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);

    // Derive view key from spend scalar
    let pub_spend_key = (&spend_scalar * &ED25519_BASEPOINT_TABLE).compress().to_bytes();
    let view_bytes: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    let view_scalar = Scalar::from_bytes_mod_order(view_bytes);
    let pub_view_key = (&view_scalar * &ED25519_BASEPOINT_TABLE).compress().to_bytes();
    let view_secret_key = view_scalar.to_bytes();

    // Build KeyImageExportEntry list from sorted outputs
    let entries: Result<Vec<KeyImageExportEntry>, String> = sorted_outputs
        .iter()
        .map(|o| {
            let ki_bytes: [u8; 32] = hex::decode(&o.key_image)
                .map_err(|e| format!("bad key_image hex: {e}"))?
                .try_into()
                .map_err(|_| "key_image not 32 bytes".to_string())?;
            let key_bytes_arr: [u8; 32] = hex::decode(&o.key)
                .map_err(|e| format!("bad output key hex: {e}"))?
                .try_into()
                .map_err(|_| "output key not 32 bytes".to_string())?;
            let pub_key = CompressedEdwardsY(key_bytes_arr)
                .decompress()
                .ok_or_else(|| "invalid output public key".to_string())?;
            let offset_bytes: [u8; 32] = hex::decode(&o.key_offset)
                .map_err(|e| format!("bad key_offset hex: {e}"))?
                .try_into()
                .map_err(|_| "key_offset not 32 bytes".to_string())?;
            let key_offset = Scalar::from_bytes_mod_order(offset_bytes);
            Ok(KeyImageExportEntry {
                key_image: ki_bytes,
                pub_key,
                key_offset,
            })
        })
        .collect();

    export_key_images_v3(
        &entries?,
        &spend_scalar,
        &pub_spend_key,
        &pub_view_key,
        &view_secret_key,
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_serai::ringct::generate_key_image;

    /// Create deterministic test keys for reproducible tests.
    fn test_keys() -> (Scalar, Scalar, EdwardsPoint, EdwardsPoint, [u8; 32], [u8; 32]) {
        // Deterministic spend scalar
        let spend_bytes: [u8; 32] = {
            let hash: [u8; 32] = Keccak256::digest(b"test spend key seed").into();
            hash
        };
        let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);

        // View key = Keccak256(spend_scalar) (matches Monero derivation)
        let view_bytes: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
        let view_scalar = Scalar::from_bytes_mod_order(view_bytes);

        // Public keys
        let pub_spend = &spend_scalar * &ED25519_BASEPOINT_TABLE;
        let pub_view = &view_scalar * &ED25519_BASEPOINT_TABLE;

        let pub_spend_bytes = pub_spend.compress().to_bytes();
        let pub_view_bytes = pub_view.compress().to_bytes();

        (spend_scalar, view_scalar, pub_spend, pub_view, pub_spend_bytes, pub_view_bytes)
    }

    /// Create a deterministic key offset and compute the one-time output public key.
    fn test_output_keys(spend_scalar: &Scalar) -> (Scalar, EdwardsPoint, [u8; 32]) {
        // Deterministic key offset
        let offset_bytes: [u8; 32] = {
            let hash: [u8; 32] = Keccak256::digest(b"test key offset").into();
            hash
        };
        let key_offset = Scalar::from_bytes_mod_order(offset_bytes);

        // Ephemeral secret key = spend + offset
        let ephemeral_sec = Zeroizing::new(spend_scalar + &key_offset);

        // One-time output public key = ephemeral_sec * G
        let pub_key = &*ephemeral_sec * &ED25519_BASEPOINT_TABLE;

        // Key image = ephemeral_sec * Hp(pub_key)
        let key_image_point = generate_key_image(&ephemeral_sec);
        let key_image = key_image_point.compress().to_bytes();

        (key_offset, pub_key, key_image)
    }

    #[test]
    fn test_ring_sig_produces_32_byte_c_and_r() {
        let (spend_scalar, _, _, _, _, _) = test_keys();
        let (key_offset, pub_key, key_image) = test_output_keys(&spend_scalar);

        let ephemeral_sec = spend_scalar + &key_offset;
        let (c, r) = generate_key_image_ring_signature(&key_image, &pub_key, &ephemeral_sec);

        assert_eq!(c.len(), 32);
        assert_eq!(r.len(), 32);
    }

    #[test]
    fn test_ring_sig_verify_valid() {
        let (spend_scalar, _, _, _, _, _) = test_keys();
        let (key_offset, pub_key, key_image) = test_output_keys(&spend_scalar);

        let ephemeral_sec = spend_scalar + &key_offset;
        let (c, r) = generate_key_image_ring_signature(&key_image, &pub_key, &ephemeral_sec);

        let ephemeral_sec2 = Zeroizing::new(spend_scalar + &key_offset);
        let key_image_point = generate_key_image(&ephemeral_sec2);

        assert!(
            verify_key_image_ring_signature(&key_image, &pub_key, &key_image_point, &c, &r),
            "Valid ring signature should verify"
        );
    }

    #[test]
    fn test_ring_sig_verify_invalid_c() {
        let (spend_scalar, _, _, _, _, _) = test_keys();
        let (key_offset, pub_key, key_image) = test_output_keys(&spend_scalar);

        let ephemeral_sec = spend_scalar + &key_offset;
        let (mut c, r) = generate_key_image_ring_signature(&key_image, &pub_key, &ephemeral_sec);

        // Modify c
        c[0] ^= 0xFF;

        let ephemeral_sec2 = Zeroizing::new(spend_scalar + &key_offset);
        let key_image_point = generate_key_image(&ephemeral_sec2);

        assert!(
            !verify_key_image_ring_signature(&key_image, &pub_key, &key_image_point, &c, &r),
            "Modified c should fail verification"
        );
    }

    #[test]
    fn test_ring_sig_verify_invalid_r() {
        let (spend_scalar, _, _, _, _, _) = test_keys();
        let (key_offset, pub_key, key_image) = test_output_keys(&spend_scalar);

        let ephemeral_sec = spend_scalar + &key_offset;
        let (c, mut r) = generate_key_image_ring_signature(&key_image, &pub_key, &ephemeral_sec);

        // Modify r
        r[0] ^= 0xFF;

        let ephemeral_sec2 = Zeroizing::new(spend_scalar + &key_offset);
        let key_image_point = generate_key_image(&ephemeral_sec2);

        assert!(
            !verify_key_image_ring_signature(&key_image, &pub_key, &key_image_point, &c, &r),
            "Modified r should fail verification"
        );
    }

    #[test]
    fn test_encrypt_produces_iv_ciphertext_auth_sig() {
        let view_bytes: [u8; 32] = Keccak256::digest(b"test view secret key").into();
        let plaintext = b"hello world test data";

        let encrypted = encrypt_with_view_key(plaintext, &view_bytes);

        // Output = 8-byte IV + ciphertext(=plaintext.len()) + 64-byte auth signature
        assert_eq!(encrypted.len(), 8 + plaintext.len() + 64);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let view_bytes: [u8; 32] = Keccak256::digest(b"test view secret key").into();
        let plaintext = b"roundtrip test data with some longer content for good measure";

        let encrypted = encrypt_with_view_key(plaintext, &view_bytes);
        let decrypted = decrypt_with_view_key(&encrypted, &view_bytes)
            .expect("Decryption should succeed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let view_bytes: [u8; 32] = Keccak256::digest(b"test view secret key").into();
        let wrong_key: [u8; 32] = Keccak256::digest(b"wrong key").into();
        let plaintext = b"secret data";

        let encrypted = encrypt_with_view_key(plaintext, &view_bytes);
        let result = decrypt_with_view_key(&encrypted, &wrong_key);

        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn test_export_v3_starts_with_magic() {
        let (spend_scalar, view_scalar, _, _, pub_spend_bytes, pub_view_bytes) = test_keys();
        let (key_offset, pub_key, key_image) = test_output_keys(&spend_scalar);
        let view_secret_bytes = view_scalar.to_bytes();

        let entries = vec![KeyImageExportEntry {
            key_image,
            pub_key,
            key_offset,
        }];

        let result = export_key_images_v3(
            &entries,
            &spend_scalar,
            &pub_spend_bytes,
            &pub_view_bytes,
            &view_secret_bytes,
            0,
        )
        .expect("Export should succeed");

        assert!(
            result.starts_with(KEY_IMAGES_MAGIC),
            "Export should start with magic bytes"
        );
    }

    #[test]
    fn test_export_v3_payload_structure() {
        let (spend_scalar, view_scalar, _, _, pub_spend_bytes, pub_view_bytes) = test_keys();
        let (key_offset, pub_key, key_image) = test_output_keys(&spend_scalar);
        let view_secret_bytes = view_scalar.to_bytes();

        let entries = vec![KeyImageExportEntry {
            key_image,
            pub_key,
            key_offset,
        }];

        let result = export_key_images_v3(
            &entries,
            &spend_scalar,
            &pub_spend_bytes,
            &pub_view_bytes,
            &view_secret_bytes,
            0,
        )
        .expect("Export should succeed");

        // Strip magic
        let encrypted = &result[KEY_IMAGES_MAGIC.len()..];

        // Decrypt to verify structure
        let plaintext = decrypt_with_view_key(encrypted, &view_secret_bytes)
            .expect("Decryption should succeed");

        // Header: 4-byte offset + 32-byte pub_spend + 32-byte pub_view = 68
        assert!(plaintext.len() >= 68, "Payload too short for header");

        // Check offset
        let offset = u32::from_le_bytes(plaintext[..4].try_into().unwrap());
        assert_eq!(offset, 0);

        // Check public keys
        assert_eq!(&plaintext[4..36], &pub_spend_bytes);
        assert_eq!(&plaintext[36..68], &pub_view_bytes);

        // Records: N * (32 ki + 64 sig) = 96 per entry
        let records = &plaintext[68..];
        assert_eq!(records.len(), 96, "One entry should be 96 bytes");

        // Check key image
        assert_eq!(&records[..32], &key_image);
    }

    #[test]
    fn test_export_import_v3_roundtrip() {
        let (spend_scalar, view_scalar, _, _, pub_spend_bytes, pub_view_bytes) = test_keys();
        let view_secret_bytes = view_scalar.to_bytes();

        // Create multiple entries
        let mut entries = Vec::new();
        for i in 0u8..3 {
            let offset_seed = format!("test key offset {}", i);
            let offset_bytes: [u8; 32] = Keccak256::digest(offset_seed.as_bytes()).into();
            let key_offset = Scalar::from_bytes_mod_order(offset_bytes);
            let ephemeral_sec = Zeroizing::new(&spend_scalar + &key_offset);
            let pub_key = &*ephemeral_sec * &ED25519_BASEPOINT_TABLE;
            let key_image_point = generate_key_image(&ephemeral_sec);
            let key_image = key_image_point.compress().to_bytes();

            entries.push(KeyImageExportEntry {
                key_image,
                pub_key,
                key_offset,
            });
        }

        let exported = export_key_images_v3(
            &entries,
            &spend_scalar,
            &pub_spend_bytes,
            &pub_view_bytes,
            &view_secret_bytes,
            0,
        )
        .expect("Export should succeed");

        let (imported_kis, imported_spend, imported_view) =
            import_key_images_v3(&exported, &view_secret_bytes)
                .expect("Import should succeed");

        assert_eq!(imported_kis.len(), 3);
        assert_eq!(imported_spend, pub_spend_bytes);
        assert_eq!(imported_view, pub_view_bytes);

        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(imported_kis[i], entry.key_image, "Key image {} mismatch", i);
        }
    }
}
