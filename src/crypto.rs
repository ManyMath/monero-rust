//! Cryptographic utilities for wallet encryption and key derivation.
//!
//! This module provides encryption and decryption functions using AES-256-GCM
//! with Argon2id key derivation from passwords. All functions are designed
//! for secure wallet state persistence.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Algorithm, Argon2, ParamsBuilder, Version,
};
use zeroize::Zeroizing;

/// Size of AES-256 key in bytes
const KEY_SIZE: usize = 32;

/// Size of AES-GCM nonce in bytes
pub const NONCE_SIZE: usize = 12;

/// Size of salt for Argon2id in bytes
pub const SALT_SIZE: usize = 32;

/// Derives an encryption key from a password using Argon2id.
///
/// Uses the same parameters as password hashing for consistency:
/// - Algorithm: Argon2id
/// - Memory: 64 MB
/// - Iterations: 3
/// - Parallelism: 4
/// - Output: 32 bytes (256 bits for AES-256)
///
/// # Arguments
/// * `password` - The password to derive a key from
/// * `salt` - 32-byte salt for key derivation
///
/// # Returns
/// A `Zeroizing` wrapper containing the 32-byte encryption key.
/// The key is automatically zeroed when dropped for security.
///
/// # Errors
/// Returns an error string if Argon2id key derivation fails.
pub fn derive_encryption_key(password: &str, salt: &[u8; SALT_SIZE]) -> Result<Zeroizing<[u8; KEY_SIZE]>, String> {
    // Configure Argon2id with recommended parameters
    // Note: ParamsBuilder methods panic on invalid values, but our values are valid constants
    let params = ParamsBuilder::new()
        .m_cost(65536) // 64 MB
        .t_cost(3) // 3 iterations
        .p_cost(4) // 4 threads
        .output_len(KEY_SIZE) // 32 bytes
        .build()
        .map_err(|e| format!("Failed to build Argon2 parameters: {}", e))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    // Convert salt bytes to SaltString format
    let salt_string = SaltString::encode_b64(salt)
        .map_err(|e| format!("Failed to encode salt: {}", e))?;

    // Derive the key
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt_string)
        .map_err(|e| format!("Failed to derive key: {}", e))?;

    // Extract the derived key bytes
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

    // Copy to Zeroizing wrapper for secure memory handling
    let mut key = Zeroizing::new([0u8; KEY_SIZE]);
    key.copy_from_slice(hash_bytes.as_bytes());

    Ok(key)
}

/// Generates a cryptographically secure random nonce for AES-GCM.
///
/// # Returns
/// A 12-byte nonce suitable for use with AES-256-GCM.
pub fn generate_nonce() -> [u8; NONCE_SIZE] {
    use rand_core::RngCore;
    let mut nonce = [0u8; NONCE_SIZE];
    rand_core::OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Encrypts data using AES-256-GCM with a password-derived key.
///
/// # Arguments
/// * `data` - The plaintext data to encrypt
/// * `password` - Password to derive the encryption key from
/// * `salt` - 32-byte salt for key derivation
/// * `nonce` - 12-byte nonce for AES-GCM (must be unique per encryption)
///
/// # Returns
/// The encrypted ciphertext with authentication tag appended.
/// The ciphertext is `data.len() + 16` bytes (16-byte auth tag).
///
/// # Errors
/// Returns an error if key derivation or encryption fails.
///
/// # Security Notes
/// - Never reuse the same (key, nonce) pair
/// - The nonce must be unique for each encryption with the same key
/// - The authentication tag provides integrity protection
pub fn encrypt_wallet_data(
    data: &[u8],
    password: &str,
    salt: &[u8; SALT_SIZE],
    nonce: &[u8; NONCE_SIZE],
) -> Result<Vec<u8>, String> {
    // Derive encryption key from password
    let key = derive_encryption_key(password, salt)?;

    // Create AES-256-GCM cipher
    let cipher = Aes256Gcm::new_from_slice(&*key)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;

    // Create nonce for encryption
    let nonce_obj = Nonce::from_slice(nonce);

    // Encrypt the data (includes authentication tag)
    let ciphertext = cipher
        .encrypt(nonce_obj, data)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    Ok(ciphertext)
}

/// Decrypts data using AES-256-GCM with a password-derived key.
///
/// # Arguments
/// * `ciphertext` - The encrypted data with authentication tag appended
/// * `password` - Password to derive the decryption key from
/// * `salt` - 32-byte salt for key derivation (must match encryption)
/// * `nonce` - 12-byte nonce used during encryption
///
/// # Returns
/// The decrypted plaintext data.
///
/// # Errors
/// Returns an error if:
/// - Key derivation fails
/// - Decryption fails (wrong key, corrupted data, or tampered ciphertext)
/// - Authentication tag verification fails
///
/// # Security Notes
/// - Authentication tag verification ensures data integrity
/// - Tampering with the ciphertext will cause decryption to fail
/// - Wrong password will cause decryption to fail
pub fn decrypt_wallet_data(
    ciphertext: &[u8],
    password: &str,
    salt: &[u8; SALT_SIZE],
    nonce: &[u8; NONCE_SIZE],
) -> Result<Vec<u8>, String> {
    // Derive decryption key from password
    let key = derive_encryption_key(password, salt)?;

    // Create AES-256-GCM cipher
    let cipher = Aes256Gcm::new_from_slice(&*key)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;

    // Create nonce for decryption
    let nonce_obj = Nonce::from_slice(nonce);

    // Decrypt and verify authentication tag
    let plaintext = cipher
        .decrypt(nonce_obj, ciphertext)
        .map_err(|_| "Decryption failed: invalid password or corrupted data".to_string())?;

    Ok(plaintext)
}
