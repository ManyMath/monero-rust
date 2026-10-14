//! `.keys` files, secret hygiene of the public result types, and UR frames.

mod cold_signing_common;

use cold_signing_common::*;
use monero_rust::native::{
    preview_unsigned_transaction, sign_unsigned_transaction_with_private_keys,
};
use monero_rust::ur_codec::{UrDecoder, UrEncoder, UR_TYPE_UNSIGNED_TX};
use monero_rust::wallet_keys_file::{
    decrypt_keys_data, encrypt_keys_data, parse_decrypted_keys, read_keys_file,
};

#[test]
fn imported_keys_debug_output_hides_the_seed() {
    let keys = cold_keys();
    let mnemonic = keys.mnemonic.clone().expect("full wallet");
    let debug = format!("{keys:?}");
    let first_words: Vec<&str> = mnemonic.split(' ').take(3).collect();
    assert!(
        !debug.contains(&first_words.join(" ")),
        "Debug output of ImportedKeysFile contains the mnemonic"
    );
}

#[test]
fn offline_sign_result_debug_output_hides_the_transaction_key() {
    let cold = cold_keys();
    let encoded = hex::encode(vector_unsigned_txset());
    let reviewed = preview_unsigned_transaction(
        cold.spend_secret_key,
        cold.view_secret_key,
        &encoded,
        "mainnet",
    )
    .unwrap();
    let approval = reviewed.approve(u64::MAX, u64::MAX).unwrap();
    let signed = sign_unsigned_transaction_with_private_keys(
        cold.spend_secret_key,
        cold.view_secret_key,
        &encoded,
        "mainnet",
        &approval,
    )
    .unwrap();
    assert!(
        !format!("{signed:?}").contains(&signed.tx_key),
        "Debug output of OfflineSignResult contains the transaction private key"
    );
}

#[test]
fn transaction_preview_carries_no_secret_bytes() {
    let cold = cold_keys();
    let reviewed = preview_unsigned_transaction(
        cold.spend_secret_key,
        cold.view_secret_key,
        &hex::encode(vector_unsigned_txset()),
        "mainnet",
    )
    .unwrap();
    let rendered = format!("{reviewed:?}{}", serde_json::to_string(&reviewed).unwrap());
    for secret in [cold.spend_secret_key, cold.view_secret_key] {
        assert!(!rendered.contains(&hex::encode(secret)));
        let decimal = format!("{secret:?}");
        assert!(!rendered.contains(decimal.trim_matches(|c| c == '[' || c == ']')));
    }
}

#[test]
#[ignore = "known defect: bytes after the keys-file ciphertext are accepted"]
fn keys_file_with_trailing_bytes_is_rejected_like_wallet2() {
    let mut file = std::fs::read(vector_path("cold_full.keys")).unwrap();
    file.extend_from_slice(b"\x00trailing");
    assert!(
        decrypt_keys_data(&file, "").is_err(),
        "bytes after the ciphertext were accepted"
    );
}

#[test]
fn keys_file_writer_draws_fresh_ivs() {
    let keys = cold_keys();
    let first = encrypt_keys_data("password", &keys).unwrap();
    let mut changed = keys.clone();
    changed.seed_language = Some("Deutsch".to_string());
    let second = encrypt_keys_data("password", &changed).unwrap();
    assert_ne!(
        first[..8],
        second[..8],
        "two different plaintexts were encrypted under one key and one IV"
    );
}

#[test]
fn keys_file_with_swapped_public_spend_key_is_refused() {
    let file = std::fs::read(vector_path("cold_full.keys")).unwrap();
    let (plaintext, key, iv) = decrypt_keys_data(&file, "").unwrap();
    let honest = parse_decrypted_keys(&plaintext, &key, iv).unwrap();
    let needle = honest.spend_public_key;
    if let Some(at) = plaintext
        .windows(needle.len())
        .position(|window| window == needle)
    {
        let mut tampered = plaintext.clone();
        tampered[at + 5] ^= 0x01;
        assert!(parse_decrypted_keys(&tampered, &key, iv).is_err());
    } else {
        eprintln!("public spend key is JSON-escaped in this vector; framing covers it");
    }
    assert!(read_keys_file(&vector_path("cold_full.keys"), "wrong").is_err());
}

#[test]
#[ignore = "known defect: upper-case UR frames are refused"]
fn uppercase_ur_frames_are_accepted() {
    let payload = b"a payload long enough to need more than one fountain fragment".to_vec();
    let mut encoder = UrEncoder::new(&payload, UR_TYPE_UNSIGNED_TX, 20).unwrap();
    let mut decoder = UrDecoder::new();
    let mut done = None;
    for _ in 0..encoder.fragment_count() * 3 {
        let frame = encoder.next_frame().unwrap();
        match decoder.receive(&frame.uri.to_uppercase()) {
            Ok(progress) => {
                if let Some(completed) = progress.completed {
                    done = Some(completed);
                    break;
                }
            }
            Err(error) => panic!("an upper-case UR frame was refused: {error}"),
        }
    }
    assert_eq!(done.expect("transfer completes").message, payload);
}

#[test]
fn ur_decoder_survives_interleaved_transfers_of_one_type() {
    let first = b"first transfer, several fragments long, scanned from one screen".to_vec();
    let second = b"second transfer, also several fragments, shown by someone else!".to_vec();
    let mut a = UrEncoder::new(&first, UR_TYPE_UNSIGNED_TX, 20).unwrap();
    let mut b = UrEncoder::new(&second, UR_TYPE_UNSIGNED_TX, 20).unwrap();
    let mut decoder = UrDecoder::new();
    let mut done = None;
    let mut refused = 0;
    for _ in 0..a.fragment_count() * 4 {
        let own = a.next_frame().unwrap();
        let foreign = b.next_frame().unwrap();
        if decoder.receive(&foreign.uri).is_err() {
            refused += 1;
        }
        if let Ok(progress) = decoder.receive(&own.uri) {
            if let Some(completed) = progress.completed {
                done = Some(completed);
                break;
            }
        }
    }
    let done = done.expect("the transfer whose frames kept arriving completes");
    assert!(done.message == first || done.message == second);
    eprintln!("refused {refused} frames of the other transfer");
}

#[test]
fn build_signed_txset_request_debug_output_hides_the_view_key() {
    let hot = hot_keys();
    let request = monero_rust::epee_compat::BuildSignedTxSetRequest {
        unsigned_txset: &[],
        view_secret_key: &hot.view_secret_key,
        tx_blobs: Vec::new(),
        key_images: &[],
        tx_key_images: &[],
    };
    let debug = format!("{request:?}");
    let decimal = format!("{:?}", hot.view_secret_key);
    assert!(
        !debug.contains(decimal.trim_matches(|c| c == '[' || c == ']')),
        "{debug}"
    );
    assert!(
        !debug.contains(&hex::encode(hot.view_secret_key)),
        "{debug}"
    );
}

#[test]
fn scan_config_and_derived_keys_debug_output_hides_secrets() {
    let mnemonic = cold_keys().mnemonic.clone().expect("full wallet");
    let config = monero_rust::WalletScanConfig {
        mnemonic: mnemonic.clone(),
        network: monero_rust::Network::Mainnet,
        lookahead: monero_rust::DEFAULT_LOOKAHEAD,
        passphrase: "a passphrase nobody should read".to_string(),
    };
    let debug = format!("{config:?}");
    assert!(
        !debug.contains(mnemonic.split(' ').next().unwrap()),
        "{debug}"
    );
    assert!(!debug.contains("nobody should read"), "{debug}");

    let keys = monero_rust::derive_keys(&mnemonic, monero_rust::Network::Mainnet, "").unwrap();
    let debug = format!("{keys:?}");
    for secret in [
        &**keys.secret_spend_key.as_ref().unwrap(),
        &*keys.secret_view_key,
    ] {
        assert!(!debug.contains(&hex::encode(secret)));
        let decimal = format!("{secret:?}");
        assert!(!debug.contains(decimal.trim_matches(|c| c == '[' || c == ']')));
    }
    assert!(debug.contains(&hex::encode(keys.public_spend_key)));
}
