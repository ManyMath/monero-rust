//! Reader for monero-wallet-cli `.keys` files.

use chacha20::ChaCha20Legacy;
use cipher::{KeyIvInit, StreamCipher};
use cuprate_cryptonight::cryptonight_hash_v0;
use monero_serai::wallet::seed::{Language, Seed};
use serde::{Deserialize, Serialize};
use curve25519_dalek::scalar::Scalar;
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

#[derive(Debug, Clone)]
pub struct ImportedKeysFile {
    pub spend_secret_key: [u8; 32],
    pub view_secret_key: [u8; 32],
    pub spend_public_key: [u8; 32],
    pub view_public_key: [u8; 32],
    pub creation_timestamp: u64,
    pub watch_only: bool,
    pub seed_language: Option<String>,
    /// Complete mnemonic backup, available only for deterministic full wallets.
    /// Other wallets require both secret keys or the original `.keys` file.
    pub mnemonic: Option<String>,
}

// Field order must be alphabetical to match Monero C++ epee (std::map).
#[derive(Serialize, Deserialize)]
struct EpeeAccountBase {
    m_creation_timestamp: u64,
    m_keys: EpeeAccountKeys,
}

#[derive(Serialize, Deserialize)]
struct EpeeAccountKeys {
    m_account_address: EpeeAccountAddress,
    #[serde(with = "serde_bytes")]
    m_encryption_iv: Vec<u8>,
    #[serde(with = "serde_bytes")]
    m_spend_secret_key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    m_view_secret_key: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct EpeeAccountAddress {
    #[serde(with = "serde_bytes")]
    m_spend_public_key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    m_view_public_key: Vec<u8>,
}

fn to_key(v: &[u8], name: &str) -> Result<[u8; 32], String> {
    v.try_into()
        .map_err(|_| format!("{name}: expected 32 bytes, got {}", v.len()))
}

fn read_leb128(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        if *offset >= data.len() {
            return Err("unexpected end of varint".into());
        }
        let byte = data[*offset];
        *offset += 1;
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err("varint too large".into());
        }
    }
    Ok(result)
}

fn find_json_string_value(data: &[u8], field_name: &str) -> Option<(usize, usize)> {
    let needle = format!("\"{}\":\"", field_name);
    let needle_bytes = needle.as_bytes();
    let pos = data.windows(needle_bytes.len()).position(|w| w == needle_bytes)?;
    let start = pos + needle_bytes.len();
    let mut i = start;
    while i < data.len() {
        if data[i] == b'\\' {
            i += 2;
        } else if data[i] == b'"' {
            return Some((start, i));
        } else {
            i += 1;
        }
    }
    None
}

fn find_json_int_value(data: &[u8], field_name: &str) -> Option<u64> {
    let needle = format!("\"{}\":", field_name);
    let needle_bytes = needle.as_bytes();
    let pos = data.windows(needle_bytes.len()).position(|w| w == needle_bytes)?;
    let start = pos + needle_bytes.len();
    let mut end = start;
    while end < data.len() && data[end].is_ascii_digit() {
        end += 1;
    }
    if end == start { return None; }
    std::str::from_utf8(&data[start..end]).ok()?.parse().ok()
}

fn unescape_json_bytes(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\\' {
            i += 1;
            if i >= data.len() { return Err("trailing backslash".into()); }
            match data[i] {
                b'"' => { out.push(b'"'); i += 1; }
                b'\\' => { out.push(b'\\'); i += 1; }
                b'/' => { out.push(b'/'); i += 1; }
                b'b' => { out.push(0x08); i += 1; }
                b'f' => { out.push(0x0C); i += 1; }
                b'n' => { out.push(b'\n'); i += 1; }
                b'r' => { out.push(b'\r'); i += 1; }
                b't' => { out.push(b'\t'); i += 1; }
                b'u' => {
                    i += 1;
                    if i + 4 > data.len() { return Err("truncated \\uXXXX".into()); }
                    let hex_str = std::str::from_utf8(&data[i..i + 4])
                        .map_err(|_| "invalid \\uXXXX")?;
                    let cp = u16::from_str_radix(hex_str, 16)
                        .map_err(|_| format!("bad \\u: {hex_str}"))?;
                    if cp <= 0xFF {
                        out.push(cp as u8);
                    } else {
                        let ch = char::from_u32(cp as u32)
                            .ok_or_else(|| format!("invalid U+{cp:04X}"))?;
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    i += 4;
                }
                other => return Err(format!("unknown escape: \\{}", other as char)),
            }
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    Ok(out)
}

fn parse_language(lang: &str) -> Option<Language> {
    match lang {
        "English" => Some(Language::English),
        "Dutch" | "Nederlands" => Some(Language::Dutch),
        "French" | "Français" => Some(Language::French),
        "Spanish" | "Español" => Some(Language::Spanish),
        "German" | "Deutsch" => Some(Language::German),
        "Italian" | "Italiano" => Some(Language::Italian),
        "Portuguese" | "Português" => Some(Language::Portuguese),
        "Japanese" | "日本語" => Some(Language::Japanese),
        "Chinese" | "简体中文 (中国)" => Some(Language::Chinese),
        "Russian" | "Русский" => Some(Language::Russian),
        "Esperanto" => Some(Language::Esperanto),
        "Lojban" => Some(Language::Lojban),
        _ => None,
    }
}

pub fn decrypt_keys_data(file_bytes: &[u8], password: &str) -> Result<(Vec<u8>, [u8; 32]), String> {
    if file_bytes.len() < 9 {
        return Err("file too small to be a valid .keys file".into());
    }

    let iv: [u8; 8] = file_bytes[..8].try_into().unwrap();
    let mut offset: usize = 8;
    let ciphertext_len = read_leb128(file_bytes, &mut offset)? as usize;
    if offset + ciphertext_len > file_bytes.len() {
        return Err("ciphertext length exceeds file size".into());
    }

    let mut buf = file_bytes[offset..offset + ciphertext_len].to_vec();
    let key: [u8; 32] = cryptonight_hash_v0(password.as_bytes());
    let mut cipher = ChaCha20Legacy::new((&key).into(), (&iv).into());
    cipher.apply_keystream(&mut buf);

    if buf.len() < 2 || buf[0] != b'{' || buf[1] != b'"' {
        return Err("decryption produced garbage (wrong password?)".into());
    }

    Ok((buf, key))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_keys_file(path: &std::path::Path, password: &str) -> Result<ImportedKeysFile, String> {
    let file_data =
        std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let (plaintext, key) = decrypt_keys_data(&file_data, password)?;
    parse_decrypted_keys(&plaintext, &key)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn decrypt_keys_file(path: &std::path::Path, password: &str) -> Result<Vec<u8>, String> {
    let file_data =
        std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let (plaintext, _key) = decrypt_keys_data(&file_data, password)?;
    Ok(plaintext)
}

pub fn parse_decrypted_keys(plaintext: &[u8], chacha_key: &[u8; 32]) -> Result<ImportedKeysFile, String> {
    let (kd_start, kd_end) = find_json_string_value(plaintext, "key_data")
        .ok_or("key_data field not found")?;

    let key_data_raw = &plaintext[kd_start..kd_end];
    let key_data_bytes = if !key_data_raw.is_empty() && key_data_raw.iter().all(|b| b.is_ascii_hexdigit()) {
        hex::decode(std::str::from_utf8(key_data_raw).unwrap())
            .map_err(|e| format!("hex decode: {e}"))?
    } else {
        unescape_json_bytes(key_data_raw)?
    };

    let acct: EpeeAccountBase = monero_epee_bin_serde::from_bytes(&key_data_bytes)
        .map_err(|e| format!("epee deserialize: {e}"))?;

    let spend_public_key = to_key(&acct.m_keys.m_account_address.m_spend_public_key, "m_spend_public_key")?;
    let view_public_key = to_key(&acct.m_keys.m_account_address.m_view_public_key, "m_view_public_key")?;
    let mut spend_secret_key = to_key(&acct.m_keys.m_spend_secret_key, "m_spend_secret_key")?;
    let mut view_secret_key = to_key(&acct.m_keys.m_view_secret_key, "m_view_secret_key")?;
    let creation_timestamp = acct.m_creation_timestamp;

    // Decrypt secret keys via xor_with_key_stream if they're encrypted
    let encrypted_secret_keys =
        find_json_int_value(plaintext, "encrypted_secret_keys").unwrap_or(0) != 0;

    if encrypted_secret_keys {
        let encryption_iv: [u8; 8] = acct.m_keys.m_encryption_iv.as_slice()
            .try_into()
            .map_err(|_| format!(
                "m_encryption_iv: expected 8 bytes, got {}",
                acct.m_keys.m_encryption_iv.len()
            ))?;

        let mut derive_input = [0u8; 33];
        derive_input[..32].copy_from_slice(chacha_key);
        derive_input[32] = 0x6b; // HASH_KEY_MEMORY
        let derived_key: [u8; 32] = cryptonight_hash_v0(&derive_input);

        let mut keystream = [0u8; 64];
        let mut cipher = ChaCha20Legacy::new((&derived_key).into(), (&encryption_iv).into());
        cipher.apply_keystream(&mut keystream);

        for i in 0..32 {
            spend_secret_key[i] ^= keystream[i];
        }
        for i in 0..32 {
            view_secret_key[i] ^= keystream[32 + i];
        }
    }

    let watch_only = find_json_int_value(plaintext, "watch_only").unwrap_or(0) != 0;
    let seed_language = find_json_string_value(plaintext, "seed_language").and_then(|(s, e)| {
        std::str::from_utf8(&plaintext[s..e]).ok().map(|s| s.to_string())
    });

    let derived_view = Zeroizing::new(Scalar::from_bytes_mod_order(
        Keccak256::digest(spend_secret_key).into(),
    ));
    let mnemonic = if !watch_only && derived_view.to_bytes() == view_secret_key {
        let lang = seed_language.as_deref().and_then(parse_language).unwrap_or(Language::English);
        Seed::from_entropy(lang, Zeroizing::new(spend_secret_key))
            .map(|seed| seed.to_string().to_string())
    } else {
        None
    };

    Ok(ImportedKeysFile {
        spend_secret_key,
        view_secret_key,
        spend_public_key,
        view_public_key,
        creation_timestamp,
        watch_only,
        seed_language,
        mnemonic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_roundtrip() {
        let data = [0xAC, 0x02];
        let mut off = 0;
        assert_eq!(read_leb128(&data, &mut off).unwrap(), 300);
    }

    #[test]
    fn unescape_control_chars() {
        let input = br#"\u0001\u0011hello"#;
        let out = unescape_json_bytes(input).unwrap();
        assert_eq!(&out, &[0x01, 0x11, b'h', b'e', b'l', b'l', b'o']);
    }

    #[test]
    fn find_field_in_json() {
        let json = br#"{"foo":"bar","baz":"qux"}"#;
        let (s, e) = find_json_string_value(json, "baz").unwrap();
        assert_eq!(&json[s..e], b"qux");
    }
}
