//! Reader and writer for monero-wallet-cli `.keys` files.
//!
//! File format: `[8-byte outer IV] [LEB128 ciphertext_len] [ciphertext]`
//!
//! The ciphertext, once decrypted with ChaCha20-legacy keyed by
//! `cn_slow_hash(password)`, is JSON containing a `key_data` field
//! (epee-serialized account data) plus wallet configuration fields.
//!
//! Secret keys within the epee blob may be encrypted with a second
//! ("inner") ChaCha20 layer derived from the outer key.

use crate::seed_compat::{Language, Seed};
use chacha20::ChaCha20Legacy;
use cipher::{KeyIvInit, StreamCipher};
use cuprate_cryptonight::cryptonight_hash_v0;
use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

/// Derive a public key only from a canonical scalar, matching Monero's
/// `crypto::secret_key_to_public_key` and its `sc_check`.
fn public_key_for(secret_key: &[u8; 32]) -> Option<[u8; 32]> {
    let scalar: Option<Scalar> = Scalar::from_canonical_bytes(*secret_key).into();
    Some((&scalar? * ED25519_BASEPOINT_TABLE).compress().to_bytes())
}

/// Verify decrypted secret keys against the public keys stored in the file.
///
/// Check the view key for every wallet and the spend key for full wallets, as
/// `wallet2::load_keys` does. The file's `encrypted_secret_keys` flag controls
/// decryption, and recognizing a JSON prefix does not establish that decryption
/// succeeded. A key mismatch must fail the import before deriving an address
/// or mnemonic from incorrect keys.
fn verify_secret_keys_match_public_keys(
    spend_secret_key: &[u8; 32],
    view_secret_key: &[u8; 32],
    spend_public_key: &[u8; 32],
    view_public_key: &[u8; 32],
    watch_only: bool,
) -> Result<(), String> {
    if public_key_for(view_secret_key).as_ref() != Some(view_public_key) {
        return Err(
            "keys file is invalid: its view secret and public keys do not match".to_string(),
        );
    }
    // Watch-only files store zeroes in place of a secret spend key. Skip its check.
    if !watch_only && public_key_for(spend_secret_key).as_ref() != Some(spend_public_key) {
        return Err(
            "keys file is invalid: its spend secret and public keys do not match".to_string(),
        );
    }
    Ok(())
}

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
    pub encryption_iv: [u8; 8],
    pub outer_iv: [u8; 8],
    pub nettype: u8,
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

fn write_leb128(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
    out
}

fn skip_json_whitespace(data: &[u8], mut i: usize) -> usize {
    while i < data.len() && matches!(data[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Return the position after the closing quote of the JSON string at `i`.
fn json_string_end(data: &[u8], i: usize) -> Option<usize> {
    if *data.get(i)? != b'"' {
        return None;
    }
    let mut j = i + 1;
    while j < data.len() {
        match data[j] {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// Return the position after the JSON value at `i`.
fn json_value_end(data: &[u8], i: usize) -> Option<usize> {
    match *data.get(i)? {
        b'"' => json_string_end(data, i),
        b'{' | b'[' => {
            let mut depth: usize = 0;
            let mut j = i;
            while j < data.len() {
                match data[j] {
                    // Skip strings whole so embedded braces do not affect nesting depth.
                    b'"' => j = json_string_end(data, j)?,
                    b'{' | b'[' => {
                        depth += 1;
                        j += 1;
                    }
                    b'}' | b']' => {
                        // Reject unmatched closing delimiters without underflow,
                        // including in release builds.
                        depth = depth.checked_sub(1)?;
                        j += 1;
                        if depth == 0 {
                            return Some(j);
                        }
                    }
                    _ => j += 1,
                }
            }
            None
        }
        // Numbers and literals end at the next structural byte.
        _ => {
            let mut j = i;
            while j < data.len()
                && !matches!(data[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

/// A top-level JSON member referencing the original plaintext.
struct JsonMember<'a> {
    name: &'a [u8],
    /// Value bytes, including quotes for strings.
    value: &'a [u8],
}

/// Read top-level members of the keys file's JSON object in file order.
///
/// Parse the object structure before looking up names. The `key_data` string
/// contains an Epee account blob and may include bytes resembling field names;
/// those must not be treated as top-level members.
///
/// Reject malformed JSON, as Monero does when loading keys through rapidjson.
fn parse_top_level_object(data: &[u8]) -> Result<Vec<JsonMember<'_>>, String> {
    let malformed = || "keys file JSON is not a well-formed object".to_string();

    let mut members = Vec::new();
    let mut i = skip_json_whitespace(data, 0);
    if data.get(i) != Some(&b'{') {
        return Err(malformed());
    }
    i = skip_json_whitespace(data, i + 1);
    if data.get(i) == Some(&b'}') {
        i += 1;
        return if skip_json_whitespace(data, i) == data.len() {
            Ok(members)
        } else {
            Err(malformed())
        };
    }

    loop {
        let name_end = json_string_end(data, i).ok_or_else(malformed)?;
        let name = data.get(i + 1..name_end - 1).ok_or_else(malformed)?;

        i = skip_json_whitespace(data, name_end);
        if data.get(i) != Some(&b':') {
            return Err(malformed());
        }
        i = skip_json_whitespace(data, i + 1);

        let value_end = json_value_end(data, i).ok_or_else(malformed)?;
        members.push(JsonMember {
            name,
            value: data.get(i..value_end).ok_or_else(malformed)?,
        });

        i = skip_json_whitespace(data, value_end);
        match data.get(i) {
            Some(&b',') => i = skip_json_whitespace(data, i + 1),
            Some(&b'}') => {
                return if skip_json_whitespace(data, i + 1) == data.len() {
                    Ok(members)
                } else {
                    Err(malformed())
                };
            }
            _ => return Err(malformed()),
        }
    }
}

/// Return the first matching member, matching rapidjson's handling of duplicates.
fn member<'a, 'b>(members: &'b [JsonMember<'a>], name: &str) -> Option<&'b JsonMember<'a>> {
    members.iter().find(|m| m.name == name.as_bytes())
}

/// Return an escaped top-level string value, or `None` if absent or not a string.
fn string_member<'a>(members: &[JsonMember<'a>], name: &str) -> Option<&'a [u8]> {
    let value = member(members, name)?.value;
    if value.len() < 2 || value[0] != b'"' || value[value.len() - 1] != b'"' {
        return None;
    }
    Some(&value[1..value.len() - 1])
}

fn int_member(members: &[JsonMember<'_>], name: &str) -> Option<u64> {
    let digits = member(members, name)?.value;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

fn unescape_json_bytes(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\\' {
            i += 1;
            if i >= data.len() {
                return Err("trailing backslash".into());
            }
            match data[i] {
                b'"' => {
                    out.push(b'"');
                    i += 1;
                }
                b'\\' => {
                    out.push(b'\\');
                    i += 1;
                }
                b'/' => {
                    out.push(b'/');
                    i += 1;
                }
                b'b' => {
                    out.push(0x08);
                    i += 1;
                }
                b'f' => {
                    out.push(0x0C);
                    i += 1;
                }
                b'n' => {
                    out.push(b'\n');
                    i += 1;
                }
                b'r' => {
                    out.push(b'\r');
                    i += 1;
                }
                b't' => {
                    out.push(b'\t');
                    i += 1;
                }
                b'u' => {
                    i += 1;
                    if i + 4 > data.len() {
                        return Err("truncated \\uXXXX".into());
                    }
                    let hex_str =
                        std::str::from_utf8(&data[i..i + 4]).map_err(|_| "invalid \\uXXXX")?;
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

/// Escape binary data into a JSON string value, matching Monero's rapidjson output.
fn escape_json_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    for &b in data {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0C => out.extend_from_slice(b"\\f"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            c if c < 0x20 => {
                out.extend_from_slice(format!("\\u{:04X}", c).as_bytes());
            }
            c => out.push(c),
        }
    }
    out
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

pub fn decrypt_keys_data(
    file_bytes: &[u8],
    password: &str,
) -> Result<(Vec<u8>, [u8; 32], [u8; 8]), String> {
    if file_bytes.len() < 9 {
        return Err("file too small to be a valid .keys file".into());
    }

    let outer_iv: [u8; 8] = file_bytes[..8].try_into().unwrap();
    let mut offset: usize = 8;
    // Check the addition: the file's length varint may overflow before the
    // bounds check or the slice below.
    let ciphertext_len = read_leb128(file_bytes, &mut offset)?;
    let end = usize::try_from(ciphertext_len)
        .ok()
        .and_then(|len| offset.checked_add(len))
        .filter(|end| *end <= file_bytes.len())
        .ok_or("ciphertext length exceeds file size")?;

    let mut buf = file_bytes[offset..end].to_vec();
    let key: [u8; 32] = cryptonight_hash_v0(password.as_bytes());
    let mut cipher = ChaCha20Legacy::new((&key).into(), (&outer_iv).into());
    cipher.apply_keystream(&mut buf);

    if buf.len() < 2 || buf[0] != b'{' || buf[1] != b'"' {
        return Err("decryption produced garbage (wrong password?)".into());
    }

    Ok((buf, key, outer_iv))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_keys_file(path: &std::path::Path, password: &str) -> Result<ImportedKeysFile, String> {
    let file_data =
        std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let (plaintext, key, outer_iv) = decrypt_keys_data(&file_data, password)?;
    parse_decrypted_keys(&plaintext, &key, outer_iv)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn decrypt_keys_file(path: &std::path::Path, password: &str) -> Result<Vec<u8>, String> {
    let file_data =
        std::fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let (plaintext, _key, _iv) = decrypt_keys_data(&file_data, password)?;
    Ok(plaintext)
}

pub fn encrypt_keys_data(password: &str, wallet: &ImportedKeysFile) -> Result<Vec<u8>, String> {
    let key: [u8; 32] = cryptonight_hash_v0(password.as_bytes());

    let (encrypted_spend, encrypted_view) = inner_encrypt_keys(
        &wallet.spend_secret_key,
        &wallet.view_secret_key,
        &key,
        &wallet.encryption_iv,
    );

    let acct = EpeeAccountBase {
        m_creation_timestamp: wallet.creation_timestamp,
        m_keys: EpeeAccountKeys {
            m_account_address: EpeeAccountAddress {
                m_spend_public_key: wallet.spend_public_key.to_vec(),
                m_view_public_key: wallet.view_public_key.to_vec(),
            },
            m_encryption_iv: wallet.encryption_iv.to_vec(),
            m_spend_secret_key: encrypted_spend.to_vec(),
            m_view_secret_key: encrypted_view.to_vec(),
        },
    };
    let epee_bytes =
        monero_epee_bin_serde::to_bytes(&acct).map_err(|e| format!("epee serialize: {e}"))?;

    let plaintext = build_plaintext_json(&epee_bytes, wallet);

    let mut ciphertext = plaintext;
    let mut cipher = ChaCha20Legacy::new((&key).into(), (&wallet.outer_iv).into());
    cipher.apply_keystream(&mut ciphertext);

    let len_bytes = write_leb128(ciphertext.len() as u64);
    let mut file_data = Vec::with_capacity(8 + len_bytes.len() + ciphertext.len());
    file_data.extend_from_slice(&wallet.outer_iv);
    file_data.extend_from_slice(&len_bytes);
    file_data.extend_from_slice(&ciphertext);

    Ok(file_data)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write_keys_file(
    path: &std::path::Path,
    password: &str,
    wallet: &ImportedKeysFile,
) -> Result<(), String> {
    let file_data = encrypt_keys_data(password, wallet)?;
    std::fs::write(path, &file_data).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn inner_encrypt_keys(
    spend: &[u8; 32],
    view: &[u8; 32],
    chacha_key: &[u8; 32],
    encryption_iv: &[u8; 8],
) -> ([u8; 32], [u8; 32]) {
    let mut derive_input = [0u8; 33];
    derive_input[..32].copy_from_slice(chacha_key);
    derive_input[32] = 0x6b; // HASH_KEY_MEMORY
    let derived_key: [u8; 32] = cryptonight_hash_v0(&derive_input);

    let mut keystream = [0u8; 64];
    let mut cipher = ChaCha20Legacy::new((&derived_key).into(), (encryption_iv).into());
    cipher.apply_keystream(&mut keystream);

    let mut enc_spend = *spend;
    let mut enc_view = *view;
    for i in 0..32 {
        enc_spend[i] ^= keystream[i];
    }
    for i in 0..32 {
        enc_view[i] ^= keystream[32 + i];
    }
    (enc_spend, enc_view)
}

// Monero v0.18 wallet2::get_keys_file_data config defaults
fn build_plaintext_json(epee_key_data: &[u8], wallet: &ImportedKeysFile) -> Vec<u8> {
    let mut j = Vec::with_capacity(2048);
    j.push(b'{');

    j.extend_from_slice(b"\"key_data\":\"");
    j.extend_from_slice(&escape_json_bytes(epee_key_data));
    j.push(b'"');

    if let Some(ref lang) = wallet.seed_language {
        write_json_str(&mut j, "seed_language", lang);
    }

    write_json_int(&mut j, "key_on_device", 0);
    write_json_int(&mut j, "watch_only", if wallet.watch_only { 1 } else { 0 });
    write_json_int(&mut j, "multisig", 0);
    write_json_int(&mut j, "multisig_threshold", 0);
    write_json_int(&mut j, "always_confirm_transfers", 1);
    write_json_int(&mut j, "print_ring_members", 0);
    write_json_int(&mut j, "store_tx_info", 1);
    write_json_int(&mut j, "default_mixin", 0);
    write_json_int(&mut j, "default_priority", 0);
    write_json_int(&mut j, "auto_refresh", 1);
    write_json_int(&mut j, "refresh_type", 1);
    write_json_int(&mut j, "refresh_height", 0);
    write_json_int(&mut j, "skip_to_height", 0);
    write_json_int(&mut j, "confirm_non_default_ring_size", 1);
    write_json_int(&mut j, "ask_password", 2);
    write_json_int(&mut j, "max_reorg_depth", 100);
    write_json_int(&mut j, "min_output_count", 0);
    write_json_int(&mut j, "min_output_value", 0);
    write_json_int(&mut j, "default_decimal_point", 12);
    write_json_int(&mut j, "merge_destinations", 0);
    write_json_int(&mut j, "confirm_backlog", 1);
    write_json_int(&mut j, "confirm_backlog_threshold", 0);
    write_json_int(&mut j, "confirm_export_overwrite", 1);
    write_json_int(&mut j, "auto_low_priority", 1);
    write_json_int(&mut j, "nettype", wallet.nettype as u64);
    write_json_int(&mut j, "segregate_pre_fork_outputs", 1);
    write_json_int(&mut j, "key_reuse_mitigation2", 1);
    write_json_int(&mut j, "segregation_height", 0);
    write_json_int(&mut j, "ignore_fractional_outputs", 1);
    write_json_int(&mut j, "ignore_outputs_above", u64::MAX);
    write_json_int(&mut j, "ignore_outputs_below", 0);
    write_json_int(&mut j, "track_uses", 0);
    write_json_int(&mut j, "background_sync_type", 0);
    write_json_int(&mut j, "show_wallet_name_when_locked", 0);
    write_json_int(&mut j, "inactivity_lock_timeout", 90);
    write_json_int(&mut j, "setup_background_mining", 1);
    write_json_int(&mut j, "subaddress_lookahead_major", 50);
    write_json_int(&mut j, "subaddress_lookahead_minor", 200);
    write_json_int(&mut j, "original_keys_available", 0);
    write_json_int(&mut j, "export_format", 0);
    write_json_int(&mut j, "load_deprecated_formats", 0);
    write_json_int(&mut j, "encrypted_secret_keys", 1);
    write_json_str(&mut j, "device_name", "");
    write_json_str(&mut j, "device_derivation_path", "");
    write_json_int(&mut j, "persistent_rpc_client_id", 0);
    j.extend_from_slice(b",\"auto_mine_for_rpc_payment\":-1.0");
    write_json_int(&mut j, "credits_target", 0);
    write_json_int(&mut j, "enable_multisig", 0);

    j.push(b'}');
    j
}

fn write_json_int(buf: &mut Vec<u8>, key: &str, value: u64) {
    buf.push(b',');
    buf.push(b'"');
    buf.extend_from_slice(key.as_bytes());
    buf.extend_from_slice(b"\":");
    buf.extend_from_slice(value.to_string().as_bytes());
}

fn write_json_str(buf: &mut Vec<u8>, key: &str, value: &str) {
    buf.push(b',');
    buf.push(b'"');
    buf.extend_from_slice(key.as_bytes());
    buf.extend_from_slice(b"\":\"");
    // Escape `seed_language` when writing it back. Imported values may contain
    // quotes that would otherwise end the string and introduce JSON members.
    buf.extend_from_slice(&escape_json_bytes(value.as_bytes()));
    buf.push(b'"');
}

pub fn parse_decrypted_keys(
    plaintext: &[u8],
    chacha_key: &[u8; 32],
    outer_iv: [u8; 8],
) -> Result<ImportedKeysFile, String> {
    let members = parse_top_level_object(plaintext)?;

    let key_data_raw = string_member(&members, "key_data").ok_or("key_data field not found")?;
    let key_data_bytes =
        if !key_data_raw.is_empty() && key_data_raw.iter().all(|b| b.is_ascii_hexdigit()) {
            hex::decode(std::str::from_utf8(key_data_raw).unwrap())
                .map_err(|e| format!("hex decode: {e}"))?
        } else {
            unescape_json_bytes(key_data_raw)?
        };

    let acct: EpeeAccountBase = crate::safe_epee::from_bytes(&key_data_bytes)
        .map_err(|e| format!("epee deserialize: {e}"))?;

    let spend_public_key = to_key(
        &acct.m_keys.m_account_address.m_spend_public_key,
        "m_spend_public_key",
    )?;
    let view_public_key = to_key(
        &acct.m_keys.m_account_address.m_view_public_key,
        "m_view_public_key",
    )?;
    let mut spend_secret_key = to_key(&acct.m_keys.m_spend_secret_key, "m_spend_secret_key")?;
    let mut view_secret_key = to_key(&acct.m_keys.m_view_secret_key, "m_view_secret_key")?;
    let creation_timestamp = acct.m_creation_timestamp;

    let encryption_iv: [u8; 8] =
        acct.m_keys
            .m_encryption_iv
            .as_slice()
            .try_into()
            .map_err(|_| {
                format!(
                    "m_encryption_iv: expected 8 bytes, got {}",
                    acct.m_keys.m_encryption_iv.len()
                )
            })?;

    // Decrypt secret keys via xor_with_key_stream if they're encrypted
    let encrypted_secret_keys = int_member(&members, "encrypted_secret_keys").unwrap_or(0) != 0;

    if encrypted_secret_keys {
        let mut derive_input = [0u8; 33];
        derive_input[..32].copy_from_slice(chacha_key);
        derive_input[32] = 0x6b;
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

    let watch_only = int_member(&members, "watch_only").unwrap_or(0) != 0;

    verify_secret_keys_match_public_keys(
        &spend_secret_key,
        &view_secret_key,
        &spend_public_key,
        &view_public_key,
        watch_only,
    )?;

    let nettype = int_member(&members, "nettype").unwrap_or(0) as u8;
    let seed_language = string_member(&members, "seed_language")
        .and_then(|value| std::str::from_utf8(value).ok().map(|s| s.to_string()));

    let derived_view = Zeroizing::new(Scalar::from_bytes_mod_order(
        Keccak256::digest(spend_secret_key).into(),
    ));
    let mnemonic = if !watch_only && derived_view.to_bytes() == view_secret_key {
        let lang = seed_language
            .as_deref()
            .and_then(parse_language)
            .unwrap_or(Language::English);
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
        encryption_iv,
        outer_iv,
        nettype,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_epee_rejects_truncation_and_recursive_unknown_fields() {
        let valid = hex::decode("0111010101010201010401780c040179052a00000000000000").unwrap();
        let parse = |wire: &[u8]| {
            let plaintext = format!(r#"{{"key_data":"{}"}}"#, hex::encode(wire));
            parse_decrypted_keys(plaintext.as_bytes(), &[0; 32], [0; 8])
        };
        for end in 0..valid.len() {
            assert!(parse(&valid[..end]).is_err());
        }
        let mut deep = valid[..9].to_vec();
        for _ in 0..1000 {
            deep.extend_from_slice(&[4, 1, b'x', 12]);
        }
        deep.push(0);
        assert!(parse(&deep).unwrap_err().contains("nesting"));
    }

    #[test]
    fn leb128_roundtrip() {
        let data = [0xAC, 0x02];
        let mut off = 0;
        assert_eq!(read_leb128(&data, &mut off).unwrap(), 300);
        assert_eq!(write_leb128(300), vec![0xAC, 0x02]);
    }

    #[test]
    fn unescape_control_chars() {
        let input = br#"\u0001\u0011hello"#;
        let out = unescape_json_bytes(input).unwrap();
        assert_eq!(&out, &[0x01, 0x11, b'h', b'e', b'l', b'l', b'o']);
    }

    #[test]
    fn escape_roundtrip() {
        let original: Vec<u8> = (0..=255).collect();
        let escaped = escape_json_bytes(&original);
        let unescaped = unescape_json_bytes(&escaped).unwrap();
        assert_eq!(original, unescaped);
    }

    #[test]
    fn find_field_in_json() {
        let json = br#"{"foo":"bar","baz":"qux"}"#;
        let members = parse_top_level_object(json).unwrap();
        assert_eq!(string_member(&members, "baz"), Some(&b"qux"[..]));
    }

    /// Ignore field-like text inside `key_data`. Lookups must match only
    /// top-level member names, regardless of where a string appears in the file.
    #[test]
    fn a_field_name_inside_a_string_value_is_not_a_field() {
        let json = br#"{"key_data":"junk\"nettype\":1junk","nettype":2}"#;
        let members = parse_top_level_object(json).unwrap();
        assert_eq!(int_member(&members, "nettype"), Some(2));
        assert_eq!(members.len(), 2, "the decoy must not become a member");
    }

    /// Reject malformed JSON that closes `key_data` early and inserts a decoy
    /// `nettype` before the real member. Monero rejects this during JSON parsing.
    #[test]
    fn refuses_a_document_that_is_not_a_flat_object() {
        for bad in [
            &br#"{"key_data":"junk"nettype":1,"nettype":2}"#[..],
            &br#"{"a":1"#[..],
            &br#"{"a":1}trailing"#[..],
            &br#"["a",1]"#[..],
            &br#"{"a" 1}"#[..],
            &br#"{"a":}"#[..],
        ] {
            assert!(
                parse_top_level_object(bad).is_err(),
                "accepted a malformed document: {}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    /// Skip nested values without treating their braces or names as top-level fields.
    #[test]
    fn skips_nested_values_whole() {
        let json = br#"{"a":{"nettype":1,"b":[1,{"c":"}"}]},"nettype":2}"#;
        let members = parse_top_level_object(json).unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(int_member(&members, "nettype"), Some(2));
    }

    /// Escape imported `seed_language` values when writing JSON.
    #[test]
    fn written_string_values_are_escaped() {
        let mut written = Vec::new();
        write_json_str(&mut written, "seed_language", "E\",\"nettype\":1,\"x\":\"y");

        // The helper includes the comma separating this member from the previous one.
        let mut json = vec![b'{'];
        json.extend_from_slice(&written[1..]);
        json.extend_from_slice(b",\"nettype\":2}");

        let members = parse_top_level_object(&json)
            .expect("an escaped value must still leave a well-formed object");
        assert_eq!(int_member(&members, "nettype"), Some(2));
        assert_eq!(members.len(), 2, "the value must not have become members");
    }
}
