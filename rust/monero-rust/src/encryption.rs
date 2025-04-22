use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2, Version,
};
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use zeroize::Zeroize;

const NONCE_SIZE: usize = 12; // 96 bits for ChaCha20Poly1305

#[derive(Debug)]
pub enum EncryptionError {
    InvalidPassword,
    EncryptionFailed,
    DecryptionFailed,
    InvalidData,
    KeyDerivationFailed,
}

impl std::fmt::Display for EncryptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncryptionError::InvalidPassword => write!(f, "Invalid password"),
            EncryptionError::EncryptionFailed => write!(f, "Encryption failed"),
            EncryptionError::DecryptionFailed => write!(f, "Decryption failed"),
            EncryptionError::InvalidData => write!(f, "Invalid encrypted data format"),
            EncryptionError::KeyDerivationFailed => write!(f, "Key derivation failed"),
        }
    }
}

impl std::error::Error for EncryptionError {}

/// Derive a 256-bit encryption key from a password using Argon2id
pub fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], EncryptionError> {
    let params = argon2::Params::new(
        65536, // m_cost: 64 MiB memory
        3,     // t_cost: 3 iterations
        1,     // p_cost: 1 thread (WASM is single-threaded)
        Some(32),
    )
    .map_err(|_| EncryptionError::KeyDerivationFailed)?;

    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);

    let salt_string =
        SaltString::encode_b64(salt).map_err(|_| EncryptionError::KeyDerivationFailed)?;

    let hash = argon2
        .hash_password(password.as_bytes(), &salt_string)
        .map_err(|_| EncryptionError::KeyDerivationFailed)?;

    let hash_bytes = hash.hash.ok_or(EncryptionError::KeyDerivationFailed)?;
    let hash_slice = hash_bytes.as_bytes();

    let mut key = [0u8; 32];
    key.copy_from_slice(&hash_slice[..32]);
    Ok(key)
}

/// Encrypt plaintext using ChaCha20-Poly1305.
/// Returns: salt (16 bytes) || nonce (12 bytes) || ciphertext || tag (16 bytes)
pub fn encrypt(plaintext: &[u8], password: &str) -> Result<Vec<u8>, EncryptionError> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);

    let mut key = derive_key(password, &salt)?;

    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    key.zeroize();

    let mut result = Vec::with_capacity(16 + NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt ciphertext using ChaCha20-Poly1305.
/// Input format: salt (16 bytes) || nonce (12 bytes) || ciphertext || tag (16 bytes)
pub fn decrypt(encrypted_data: &[u8], password: &str) -> Result<Vec<u8>, EncryptionError> {
    if encrypted_data.len() < 44 {
        return Err(EncryptionError::InvalidData);
    }

    let (salt, rest) = encrypted_data.split_at(16);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_SIZE);

    let mut key = derive_key(password, salt)?;

    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| EncryptionError::DecryptionFailed)?;

    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| EncryptionError::DecryptionFailed)?;

    key.zeroize();

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "test_password_123";
        let plaintext = b"Hello, World! This is a test message.";

        let encrypted = encrypt(plaintext, password).expect("Encryption failed");
        let decrypted = decrypt(&encrypted, password).expect("Decryption failed");

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_wrong_password() {
        let password = "correct_password";
        let wrong_password = "wrong_password";
        let plaintext = b"Secret message";

        let encrypted = encrypt(plaintext, password).expect("Encryption failed");
        let result = decrypt(&encrypted, wrong_password);

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_data_too_short() {
        let password = "test_password";
        let invalid_data = b"too short";

        let result = decrypt(invalid_data, password);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_plaintext() {
        let password = "test_password";
        let plaintext = b"";

        let encrypted = encrypt(plaintext, password).expect("Encryption failed");
        let decrypted = decrypt(&encrypted, password).expect("Decryption failed");

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_large_plaintext() {
        let password = "test_password";
        let plaintext = vec![0xABu8; 100_000];

        let encrypted = encrypt(&plaintext, password).expect("Encryption failed");
        let decrypted = decrypt(&encrypted, password).expect("Decryption failed");

        assert_eq!(plaintext, decrypted);
    }
}
