//! Feather/Monero-compatible epee serialization for offline transaction signing.
//!
//! Provides conversion between our internal `UnsignedTransaction` format and Monero's
//! standard `unsigned_tx_set` / `signed_tx_set` / `key_images` epee formats,
//! enabling interoperability with Feather Wallet, monero-wallet-cli, and XmrSigner.

use serde::{Deserialize, Serialize};

pub const UNSIGNED_TX_MAGIC: &[u8] = b"Monero unsigned tx set\x05";
pub const SIGNED_TX_MAGIC: &[u8] = b"Monero signed tx set\x05";
pub const KEY_IMAGES_MAGIC: &[u8] = b"Monero key image export\x03";
pub const OUTPUT_EXPORT_MAGIC: &[u8] = b"Monero output export\x04";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedKeyImage {
    pub key_image: String,
    pub tx_hash: String,
    pub output_index: u8,
}

pub fn wrap_with_magic(magic: &[u8], data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(magic.len() + data.len());
    result.extend_from_slice(magic);
    result.extend_from_slice(data);
    result
}

pub fn strip_magic<'a>(magic: &[u8], data: &'a [u8]) -> Result<&'a [u8], String> {
    if data.len() < magic.len() {
        return Err("Data too short for magic header".to_string());
    }
    if &data[..magic.len()] != magic {
        return Err("Magic header mismatch".to_string());
    }
    Ok(&data[magic.len()..])
}

pub fn export_key_images(key_images: &[ExportedKeyImage]) -> Result<Vec<u8>, String> {
    #[derive(Serialize)]
    struct KeyImageExport {
        key_images: Vec<KeyImageEntry>,
    }

    #[derive(Serialize)]
    struct KeyImageEntry {
        key_image: Vec<u8>,
    }

    let entries: Result<Vec<_>, _> = key_images.iter().map(|ki| {
        let bytes = hex::decode(&ki.key_image)
            .map_err(|e| format!("Invalid key image hex: {:?}", e))?;
        if bytes.len() != 32 {
            return Err("Key image must be 32 bytes".to_string());
        }
        Ok(KeyImageEntry { key_image: bytes })
    }).collect();

    let export = KeyImageExport { key_images: entries? };
    let epee_data = monero_epee_bin_serde::to_bytes(&export)
        .map_err(|e| format!("epee serialize: {:?}", e))?;

    Ok(wrap_with_magic(KEY_IMAGES_MAGIC, &epee_data))
}

pub fn import_key_images(data: &[u8]) -> Result<Vec<String>, String> {
    let payload = strip_magic(KEY_IMAGES_MAGIC, data)?;

    #[derive(Deserialize)]
    struct KeyImageExport {
        key_images: Vec<KeyImageEntry>,
    }

    #[derive(Deserialize)]
    struct KeyImageEntry {
        key_image: Vec<u8>,
    }

    let export: KeyImageExport = monero_epee_bin_serde::from_bytes(payload)
        .map_err(|e| format!("epee deserialize: {:?}", e))?;

    Ok(export.key_images.into_iter().map(|e| hex::encode(e.key_image)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_wrap_strip_roundtrip() {
        let data = b"hello world";
        let wrapped = wrap_with_magic(UNSIGNED_TX_MAGIC, data);
        let stripped = strip_magic(UNSIGNED_TX_MAGIC, &wrapped).unwrap();
        assert_eq!(stripped, data);
    }

    #[test]
    fn test_magic_mismatch() {
        let wrapped = wrap_with_magic(UNSIGNED_TX_MAGIC, b"data");
        let result = strip_magic(SIGNED_TX_MAGIC, &wrapped);
        assert!(result.is_err());
    }

    #[test]
    fn test_magic_too_short() {
        let result = strip_magic(UNSIGNED_TX_MAGIC, b"short");
        assert!(result.is_err());
    }
}
