use monero_rust::crypto::{
    derive_encryption_key, encrypt_wallet_data, decrypt_wallet_data,
    generate_nonce, SALT_SIZE, NONCE_SIZE
};

#[test]
fn test_key_derivation_deterministic() {
    let password = "test_password_123";
    let salt = [42u8; SALT_SIZE];

    let key1 = derive_encryption_key(password, &salt).expect("Failed to derive key");
    let key2 = derive_encryption_key(password, &salt).expect("Failed to derive key");

    // Same password and salt should produce same key
    assert_eq!(&*key1, &*key2);
}

#[test]
fn test_key_derivation_different_salts() {
    let password = "test_password_123";
    let salt1 = [1u8; SALT_SIZE];
    let salt2 = [2u8; SALT_SIZE];

    let key1 = derive_encryption_key(password, &salt1).expect("Failed to derive key");
    let key2 = derive_encryption_key(password, &salt2).expect("Failed to derive key");

    // Different salts should produce different keys
    assert_ne!(&*key1, &*key2);
}

#[test]
fn test_key_derivation_different_passwords() {
    let salt = [42u8; SALT_SIZE];
    let password1 = "password1";
    let password2 = "password2";

    let key1 = derive_encryption_key(password1, &salt).expect("Failed to derive key");
    let key2 = derive_encryption_key(password2, &salt).expect("Failed to derive key");

    // Different passwords should produce different keys
    assert_ne!(&*key1, &*key2);
}

#[test]
fn test_nonce_generation() {
    let nonce1 = generate_nonce();
    let nonce2 = generate_nonce();

    // Nonces should be different (with very high probability)
    assert_ne!(nonce1, nonce2);
    assert_eq!(nonce1.len(), NONCE_SIZE);
}

#[test]
fn test_encrypt_decrypt_roundtrip() {
    let plaintext = b"This is my secret wallet data!";
    let password = "my_secure_password";
    let salt = [99u8; SALT_SIZE];
    let nonce = generate_nonce();

    // Encrypt
    let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce)
        .expect("Encryption failed");

    // Ciphertext should be different from plaintext
    assert_ne!(&ciphertext[..plaintext.len()], plaintext);

    // Ciphertext should be longer (includes 16-byte auth tag)
    assert_eq!(ciphertext.len(), plaintext.len() + 16);

    // Decrypt
    let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce)
        .expect("Decryption failed");

    // Decrypted should match original plaintext
    assert_eq!(&decrypted, plaintext);
}

#[test]
fn test_decrypt_wrong_password() {
    let plaintext = b"Secret data";
    let correct_password = "correct";
    let wrong_password = "wrong";
    let salt = [123u8; SALT_SIZE];
    let nonce = generate_nonce();

    // Encrypt with correct password
    let ciphertext = encrypt_wallet_data(plaintext, correct_password, &salt, &nonce)
        .expect("Encryption failed");

    // Decrypt with wrong password should fail
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

    // Encrypt with salt1
    let ciphertext = encrypt_wallet_data(plaintext, password, &salt1, &nonce)
        .expect("Encryption failed");

    // Decrypt with salt2 should fail
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

    // Encrypt with nonce1
    let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce1)
        .expect("Encryption failed");

    // Decrypt with nonce2 should fail
    let result = decrypt_wallet_data(&ciphertext, password, &salt, &nonce2);
    assert!(result.is_err());
}

#[test]
fn test_decrypt_corrupted_data() {
    let plaintext = b"Secret data";
    let password = "password";
    let salt = [42u8; SALT_SIZE];
    let nonce = generate_nonce();

    // Encrypt
    let mut ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce)
        .expect("Encryption failed");

    // Corrupt the ciphertext
    ciphertext[0] ^= 0xFF;

    // Decrypt should fail due to authentication tag mismatch
    let result = decrypt_wallet_data(&ciphertext, password, &salt, &nonce);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid password or corrupted data"));
}

#[test]
fn test_encrypt_large_data() {
    let plaintext = vec![0xAB; 10_000]; // 10 KB of data
    let password = "password";
    let salt = [42u8; SALT_SIZE];
    let nonce = generate_nonce();

    // Encrypt
    let ciphertext = encrypt_wallet_data(&plaintext, password, &salt, &nonce)
        .expect("Encryption failed");

    // Decrypt
    let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce)
        .expect("Decryption failed");

    // Should match
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_encrypt_empty_data() {
    let plaintext = b"";
    let password = "password";
    let salt = [42u8; SALT_SIZE];
    let nonce = generate_nonce();

    // Encrypt empty data
    let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce)
        .expect("Encryption failed");

    // Should still have auth tag (16 bytes)
    assert_eq!(ciphertext.len(), 16);

    // Decrypt
    let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce)
        .expect("Decryption failed");

    assert_eq!(&decrypted, plaintext);
}
