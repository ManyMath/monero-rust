//! Wallet encryption and key derivation.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Algorithm, Argon2, ParamsBuilder, Version,
};
use zeroize::Zeroizing;

const KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const SALT_SIZE: usize = 32;

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
