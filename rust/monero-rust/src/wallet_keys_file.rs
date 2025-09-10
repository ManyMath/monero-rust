//! Reader for monero-wallet-cli `.keys` files.

use std::collections::HashMap;
use std::path::Path;

use chacha20::ChaCha20Legacy;
use cipher::{KeyIvInit, StreamCipher};
use cuprate_cryptonight::cryptonight_hash_v0;
use monero_serai::wallet::seed::{Language, Seed};
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

const EPEE_HEADER: &[u8] = b"\x01\x11\x01\x01\x01\x01\x02\x01\x01";

/// Parsed epee value.
#[derive(Debug, Clone)]
enum EpeeValue {
    U64(u64),
    Blob(Vec<u8>),
    Section(HashMap<String, EpeeValue>),
    #[allow(dead_code)]
    Other,
}

struct EpeeCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> EpeeCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        if self.pos >= self.data.len() {
            return Err("unexpected EOF reading u8".into());
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.data.len() {
            return Err(format!("unexpected EOF reading {} bytes at {}", n, self.pos));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn read_u64_le(&mut self) -> Result<u64, String> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_epee_varint(&mut self) -> Result<usize, String> {
        if self.remaining() == 0 {
            return Err("unexpected EOF reading epee varint".into());
        }
        let width = self.data[self.pos] & 0x03;
        let (raw, size) = match width {
            0 => (self.read_u8()? as u64, 0),       // already consumed
            1 => {
                let b = self.read_bytes(2)?;
                (u16::from_le_bytes([b[0], b[1]]) as u64, 0)
            }
            2 => {
                let b = self.read_bytes(4)?;
                (u32::from_le_bytes(b.try_into().unwrap()) as u64, 0)
            }
            3 => {
                let b = self.read_bytes(8)?;
                (u64::from_le_bytes(b.try_into().unwrap()), 0)
            }
            _ => unreachable!(),
        };
        let _ = size;
        Ok((raw >> 2) as usize)
    }

    /// Parse a section (struct): varint field_count, then N fields.
    fn read_section(&mut self) -> Result<HashMap<String, EpeeValue>, String> {
        let field_count = self.read_epee_varint()?;
        let mut fields = HashMap::new();

        for _ in 0..field_count {
            let name_len = self.read_u8()? as usize;
            let name_bytes = self.read_bytes(name_len)?;
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            let marker = self.read_u8()?;
            let value = self.read_value(marker)?;
            fields.insert(name, value);
        }

        Ok(fields)
    }

    fn read_value(&mut self, marker: u8) -> Result<EpeeValue, String> {
        match marker {
            // U64
            5 => Ok(EpeeValue::U64(self.read_u64_le()?)),
            // STRING / BLOB
            10 => {
                let len = self.read_epee_varint()?;
                let blob = self.read_bytes(len)?;
                Ok(EpeeValue::Blob(blob.to_vec()))
            }
            // STRUCT (nested section)
            12 => {
                let section = self.read_section()?;
                Ok(EpeeValue::Section(section))
            }
            // Skip other types we don't need
            1 => { self.read_bytes(8)?; Ok(EpeeValue::Other) }    // I64
            2 => { self.read_bytes(4)?; Ok(EpeeValue::Other) }    // I32
            3 => { self.read_bytes(2)?; Ok(EpeeValue::Other) }    // I16
            4 => { self.read_bytes(1)?; Ok(EpeeValue::Other) }    // I8
            6 => { self.read_bytes(4)?; Ok(EpeeValue::Other) }    // U32
            7 => { self.read_bytes(2)?; Ok(EpeeValue::Other) }    // U16
            8 => { self.read_bytes(1)?; Ok(EpeeValue::Other) }    // U8
            9 => { self.read_bytes(8)?; Ok(EpeeValue::Other) }    // F64
            11 => { self.read_bytes(1)?; Ok(EpeeValue::Other) }   // BOOL
            // Array types (0x80 | element_type)
            m if m & 0x80 != 0 => {
                let elem_type = m & 0x7f;
                let count = self.read_epee_varint()?;
                for _ in 0..count {
                    self.read_value(elem_type)?;
                }
                Ok(EpeeValue::Other)
            }
            _ => Err(format!("unknown epee marker 0x{marker:02x} at offset {}", self.pos - 1)),
        }
    }
}

fn parse_epee_account_data(data: &[u8]) -> Result<HashMap<String, EpeeValue>, String> {
    if !data.starts_with(EPEE_HEADER) {
        return Err("missing epee header".into());
    }
    let mut cursor = EpeeCursor::new(data);
    cursor.pos = EPEE_HEADER.len();
    cursor.read_section()
}

fn get_blob(fields: &HashMap<String, EpeeValue>, key: &str) -> Result<[u8; 32], String> {
    match fields.get(key) {
        Some(EpeeValue::Blob(v)) if v.len() == 32 => Ok(v.as_slice().try_into().unwrap()),
        Some(EpeeValue::Blob(v)) => Err(format!("{key}: expected 32 bytes, got {}", v.len())),
        _ => Err(format!("{key} not found")),
    }
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

pub fn read_keys_file(path: &Path, password: &str) -> Result<ImportedKeysFile, String> {
    let file_data =
        std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if file_data.len() < 9 {
        return Err("file too small to be a valid .keys file".into());
    }

    let iv: [u8; 8] = file_data[..8].try_into().unwrap();
    let mut offset: usize = 8;
    let ciphertext_len = read_leb128(&file_data, &mut offset)? as usize;
    if offset + ciphertext_len > file_data.len() {
        return Err("ciphertext length exceeds file size".into());
    }

    let mut buf = file_data[offset..offset + ciphertext_len].to_vec();
    let key: [u8; 32] = cryptonight_hash_v0(password.as_bytes());
    let mut cipher = ChaCha20Legacy::new((&key).into(), (&iv).into());
    cipher.apply_keystream(&mut buf);

    if buf.len() < 2 || buf[0] != b'{' || buf[1] != b'"' {
        return Err("decryption produced garbage (wrong password?)".into());
    }

    parse_decrypted_keys(&buf, &key)
}

pub fn decrypt_keys_file(path: &Path, password: &str) -> Result<Vec<u8>, String> {
    let file_data =
        std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if file_data.len() < 9 {
        return Err("file too small to be a valid .keys file".into());
    }

    let iv: [u8; 8] = file_data[..8].try_into().unwrap();
    let mut offset: usize = 8;
    let ciphertext_len = read_leb128(&file_data, &mut offset)? as usize;
    if offset + ciphertext_len > file_data.len() {
        return Err("ciphertext length exceeds file size".into());
    }

    let mut buf = file_data[offset..offset + ciphertext_len].to_vec();
    let key: [u8; 32] = cryptonight_hash_v0(password.as_bytes());
    let mut cipher = ChaCha20Legacy::new((&key).into(), (&iv).into());
    cipher.apply_keystream(&mut buf);

    if buf.len() < 2 || buf[0] != b'{' || buf[1] != b'"' {
        return Err("decryption produced garbage (wrong password?)".into());
    }
    Ok(buf)
}

fn parse_decrypted_keys(plaintext: &[u8], chacha_key: &[u8; 32]) -> Result<ImportedKeysFile, String> {
    let (kd_start, kd_end) = find_json_string_value(plaintext, "key_data")
        .ok_or("key_data field not found")?;

    let key_data_raw = &plaintext[kd_start..kd_end];
    let key_data_bytes = if !key_data_raw.is_empty() && key_data_raw.iter().all(|b| b.is_ascii_hexdigit()) {
        hex::decode(std::str::from_utf8(key_data_raw).unwrap())
            .map_err(|e| format!("hex decode: {e}"))?
    } else {
        unescape_json_bytes(key_data_raw)?
    };

    let root = parse_epee_account_data(&key_data_bytes)?;

    let keys_section = match root.get("m_keys") {
        Some(EpeeValue::Section(s)) => s,
        _ => return Err("m_keys section not found".into()),
    };

    let addr_section = match keys_section.get("m_account_address") {
        Some(EpeeValue::Section(s)) => s,
        _ => return Err("m_account_address section not found".into()),
    };

    let spend_public_key = get_blob(addr_section, "m_spend_public_key")?;
    let view_public_key = get_blob(addr_section, "m_view_public_key")?;
    let mut spend_secret_key = get_blob(keys_section, "m_spend_secret_key")?;
    let mut view_secret_key = get_blob(keys_section, "m_view_secret_key")?;

    let creation_timestamp = match root.get("m_creation_timestamp") {
        Some(EpeeValue::U64(v)) => *v,
        _ => 0,
    };

    // Decrypt secret keys via xor_with_key_stream if they're encrypted
    let encrypted_secret_keys =
        find_json_int_value(plaintext, "encrypted_secret_keys").unwrap_or(0) != 0;

    if encrypted_secret_keys {
        let encryption_iv = match keys_section.get("m_encryption_iv") {
            Some(EpeeValue::Blob(v)) if v.len() == 8 => {
                let iv: [u8; 8] = v.as_slice().try_into().unwrap();
                iv
            }
            _ => return Err("encrypted_secret_keys set but m_encryption_iv not found".into()),
        };

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
    fn epee_varint_values() {
        assert_eq!(EpeeCursor::new(&[0x00]).read_epee_varint().unwrap(), 0);
        assert_eq!(EpeeCursor::new(&[0x04]).read_epee_varint().unwrap(), 1);
        assert_eq!(EpeeCursor::new(&[0x08]).read_epee_varint().unwrap(), 2);
        assert_eq!(EpeeCursor::new(&[0x80]).read_epee_varint().unwrap(), 32);
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
