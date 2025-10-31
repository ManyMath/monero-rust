//! Wallet encryption and key derivation.
//!
//! CryptoNight implementation from Cuprate (https://github.com/Cuprate/cuprate)
//! Copyright (c) 2023-2024 Cuprate Contributors, MIT License

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Algorithm, Argon2, ParamsBuilder, Version,
};
use chacha20::{
    cipher::{KeyIvInit, StreamCipher},
    ChaCha20Legacy,
};
use zeroize::Zeroizing;

const KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const SALT_SIZE: usize = 32;

pub const CHACHA_KEY_SIZE: usize = 32;
pub const CHACHA_IV_SIZE: usize = 8;

/// Derives an encryption key from a password using Argon2id.
pub fn derive_encryption_key(password: &str, salt: &[u8; SALT_SIZE]) -> Result<Zeroizing<[u8; KEY_SIZE]>, String> {
    let params = ParamsBuilder::new()
        .m_cost(65536)
        .t_cost(3)
        .p_cost(4)
        .output_len(KEY_SIZE)
        .build()
        .map_err(|e| format!("Failed to build Argon2 parameters: {}", e))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let salt_string = SaltString::encode_b64(salt)
        .map_err(|e| format!("Failed to encode salt: {}", e))?;

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt_string)
        .map_err(|e| format!("Failed to derive key: {}", e))?;

    let hash_bytes = password_hash
        .hash
        .ok_or_else(|| "Key derivation produced no output".to_string())?;

    if hash_bytes.len() != KEY_SIZE {
        return Err(format!(
            "Unexpected key length: expected {}, got {}",
            KEY_SIZE,
            hash_bytes.len()
        ));
    }

    let mut key = Zeroizing::new([0u8; KEY_SIZE]);
    key.copy_from_slice(hash_bytes.as_bytes());

    Ok(key)
}

/// Generates a cryptographically secure random nonce for AES-GCM.
pub fn generate_nonce() -> [u8; NONCE_SIZE] {
    use rand_core::RngCore;
    let mut nonce = [0u8; NONCE_SIZE];
    rand_core::OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Encrypts data using AES-256-GCM with a password-derived key.
pub fn encrypt_wallet_data(
    data: &[u8],
    password: &str,
    salt: &[u8; SALT_SIZE],
    nonce: &[u8; NONCE_SIZE],
) -> Result<Vec<u8>, String> {
    let key = derive_encryption_key(password, salt)?;

    let cipher = Aes256Gcm::new_from_slice(&*key)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;
    let nonce_obj = Nonce::from(*nonce);
    let ciphertext = cipher
        .encrypt(&nonce_obj, data)
        .map_err(|e| format!("Encryption failed: {}", e))?;
    Ok(ciphertext)
}

pub fn decrypt_wallet_data(
    ciphertext: &[u8],
    password: &str,
    salt: &[u8; SALT_SIZE],
    nonce: &[u8; NONCE_SIZE],
) -> Result<Vec<u8>, String> {
    let key = derive_encryption_key(password, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&*key)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;
    let nonce_obj = Nonce::from(*nonce);
    let plaintext = cipher
        .decrypt(&nonce_obj, ciphertext)
        .map_err(|_| "Decryption failed: invalid password or corrupted data".to_string())?;
    Ok(plaintext)
}

/// Derives ChaCha20 key using CryptoNight (Monero-compatible).
/// Uses cn_slow_hash for key derivation matching monero-wallet-cli.
pub fn derive_chacha_key_cryptonight(secret_key: &[u8; 32], kdf_rounds: u64) -> [u8; CHACHA_KEY_SIZE] {
    let mut hash = cuprate_cryptonight::cryptonight_hash_v0(secret_key);
    for _ in 1..kdf_rounds {
        hash = cuprate_cryptonight::cryptonight_hash_v0(&hash);
    }
    hash
}

pub fn generate_chacha_iv() -> [u8; CHACHA_IV_SIZE] {
    use rand_core::RngCore;
    let mut iv = [0u8; CHACHA_IV_SIZE];
    rand_core::OsRng.fill_bytes(&mut iv);
    iv
}

/// ChaCha20 encryption using the DJB variant (8-byte nonce) for Monero compatibility.
pub fn chacha20_encrypt(data: &[u8], key: &[u8; CHACHA_KEY_SIZE], iv: &[u8; CHACHA_IV_SIZE]) -> Vec<u8> {
    let mut cipher = ChaCha20Legacy::new(key.into(), iv.into());
    let mut buffer = data.to_vec();
    cipher.apply_keystream(&mut buffer);
    buffer
}

pub fn chacha20_decrypt(ciphertext: &[u8], key: &[u8; CHACHA_KEY_SIZE], iv: &[u8; CHACHA_IV_SIZE]) -> Vec<u8> {
    chacha20_encrypt(ciphertext, key, iv)
}

fn keccak256_to_scalar(data: &[u8]) -> curve25519_dalek::scalar::Scalar {
    use sha3::{Digest, Keccak256};
    let hash: [u8; 32] = Keccak256::digest(data).into();
    curve25519_dalek::scalar::Scalar::from_bytes_mod_order(hash)
}

/// Generates a key image signature for export (single-element ring signature).
pub fn generate_key_image_signature(
    ephemeral_secret_key: &curve25519_dalek::scalar::Scalar,
    output_public_key: &curve25519_dalek::edwards::EdwardsPoint,
    key_image: &curve25519_dalek::edwards::EdwardsPoint,
) -> [u8; 64] {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    use monero_generators::biased_hash_to_point;
    use rand_core::RngCore;

    let mut k_bytes = [0u8; 64];
    rand_core::OsRng.fill_bytes(&mut k_bytes);
    let k = curve25519_dalek::scalar::Scalar::from_bytes_mod_order_wide(&k_bytes);

    let l_point = &k * ED25519_BASEPOINT_TABLE;
    let hp = biased_hash_to_point(output_public_key.compress().to_bytes());
    let r_point = k * hp;

    let mut buf = Vec::with_capacity(96);
    buf.extend_from_slice(&key_image.compress().to_bytes());
    buf.extend_from_slice(&l_point.compress().to_bytes());
    buf.extend_from_slice(&r_point.compress().to_bytes());

    let c = keccak256_to_scalar(&buf);
    let r = k - (c * ephemeral_secret_key);

    let mut signature = [0u8; 64];
    signature[..32].copy_from_slice(&c.to_bytes());
    signature[32..].copy_from_slice(&r.to_bytes());
    signature
}

/// Verifies a key image signature.
pub fn verify_key_image_signature(
    signature: &[u8; 64],
    output_public_key: &curve25519_dalek::edwards::EdwardsPoint,
    key_image: &curve25519_dalek::edwards::EdwardsPoint,
) -> bool {
    use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
    use curve25519_dalek::scalar::Scalar;
    use curve25519_dalek::traits::VartimeMultiscalarMul;
    use monero_generators::biased_hash_to_point;

    let c_bytes: [u8; 32] = signature[..32].try_into().unwrap();
    let r_bytes: [u8; 32] = signature[32..].try_into().unwrap();

    let c = match Scalar::from_canonical_bytes(c_bytes).into_option() {
        Some(s) => s,
        None => return false,
    };
    let r = match Scalar::from_canonical_bytes(r_bytes).into_option() {
        Some(s) => s,
        None => return false,
    };

    let l_point = curve25519_dalek::edwards::EdwardsPoint::vartime_multiscalar_mul(
        [r, c],
        [ED25519_BASEPOINT_POINT, *output_public_key],
    );

    let hp = biased_hash_to_point(output_public_key.compress().to_bytes());
    let r_point = curve25519_dalek::edwards::EdwardsPoint::vartime_multiscalar_mul(
        [r, c],
        [hp, *key_image],
    );

    let mut buf = Vec::with_capacity(96);
    buf.extend_from_slice(&key_image.compress().to_bytes());
    buf.extend_from_slice(&l_point.compress().to_bytes());
    buf.extend_from_slice(&r_point.compress().to_bytes());

    keccak256_to_scalar(&buf) == c
}
