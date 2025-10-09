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
const NONCE_SIZE: usize = 12;
const SALT_SIZE: usize = 32;

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

    let nonce_obj = Nonce::from_slice(nonce);

    let ciphertext = cipher
        .encrypt(nonce_obj, data)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    Ok(ciphertext)
}

/// Decrypts data using AES-256-GCM with a password-derived key.
pub fn decrypt_wallet_data(
    ciphertext: &[u8],
    password: &str,
    salt: &[u8; SALT_SIZE],
    nonce: &[u8; NONCE_SIZE],
) -> Result<Vec<u8>, String> {
    let key = derive_encryption_key(password, salt)?;

    let cipher = Aes256Gcm::new_from_slice(&*key)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;

    let nonce_obj = Nonce::from_slice(nonce);

    let plaintext = cipher
        .decrypt(nonce_obj, ciphertext)
        .map_err(|_| "Decryption failed: invalid password or corrupted data".to_string())?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_derivation_deterministic() {
        let password = "test_password_123";
        let salt = [42u8; SALT_SIZE];

        let key1 = derive_encryption_key(password, &salt).expect("Failed to derive key");
        let key2 = derive_encryption_key(password, &salt).expect("Failed to derive key");

        assert_eq!(&*key1, &*key2);
    }

    #[test]
    fn test_key_derivation_different_salts() {
        let password = "test_password_123";
        let salt1 = [1u8; SALT_SIZE];
        let salt2 = [2u8; SALT_SIZE];

        let key1 = derive_encryption_key(password, &salt1).expect("Failed to derive key");
        let key2 = derive_encryption_key(password, &salt2).expect("Failed to derive key");

        assert_ne!(&*key1, &*key2);
    }

    #[test]
    fn test_key_derivation_different_passwords() {
        let salt = [42u8; SALT_SIZE];
        let password1 = "password1";
        let password2 = "password2";

        let key1 = derive_encryption_key(password1, &salt).expect("Failed to derive key");
        let key2 = derive_encryption_key(password2, &salt).expect("Failed to derive key");

        assert_ne!(&*key1, &*key2);
    }

    #[test]
    fn test_nonce_generation() {
        let nonce1 = generate_nonce();
        let nonce2 = generate_nonce();

        assert_ne!(nonce1, nonce2);
        assert_eq!(nonce1.len(), NONCE_SIZE);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let plaintext = b"This is my secret wallet data!";
        let password = "my_secure_password";
        let salt = [99u8; SALT_SIZE];
        let nonce = generate_nonce();

        let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce)
            .expect("Encryption failed");

        assert_ne!(&ciphertext[..plaintext.len()], plaintext);
        assert_eq!(ciphertext.len(), plaintext.len() + 16);

        let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce)
            .expect("Decryption failed");

        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_wrong_password() {
        let plaintext = b"Secret data";
        let correct_password = "correct";
        let wrong_password = "wrong";
        let salt = [123u8; SALT_SIZE];
        let nonce = generate_nonce();

        let ciphertext = encrypt_wallet_data(plaintext, correct_password, &salt, &nonce)
            .expect("Encryption failed");

        let result = decrypt_wallet_data(&ciphertext, wrong_password, &salt, &nonce);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid password"));
    }

    #[test]
    fn test_decrypt_wrong_salt() {
        let plaintext = b"Secret data";
        let password = "password";
        let salt1 = [1u8; SALT_SIZE];
        let salt2 = [2u8; SALT_SIZE];
        let nonce = generate_nonce();

        let ciphertext = encrypt_wallet_data(plaintext, password, &salt1, &nonce)
            .expect("Encryption failed");

        let result = decrypt_wallet_data(&ciphertext, password, &salt2, &nonce);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_wrong_nonce() {
        let plaintext = b"Secret data";
        let password = "password";
        let salt = [42u8; SALT_SIZE];
        let nonce1 = [1u8; NONCE_SIZE];
        let nonce2 = [2u8; NONCE_SIZE];

        let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce1)
            .expect("Encryption failed");

        let result = decrypt_wallet_data(&ciphertext, password, &salt, &nonce2);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_corrupted_data() {
        let plaintext = b"Secret data";
        let password = "password";
        let salt = [42u8; SALT_SIZE];
        let nonce = generate_nonce();

        let mut ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce)
            .expect("Encryption failed");

        ciphertext[0] ^= 0xFF;

        let result = decrypt_wallet_data(&ciphertext, password, &salt, &nonce);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid password or corrupted data"));
    }

    #[test]
    fn test_encrypt_large_data() {
        let plaintext = vec![0xAB; 10_000];
        let password = "password";
        let salt = [42u8; SALT_SIZE];
        let nonce = generate_nonce();

        let ciphertext = encrypt_wallet_data(&plaintext, password, &salt, &nonce)
            .expect("Encryption failed");

        let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce)
            .expect("Decryption failed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_empty_data() {
        let plaintext = b"";
        let password = "password";
        let salt = [42u8; SALT_SIZE];
        let nonce = generate_nonce();

        let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce)
            .expect("Encryption failed");

        assert_eq!(ciphertext.len(), 16);

        let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce)
            .expect("Decryption failed");

        assert_eq!(&decrypted, plaintext);
    }
}
