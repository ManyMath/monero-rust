use monero_rust::wallet_keys_file::{read_keys_file, write_keys_file};
use std::path::{Path, PathBuf};

const HONKED_MNEMONIC: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const HONKED_STAGENET_ADDRESS: &str = "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf";

const VOCAL_MNEMONIC: &str = "vocal either anvil films dolphin zeal bacon cuisine quote syndrome rejoices envy okay pancakes tulips lair greater petals organs enmity dedicated oust thwart tomorrow tomorrow";
const VOCAL_SPEND_KEY: &str = "722bbfcf99a9b2c9e700ce857850dd8c4c94c73dca8d914c603f5fee0e365803";
const VOCAL_VIEW_KEY: &str = "0a1a38f6d246e894600a3e27238a064bf5e8d91801df47a17107596b1378e501";
const VOCAL_STAGENET_ADDRESS: &str = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

fn vector_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors")
        .join(name)
}

#[test]
fn import_honked_keys() {
    let path = vector_path("honked.keys");
    let imported = read_keys_file(&path, "")
        .or_else(|_| read_keys_file(&path, "1"))
        .expect("failed to decrypt honked.keys");

    assert!(!imported.watch_only);

    let mnemonic = imported.mnemonic.as_deref().expect("missing mnemonic");
    assert_eq!(mnemonic, HONKED_MNEMONIC);

    let address = monero_rust::derive_address(mnemonic, monero_rust::Network::Stagenet, "")
        .expect("address derivation failed");
    assert_eq!(address.to_string(), HONKED_STAGENET_ADDRESS);

    let keys = monero_rust::derive_keys(mnemonic, monero_rust::Network::Stagenet, "").unwrap();
    assert_eq!(imported.spend_public_key, keys.public_spend_key);
    assert_eq!(imported.view_public_key, keys.public_view_key);
}

#[test]
fn import_vocal_keys() {
    let path = vector_path("vocal.keys");
    let imported = read_keys_file(&path, "")
        .or_else(|_| read_keys_file(&path, "1"))
        .expect("failed to decrypt vocal.keys");

    assert!(!imported.watch_only);

    assert_eq!(hex::encode(imported.spend_secret_key), VOCAL_SPEND_KEY);
    assert_eq!(hex::encode(imported.view_secret_key), VOCAL_VIEW_KEY);

    let mnemonic = imported.mnemonic.as_deref().expect("missing mnemonic");
    assert_eq!(mnemonic, VOCAL_MNEMONIC);

    let address = monero_rust::derive_address(mnemonic, monero_rust::Network::Stagenet, "")
        .expect("address derivation failed");
    assert_eq!(address.to_string(), VOCAL_STAGENET_ADDRESS);
}

#[test]
fn import_view_only_wallet() {
    let path = vector_path("hemlock_view_only.keys");
    let imported = read_keys_file(&path, "")
        .or_else(|_| read_keys_file(&path, "1"))
        .expect("failed to decrypt hemlock_view_only.keys");

    assert!(imported.watch_only);
    assert!(imported.mnemonic.is_none());
    assert_eq!(imported.spend_secret_key, [0u8; 32]);
    assert_ne!(imported.view_secret_key, [0u8; 32]);
}

fn assert_roundtrip(name: &str) {
    let original = read_keys_file(&vector_path(name), "").unwrap();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_keys_file(tmp.path(), "", &original).unwrap();

    let reimported = read_keys_file(tmp.path(), "").unwrap();

    assert_eq!(original.spend_secret_key, reimported.spend_secret_key);
    assert_eq!(original.view_secret_key, reimported.view_secret_key);
    assert_eq!(original.spend_public_key, reimported.spend_public_key);
    assert_eq!(original.view_public_key, reimported.view_public_key);
    assert_eq!(original.creation_timestamp, reimported.creation_timestamp);
    assert_eq!(original.watch_only, reimported.watch_only);
    assert_eq!(original.seed_language, reimported.seed_language);
    assert_eq!(original.mnemonic, reimported.mnemonic);
    assert_eq!(original.nettype, reimported.nettype);
}

#[test]
fn roundtrip_honked() {
    assert_roundtrip("honked.keys");
}

#[test]
fn roundtrip_vocal() {
    assert_roundtrip("vocal.keys");
}

#[test]
fn roundtrip_view_only() {
    assert_roundtrip("hemlock_view_only.keys");
}

fn assert_bit_exact(name: &str) {
    let original_path = vector_path(name);
    let original_bytes = std::fs::read(&original_path).unwrap();

    let imported = read_keys_file(&original_path, "").unwrap();

    let written_bytes =
        monero_rust::wallet_keys_file::encrypt_keys_data_with_file_ivs("", &imported, 1).unwrap();
    assert_eq!(
        original_bytes, written_bytes,
        "{name}: written file differs from original"
    );

    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_keys_file(tmp.path(), "", &imported).unwrap();
    let fresh = std::fs::read(tmp.path()).unwrap();
    assert_ne!(fresh[..8], original_bytes[..8], "{name}: outer IV reused");
    let reread = read_keys_file(tmp.path(), "").unwrap();
    assert_ne!(
        reread.encryption_iv, imported.encryption_iv,
        "{name}: inner IV reused"
    );
    assert_eq!(reread.spend_secret_key, imported.spend_secret_key);
    assert_eq!(reread.view_secret_key, imported.view_secret_key);
}

#[test]
fn bit_exact_honked() {
    assert_bit_exact("honked.keys");
}

#[test]
fn bit_exact_vocal() {
    assert_bit_exact("vocal.keys");
}

#[test]
fn bit_exact_hemlock_view_only() {
    assert_bit_exact("hemlock_view_only.keys");
}

/// Reject a ciphertext length that overflows when added to the data offset.
/// An untrusted LEB128 length must return a parse error before slicing, in
/// both debug and release builds.
#[test]
fn rejects_a_keys_file_whose_length_overflows() {
    // Write the 8-byte outer IV followed by u64::MAX encoded as LEB128.
    let mut file = vec![0u8; 8];
    file.extend_from_slice(&[0xFF; 9]);
    file.push(0x01);

    let err = monero_rust::wallet_keys_file::decrypt_keys_data(&file, "")
        .expect_err("an out-of-range ciphertext length must be rejected");
    assert!(err.contains("ciphertext length"), "unexpected error: {err}");
}

/// Read fixture plaintext and the keys needed to parse it, allowing tests
/// to modify the JSON after decryption.
fn decrypt_vector(name: &str) -> (Vec<u8>, [u8; 32], [u8; 8]) {
    let bytes = std::fs::read(vector_path(name)).expect("failed to read vector");
    let (plaintext, key, iv) = monero_rust::wallet_keys_file::decrypt_keys_data(&bytes, "")
        .or_else(|_| monero_rust::wallet_keys_file::decrypt_keys_data(&bytes, "1"))
        .expect("failed to decrypt vector");
    (plaintext.to_vec(), *key, iv)
}

/// Change one ASCII digit without changing the plaintext length.
fn retarget_int_field(plaintext: &[u8], field: &str, from: u8, to: u8) -> Vec<u8> {
    let needle = format!("\"{field}\":{}", from as char);
    let pos = plaintext
        .windows(needle.len())
        .position(|w| w == needle.as_bytes())
        .unwrap_or_else(|| panic!("{needle} not found in the decrypted keys file"));
    let mut tampered = plaintext.to_vec();
    tampered[pos + needle.len() - 1] = to;
    tampered
}

/// Reject secret keys that do not match the file's public keys.
///
/// Change `encrypted_secret_keys` without changing the stored key data. The
/// file still parses as JSON, but the flag makes the importer interpret encrypted
/// bytes as plaintext keys. As in `wallet2::load_keys`, key verification must
/// catch the mismatch before returning an incorrect wallet.
#[test]
fn rejects_a_keys_file_whose_secret_keys_do_not_match_its_public_keys() {
    let (plaintext, key, outer_iv) = decrypt_vector("honked.keys");

    let honest = monero_rust::wallet_keys_file::parse_decrypted_keys(&plaintext, &key, outer_iv)
        .expect("the untampered vector should parse");
    assert_eq!(honest.mnemonic.as_deref(), Some(HONKED_MNEMONIC));

    // Clear the flag even though the secret keys remain encrypted.
    let tampered = retarget_int_field(&plaintext, "encrypted_secret_keys", b'1', b'0');

    let err = monero_rust::wallet_keys_file::parse_decrypted_keys(&tampered, &key, outer_iv)
        .expect_err("secret keys that do not match the file's public keys must be rejected");
    assert!(err.contains("do not match"), "unexpected error: {err}");
}

/// Accept a view-only file with an absent spend secret key. Monero verifies
/// its view key but skips the spend-key check.
#[test]
fn accepts_a_view_only_keys_file_whose_spend_secret_key_is_absent() {
    let (plaintext, key, outer_iv) = decrypt_vector("hemlock_view_only.keys");
    let imported = monero_rust::wallet_keys_file::parse_decrypted_keys(&plaintext, &key, outer_iv)
        .expect("a view-only keys file must still parse");
    assert!(imported.watch_only);
    assert_eq!(imported.spend_secret_key, [0u8; 32]);
}

/// Replace the first `from` match with `to`; panic if no match exists.
fn splice_once(plaintext: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let pos = plaintext
        .windows(from.len())
        .position(|w| w == from)
        .unwrap_or_else(|| {
            panic!(
                "{} not found in the decrypted keys file",
                String::from_utf8_lossy(from)
            )
        });
    let mut out = plaintext[..pos].to_vec();
    out.extend_from_slice(to);
    out.extend_from_slice(&plaintext[pos + from.len()..]);
    out
}

fn first_occurrence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reject malformed JSON containing a decoy field before the real member.
///
/// Closing a string early can expose field-like text that a substring search
/// would accept. The importer must parse the object and reject invalid syntax,
/// as Monero does with rapidjson.
#[test]
fn refuses_a_keys_file_with_a_decoy_field_smuggled_past_a_string() {
    let (plaintext, key, outer_iv) = decrypt_vector("honked.keys");

    let honest = monero_rust::wallet_keys_file::parse_decrypted_keys(&plaintext, &key, outer_iv)
        .expect("the untampered vector should parse");
    assert_eq!(
        honest.nettype, 2,
        "premise: honked.keys is a stagenet wallet"
    );

    // Close `seed_language` early and insert a decoy `nettype`.
    let tampered = splice_once(
        &plaintext,
        br#""seed_language":"English""#,
        br#""seed_language":"a"nettype":1,"b""#,
    );

    // Confirm that a substring search would encounter the decoy first.
    let decoy = first_occurrence(&tampered, br#""nettype":1"#).expect("decoy must be present");
    let real = first_occurrence(&tampered, br#""nettype":2"#).expect("real member must survive");
    assert!(
        decoy < real,
        "the decoy must precede the real member for this to be the bug under test"
    );

    let err = monero_rust::wallet_keys_file::parse_decrypted_keys(&tampered, &key, outer_iv)
        .expect_err("a keys file whose JSON is malformed must be refused");
    assert!(
        err.contains("well-formed"),
        "the malformed document must be what rejected it, got: {err}"
    );
}

/// Still verify the view key in watch-only files. Changing the encryption flag
/// must fail import even though the spend-key check is skipped.
#[test]
fn rejects_a_view_only_keys_file_whose_view_key_does_not_match() {
    let (plaintext, key, outer_iv) = decrypt_vector("hemlock_view_only.keys");
    let tampered = retarget_int_field(&plaintext, "encrypted_secret_keys", b'1', b'0');

    let err = monero_rust::wallet_keys_file::parse_decrypted_keys(&tampered, &key, outer_iv)
        .expect_err("a view-only file whose view key does not match must be rejected");
    assert!(
        err.contains("view secret and public keys do not match"),
        "the view-key half of the check must be what rejected it, got: {err}"
    );
}

#[test]
fn independent_view_key_does_not_offer_incomplete_mnemonic_backup() {
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};

    let mut wallet = read_keys_file(&vector_path("vocal.keys"), "").unwrap();
    let independent_view = Scalar::from(42u64);
    wallet.view_secret_key = independent_view.to_bytes();
    wallet.view_public_key = (&independent_view * ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    write_keys_file(tmp.path(), "", &wallet).unwrap();
    let imported = read_keys_file(tmp.path(), "").unwrap();
    assert_eq!(imported.spend_secret_key, wallet.spend_secret_key);
    assert_eq!(imported.view_secret_key, wallet.view_secret_key);
    assert_eq!(imported.spend_public_key, wallet.spend_public_key);
    assert_eq!(imported.view_public_key, wallet.view_public_key);
    assert!(!imported.watch_only);
    assert!(imported.mnemonic.is_none());
}
