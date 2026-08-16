use argon2::{password_hash::SaltString, Argon2, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use zeroize::Zeroizing;

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

/// Derive a 256-bit key with Argon2id and clear it on drop.
///
/// Write directly into a `Zeroizing` buffer. The `hash_password` API returns an
/// immutable `password_hash::Output` that cannot be cleared after use.
/// Compatibility tests check that both APIs derive the same key, since changing
/// the result would prevent existing wallets from being decrypted.
pub fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, EncryptionError> {
    let params = argon2::Params::new(
        65536, // m_cost: 64 MiB memory
        3,     // t_cost: 3 iterations
        1,     // p_cost: 1 thread (WASM is single-threaded)
        Some(32),
    )
    .map_err(|_| EncryptionError::KeyDerivationFailed)?;

    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);

    // Keep the existing salt validation through `SaltString`.
    let salt_string =
        SaltString::encode_b64(salt).map_err(|_| EncryptionError::KeyDerivationFailed)?;
    let mut salt_bytes = [0u8; 64];
    let salt_bytes = salt_string
        .decode_b64(&mut salt_bytes)
        .map_err(|_| EncryptionError::KeyDerivationFailed)?;

    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt_bytes, &mut *key)
        .map_err(|_| EncryptionError::KeyDerivationFailed)?;
    Ok(key)
}

/// Encrypt plaintext using ChaCha20-Poly1305.
/// Returns: salt (16 bytes) || nonce (12 bytes) || ciphertext || tag (16 bytes)
pub fn encrypt(plaintext: &[u8], password: &str) -> Result<Vec<u8>, EncryptionError> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);

    // The key is cleared on drop, including when decryption returns an error.
    let key = derive_key(password, &salt)?;

    let cipher =
        ChaCha20Poly1305::new_from_slice(&*key).map_err(|_| EncryptionError::EncryptionFailed)?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let mut result = Vec::with_capacity(16 + NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Derive a key from a password using a fresh random salt.
/// Returns (key, salt) so the caller can cache the key and reuse it.
///
/// Callers can retain the key in its zeroizing buffer for reuse. They must also
/// protect the password and any copies of the key.
pub fn derive_key_fresh(
    password: &str,
) -> Result<(Zeroizing<[u8; 32]>, [u8; 16]), EncryptionError> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let key = derive_key(password, &salt)?;
    Ok((key, salt))
}

/// Encrypt plaintext using a pre-derived key and its associated salt.
/// Output format is identical to `encrypt()`: salt || nonce || ciphertext+tag.
/// This allows `decrypt()` with the original password to work unchanged.
pub fn encrypt_with_key(
    plaintext: &[u8],
    key: &[u8; 32],
    salt: &[u8; 16],
) -> Result<Vec<u8>, EncryptionError> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|_| EncryptionError::EncryptionFailed)?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let mut result = Vec::with_capacity(16 + NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(salt);
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

    let key = derive_key(password, salt)?;

    let cipher =
        ChaCha20Poly1305::new_from_slice(&*key).map_err(|_| EncryptionError::DecryptionFailed)?;

    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| EncryptionError::DecryptionFailed)?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Compare against the original `hash_password` API in the compatibility test.
    use argon2::password_hash::PasswordHasher;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "test_password_123";
        let plaintext = b"Hello, World! This is a test message.";

        let encrypted = encrypt(plaintext, password).expect("Encryption failed");
        // Encrypted data must be larger than plaintext (has salt + nonce + tag)
        assert!(encrypted.len() > plaintext.len());
        // Must start with 16-byte salt + 12-byte nonce = 28 bytes overhead minimum
        assert!(encrypted.len() >= plaintext.len() + 28);
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

    /// Check the derived key against a vector from the original implementation.
    /// Existing encrypted wallets must remain readable after changes to the KDF.
    #[test]
    fn derive_key_matches_the_captured_vector() {
        assert_eq!(
            hex::encode(&*derive_key("test_password_123", &[0u8; 16]).unwrap()),
            "3b69d3759a1eed1ef9876add0fd2e097f1752d0431f02c1dda5e8dbaf1852183",
            "the KDF's output changed; every stored wallet is encrypted under the old one"
        );
    }

    /// Check that both Argon2 APIs still produce the same key after dependency updates.
    #[test]
    fn derive_key_matches_the_password_hash_api_it_replaced() {
        for (password, salt) in [
            ("test_password_123", [0u8; 16]),
            ("", [1u8; 16]),
            ("a much longer password with spaces and ünïcödé", [0xABu8; 16]),
        ] {
            let params = argon2::Params::new(65536, 3, 1, Some(32)).unwrap();
            let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);
            let salt_string = SaltString::encode_b64(&salt).unwrap();
            let hash = argon2
                .hash_password(password.as_bytes(), &salt_string)
                .unwrap();
            let expected = hash.hash.unwrap();

            assert_eq!(
                &*derive_key(password, &salt).unwrap(),
                &expected.as_bytes()[..32],
                "password={password:?}"
            );
        }
    }

    #[test]
    fn test_encrypt_with_key_decrypt_with_password() {
        let password = "test_password_123";
        let plaintext = b"Auto-saved wallet data";

        let (key, salt) = derive_key_fresh(password).expect("Key derivation failed");
        let encrypted = encrypt_with_key(plaintext, &key, &salt).expect("Encrypt with key failed");
        let decrypted = decrypt(&encrypted, password).expect("Decrypt with password failed");

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

    #[test]
    fn test_encrypt_decrypt_with_realistic_wallet_json() {
        let wallet_json = r#"[{"tx_hash":"abc123","output_index":0,"amount":1000000000000,"key":"deadbeef","key_image":"ki_abc"}]"#;
        let password = "strong_password_42";
        let encrypted = encrypt(wallet_json.as_bytes(), password).expect("Encryption failed");
        let decrypted = decrypt(&encrypted, password).expect("Decryption failed");
        assert_eq!(wallet_json.as_bytes(), &decrypted[..]);
    }

    #[test]
    fn test_encrypt_produces_different_ciphertext_each_call() {
        let plaintext = b"same plaintext";
        let password = "same_password";
        let enc1 = encrypt(plaintext, password).expect("Encryption failed");
        let enc2 = encrypt(plaintext, password).expect("Encryption failed");
        // Different salt + nonce = different ciphertext
        assert_ne!(enc1, enc2);
        // But both decrypt to same plaintext
        assert_eq!(
            decrypt(&enc1, password).expect("Decryption failed"),
            decrypt(&enc2, password).expect("Decryption failed"),
        );
    }
}
