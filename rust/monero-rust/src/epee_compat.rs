//! Key image export/import with support for both legacy EPEE and Monero v3 formats.
//!
//! The old EPEE-based export format is deprecated but retained for backward
//! compatibility. New exports should use `key_image_signing::export_key_images_v3`
//! for full interoperability with Feather Wallet and monero-wallet-cli.
//!
//! The `import_key_images` function auto-detects the format: it tries EPEE first,
//! and if that fails, falls back to the v3 encrypted binary format (requires
//! the view secret key for decryption).

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

#[deprecated(note = "Use key_image_signing::export_key_images_v3 for interop-compatible format")]
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

/// Import key images from either old EPEE format or new v3 encrypted format.
///
/// - If `view_secret_key` is `None`, only old EPEE format is supported.
/// - If `view_secret_key` is `Some`, tries old EPEE format first, then v3.
pub fn import_key_images(data: &[u8], view_secret_key: Option<&[u8; 32]>) -> Result<Vec<String>, String> {
    let payload = strip_magic(KEY_IMAGES_MAGIC, data)?;

    // Try old EPEE format first.
    // Note: EPEE serializes Vec<u8> as a byte array, so we must use
    // serde_bytes::ByteBuf for deserialization to handle this correctly.
    #[derive(Deserialize)]
    struct KeyImageExport {
        key_images: Vec<KeyImageEntry>,
    }

    #[derive(Deserialize)]
    struct KeyImageEntry {
        #[serde(with = "serde_bytes")]
        key_image: Vec<u8>,
    }

    if let Ok(export) = monero_epee_bin_serde::from_bytes::<KeyImageExport, _>(payload) {
        return Ok(export.key_images.into_iter().map(|e| hex::encode(e.key_image)).collect());
    }

    // EPEE failed; try v3 encrypted format if view key is provided
    match view_secret_key {
        Some(vsk) => {
            let (key_images, _pub_spend, _pub_view) =
                crate::key_image_signing::import_key_images_v3(data, vsk)?;
            Ok(key_images.iter().map(hex::encode).collect())
        }
        None => Err("Data is not in old EPEE format and no view secret key provided for v3 decryption".to_string()),
    }
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

    #[test]
    fn test_backward_compat_import_old_epee_format() {
        // Create data in the old EPEE format
        let key_images = vec![
            ExportedKeyImage {
                key_image: "a".repeat(64), // 32 bytes as hex
                tx_hash: "b".repeat(64),
                output_index: 0,
            },
        ];

        #[allow(deprecated)]
        let exported = export_key_images(&key_images).expect("old export should work");

        // Import with None view key (old format only)
        let imported = import_key_images(&exported, None).expect("old import should work");
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0], "a".repeat(64));
    }

    #[test]
    fn test_import_rejects_non_epee_without_view_key() {
        // Construct data that looks like it has magic but isn't EPEE
        let mut data = Vec::new();
        data.extend_from_slice(KEY_IMAGES_MAGIC);
        data.extend_from_slice(b"this is not epee data at all!");

        let result = import_key_images(&data, None);
        assert!(result.is_err(), "Non-EPEE data without view key should fail");
    }
}
