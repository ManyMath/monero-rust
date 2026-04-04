use crate::monero_backend::wallet::seed::{Language, Seed};
use curve25519_dalek::scalar::Scalar;
use zeroize::Zeroizing;

/// Convert a 12-word BIP39 mnemonic into a 25-word Monero legacy mnemonic.
///
/// Algorithm (matches Cake Wallet's "Exodus-style" BIP39 implementation):
/// 1. BIP39 mnemonic + passphrase -> PBKDF2-HMAC-SHA512 (2048 rounds) -> 64-byte seed
/// 2. BIP32 derivation m/44'/128'/<account_index>'/0/0 -> 32-byte secp256k1 private key
/// 3. Scalar::from_bytes_mod_order(key_bytes) -> reduce mod Ed25519 curve order
/// 4. Seed::from_entropy(English, scalar_bytes) -> 25-word Monero legacy mnemonic
pub fn bip39_to_legacy_mnemonic(
    mnemonic: &str,
    passphrase: &str,
    account_index: u32,
) -> Result<String, String> {
    let parsed = bip39::Mnemonic::parse_in(bip39::Language::English, mnemonic)
        .map_err(|e| format!("Invalid BIP39 mnemonic: {}", e))?;

    let bip39_seed = parsed.to_seed(passphrase);

    let path = format!("m/44'/128'/{}'/0/0", account_index);
    let derivation_path: bip32::DerivationPath = path
        .parse()
        .map_err(|e| format!("Invalid derivation path: {}", e))?;

    let child_key = bip32::XPrv::derive_from_path(&bip39_seed, &derivation_path)
        .map_err(|e| format!("BIP32 derivation failed: {}", e))?;

    let key_bytes: [u8; 32] = child_key.private_key().to_bytes().into();

    // Scalar::from_bytes_mod_order interprets bytes as little-endian and reduces mod l.
    // The BIP32 private key bytes are already in the right byte order for this operation
    // (Cake Wallet's _readBytes reads as LE, which is what from_bytes_mod_order expects).
    let scalar = Scalar::from_bytes_mod_order(key_bytes);

    let entropy = Zeroizing::new(scalar.to_bytes());
    let seed = Seed::from_entropy(Language::English, entropy)
        .ok_or_else(|| "Failed to encode as Monero legacy mnemonic".to_string())?;

    Ok(Seed::to_string(&seed).to_string())
}

/// Validate a 12-word BIP39 mnemonic.
pub fn validate_bip39(mnemonic: &str) -> Result<(), String> {
    bip39::Mnemonic::parse_in(bip39::Language::English, mnemonic)
        .map_err(|e| format!("Invalid BIP39 mnemonic: {}", e))?;
    Ok(())
}

/// Generate a new random 12-word BIP39 mnemonic.
pub fn generate_bip39() -> Result<String, String> {
    let mnemonic = bip39::Mnemonic::generate(12)
        .map_err(|e| format!("Failed to generate BIP39 mnemonic: {}", e))?;
    Ok(mnemonic.to_string())
}
