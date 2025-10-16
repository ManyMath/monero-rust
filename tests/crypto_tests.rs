use monero_rust::crypto::{
    decrypt_wallet_data, derive_encryption_key, encrypt_wallet_data, generate_nonce,
    NONCE_SIZE, SALT_SIZE,
};

#[test]
fn test_key_derivation_deterministic() {
    let password = "test_password_123";
    let salt = [42u8; SALT_SIZE];

    let key1 = derive_encryption_key(password, &salt).unwrap();
    let key2 = derive_encryption_key(password, &salt).unwrap();

    assert_eq!(&*key1, &*key2);
}

#[test]
fn test_key_derivation_different_salts() {
    let password = "test_password_123";
    let salt1 = [1u8; SALT_SIZE];
    let salt2 = [2u8; SALT_SIZE];

    let key1 = derive_encryption_key(password, &salt1).unwrap();
    let key2 = derive_encryption_key(password, &salt2).unwrap();

    assert_ne!(&*key1, &*key2);
}

#[test]
fn test_key_derivation_different_passwords() {
    let salt = [42u8; SALT_SIZE];

    let key1 = derive_encryption_key("password1", &salt).unwrap();
    let key2 = derive_encryption_key("password2", &salt).unwrap();

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

    let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce).unwrap();

    assert_ne!(&ciphertext[..plaintext.len()], plaintext);
    assert_eq!(ciphertext.len(), plaintext.len() + 16); // +16 for auth tag

    let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce).unwrap();
    assert_eq!(&decrypted, plaintext);
}

#[test]
fn test_decrypt_wrong_password() {
    let plaintext = b"Secret data";
    let salt = [123u8; SALT_SIZE];
    let nonce = generate_nonce();

    let ciphertext = encrypt_wallet_data(plaintext, "correct", &salt, &nonce).unwrap();

    let result = decrypt_wallet_data(&ciphertext, "wrong", &salt, &nonce);
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

    let ciphertext = encrypt_wallet_data(plaintext, password, &salt1, &nonce).unwrap();

    assert!(decrypt_wallet_data(&ciphertext, password, &salt2, &nonce).is_err());
}

#[test]
fn test_decrypt_wrong_nonce() {
    let plaintext = b"Secret data";
    let password = "password";
    let salt = [42u8; SALT_SIZE];
    let nonce1 = [1u8; NONCE_SIZE];
    let nonce2 = [2u8; NONCE_SIZE];

    let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce1).unwrap();

    assert!(decrypt_wallet_data(&ciphertext, password, &salt, &nonce2).is_err());
}

#[test]
fn test_decrypt_corrupted_data() {
    let plaintext = b"Secret data";
    let password = "password";
    let salt = [42u8; SALT_SIZE];
    let nonce = generate_nonce();

    let mut ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce).unwrap();
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

    let ciphertext = encrypt_wallet_data(&plaintext, password, &salt, &nonce).unwrap();
    let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce).unwrap();

    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_encrypt_empty_data() {
    let plaintext = b"";
    let password = "password";
    let salt = [42u8; SALT_SIZE];
    let nonce = generate_nonce();

    let ciphertext = encrypt_wallet_data(plaintext, password, &salt, &nonce).unwrap();
    assert_eq!(ciphertext.len(), 16); // just the auth tag

    let decrypted = decrypt_wallet_data(&ciphertext, password, &salt, &nonce).unwrap();
    assert_eq!(&decrypted, plaintext);
}
