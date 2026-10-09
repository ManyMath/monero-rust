//! Seeded mutation loops over every parser of counterpart bytes: any error is
//! fine, a panic is not. `OFFLINE_FUZZ_SECONDS` (default 20) bounds each loop.

mod cold_signing_common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use cold_signing_common::*;
use monero_rust::epee_compat::{self, ExportedKeyImage};
use monero_rust::key_image_signing::import_key_images_v3;
use monero_rust::native::{
    create_unsigned_transaction_from_wallet2_txset_keys, preview_unsigned_transaction,
};
use monero_rust::unsigned_tx::UnsignedTransaction;
use monero_rust::ur_codec::UrDecoder;
use monero_rust::wallet_keys_file::{decrypt_keys_data, parse_decrypted_keys};

fn budget() -> Duration {
    Duration::from_secs(
        std::env::var("OFFLINE_FUZZ_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20),
    )
}

fn fuzz(
    name: &str,
    seed: u64,
    cap: usize,
    base: &[u8],
    mut case: impl FnMut(&[u8]) -> bool,
) -> (usize, usize) {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut rng = Xs(seed);
    let started = Instant::now();
    let limit = budget();
    let mut slowest = Duration::ZERO;
    let (mut cases, mut accepted) = (0usize, 0usize);
    let mut panics: Vec<String> = Vec::new();
    while cases < cap && started.elapsed() < limit {
        let mut data = base.to_vec();
        for _ in 0..=rng.below(3) {
            mutate(&mut rng, &mut data);
        }
        let case_started = Instant::now();
        match catch_unwind(AssertUnwindSafe(|| case(&data))) {
            Ok(true) => accepted += 1,
            Ok(false) => {}
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "non-string panic".into());
                if panics.len() < 5 {
                    panics.push(format!(
                        "case {cases}: {message}; input {}",
                        hex::encode(&data)
                    ));
                }
            }
        }
        slowest = slowest.max(case_started.elapsed());
        cases += 1;
    }
    std::panic::set_hook(hook);
    eprintln!(
        "{name}: {cases} cases in {:?}, {accepted} accepted, slowest case {slowest:?}, {} panics",
        started.elapsed(),
        panics.len()
    );
    assert!(panics.is_empty(), "{name} panicked:\n{}", panics.join("\n"));
    (cases, accepted)
}

#[test]
fn fuzz_app_unsigned_transaction_envelope() {
    let cold = cold_keys();
    let converted = create_unsigned_transaction_from_wallet2_txset_keys(
        cold.spend_secret_key,
        cold.view_secret_key,
        &vector_unsigned_txset(),
        "mainnet",
    )
    .unwrap();
    let base = hex::decode(converted.unsigned_tx_hex).unwrap();
    assert!(UnsignedTransaction::from_bytes(&base).is_ok());
    fuzz("MRUT envelope", 0x5eed_0001, 200_000, &base, |data| {
        let parsed = UnsignedTransaction::from_bytes(data).is_ok();
        let _ = preview_unsigned_transaction(
            cold.spend_secret_key,
            cold.view_secret_key,
            &hex::encode(data),
            "mainnet",
        );
        parsed
    });
}

#[test]
fn fuzz_wallet2_unsigned_txset_archive() {
    let cold = cold_keys();
    let base = open(
        epee_compat::UNSIGNED_TX_MAGIC,
        &vector_unsigned_txset(),
        &cold.view_secret_key,
    );
    fuzz(
        "wallet2 unsigned txset",
        0x5eed_0002,
        20_000,
        &base,
        |data| {
            let sealed = seal(epee_compat::UNSIGNED_TX_MAGIC, data, &cold.view_secret_key);
            let parsed =
                epee_compat::parse_unsigned_monero_txset_summary(&sealed, &cold.view_secret_key)
                    .is_ok();
            if parsed {
                let _ = preview_unsigned_transaction(
                    cold.spend_secret_key,
                    cold.view_secret_key,
                    &hex::encode(&sealed),
                    "mainnet",
                );
            }
            parsed
        },
    );
}

#[test]
fn fuzz_wallet2_signed_txset_archive() {
    let hot = hot_keys();
    let vector = std::fs::read(vector_path("monero_wallet_rpc_signed_monero_tx")).unwrap();
    epee_compat::extract_signed_monero_txset_transactions(&vector, &hot.view_secret_key)
        .expect("wallet2's signed txset is accepted as it is");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx")).unwrap();
    epee_compat::extract_signed_monero_txset_transactions_for_unsigned(
        &vector,
        &hot.view_secret_key,
        &unsigned,
    )
    .expect("and it answers the unsigned txset");
    let base = open(epee_compat::SIGNED_TX_MAGIC, &vector, &hot.view_secret_key);
    fuzz("wallet2 signed txset", 0x5eed_0003, 20_000, &base, |data| {
        let sealed = seal(epee_compat::SIGNED_TX_MAGIC, data, &hot.view_secret_key);
        if epee_compat::extract_signed_monero_txset_transactions(&sealed, &hot.view_secret_key)
            .is_err()
        {
            return false;
        }
        epee_compat::extract_signed_monero_txset_transactions_for_unsigned(
            &sealed,
            &hot.view_secret_key,
            &unsigned,
        )
        .is_ok()
    });
}

#[test]
fn fuzz_wallet2_output_export_archive() {
    let hot = hot_keys();
    let vector = std::fs::read(vector_path("outputs")).unwrap();
    let base = open(
        epee_compat::OUTPUT_EXPORT_MAGIC,
        &vector,
        &hot.view_secret_key,
    );
    fuzz(
        "wallet2 output export",
        0x5eed_0004,
        20_000,
        &base,
        |data| {
            let sealed = seal(epee_compat::OUTPUT_EXPORT_MAGIC, data, &hot.view_secret_key);
            let _ = epee_compat::inspect_output_export(&sealed, &hot.view_secret_key);
            epee_compat::parse_output_export(&sealed, &hot.view_secret_key).is_ok()
        },
    );
}

#[test]
fn fuzz_key_image_export_v3_and_container_framing() {
    let hot = hot_keys();
    let mut body = 0u32.to_le_bytes().to_vec();
    body.extend_from_slice(&hot.spend_public_key);
    body.extend_from_slice(&hot.view_public_key);
    body.extend_from_slice(&[0x42; 96 * 3]);
    fuzz(
        "key image export v3 body",
        0x5eed_0005,
        20_000,
        &body,
        |data| {
            let sealed = seal(epee_compat::KEY_IMAGES_MAGIC, data, &hot.view_secret_key);
            let _ = epee_compat::import_signed_key_images_for_account(
                &sealed,
                Some(&hot.view_secret_key),
                Some((&hot.spend_public_key, &hot.view_public_key)),
            );
            import_key_images_v3(&sealed, &hot.view_secret_key).is_ok()
        },
    );

    let sealed = seal(epee_compat::KEY_IMAGES_MAGIC, &body, &hot.view_secret_key);
    fuzz(
        "key image export framing",
        0x5eed_0006,
        20_000,
        &sealed,
        |data| {
            let _ = epee_compat::import_key_images(data, None);
            import_key_images_v3(data, &hot.view_secret_key).is_ok()
        },
    );
}

#[test]
fn fuzz_legacy_epee_key_image_export() {
    #[allow(deprecated)]
    let base = epee_compat::export_key_images(&[
        ExportedKeyImage {
            key_image: "a1".repeat(32),
            tx_hash: "00".repeat(32),
            output_index: 0,
        },
        ExportedKeyImage {
            key_image: "b2".repeat(32),
            tx_hash: "00".repeat(32),
            output_index: 1,
        },
    ])
    .unwrap();
    fuzz(
        "legacy Epee key images",
        0x5eed_0007,
        300_000,
        &base,
        |data| {
            let _ = epee_compat::import_signed_key_images_for_account(data, None, None);
            epee_compat::import_key_images(data, None).is_ok()
        },
    );
}

#[test]
fn fuzz_keys_file_plaintext_and_container() {
    let file = std::fs::read(vector_path("cold_full.keys")).unwrap();
    let (plaintext, key, iv) = decrypt_keys_data(&file, "").unwrap();
    assert!(parse_decrypted_keys(&plaintext, &key, iv).is_ok());
    fuzz(
        "keys file plaintext",
        0x5eed_0008,
        50_000,
        &plaintext,
        |data| parse_decrypted_keys(data, &key, iv).is_ok(),
    );
    fuzz("keys file container", 0x5eed_0009, 5_000, &file, |data| {
        decrypt_keys_data(data, "").is_ok()
    });
}

#[test]
fn fuzz_wallet_rpc_key_image_json() {
    let base = format!(
        r#"{{"result":{{"offset":3,"signed_key_images":[{{"key_image":"{}","signature":"{}"}},{{"key_image":"{}","signature":"{}"}}]}}}}"#,
        "a1".repeat(32),
        "0b".repeat(64),
        "c2".repeat(32),
        "0d".repeat(64)
    );
    assert!(monero_rust::parse_rpc_export(&base).is_ok());
    fuzz(
        "key image RPC JSON",
        0x5eed_000a,
        200_000,
        base.as_bytes(),
        |data| {
            std::str::from_utf8(data)
                .map(|text| monero_rust::parse_rpc_export(text).is_ok())
                .unwrap_or(false)
        },
    );
}

fn cbor_uint(value: u64, out: &mut Vec<u8>) {
    match value {
        0..=23 => out.push(value as u8),
        24..=0xff => out.extend_from_slice(&[0x18, value as u8]),
        0x100..=0xffff => {
            out.push(0x19);
            out.extend_from_slice(&(value as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(0x1a);
            out.extend_from_slice(&(value as u32).to_be_bytes());
        }
        _ => {
            out.push(0x1b);
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn ur_part(sequence: u64, count: u64, length: u64, checksum: u64, data: &[u8]) -> String {
    let mut cbor = vec![0x85];
    for value in [sequence, count, length, checksum] {
        cbor_uint(value, &mut cbor);
    }
    let mut head = Vec::new();
    cbor_uint(data.len() as u64, &mut head);
    head[0] |= 0x40;
    cbor.extend_from_slice(&head);
    cbor.extend_from_slice(data);
    format!(
        "ur:xmr-txunsigned/{}-{}/{}",
        sequence % 65_536,
        count % 65_536,
        ur::bytewords::encode(&cbor, ur::bytewords::Style::Minimal)
    )
}

#[test]
fn fuzz_ur_fountain_frames() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut rng = Xs(0x5eed_000b);
    let started = Instant::now();
    let limit = budget();
    let interesting = [
        0u64,
        1,
        2,
        3,
        15,
        16,
        255,
        256,
        8_191,
        8_192,
        8_193,
        65_535,
        65_536,
        u32::MAX as u64 - 1,
        u32::MAX as u64,
        u32::MAX as u64 + 1,
        u64::MAX,
    ];
    let pick = |rng: &mut Xs| {
        if rng.below(3) == 0 {
            rng.next() % 40
        } else {
            interesting[rng.below(interesting.len())]
        }
    };
    let mut shared = UrDecoder::new();
    let (mut cases, mut errors, mut completed) = (0usize, 0usize, 0usize);
    let mut panics = Vec::new();
    let mut slowest = Duration::ZERO;
    while cases < 60_000 && started.elapsed() < limit {
        let data_len = [0usize, 1, 2, 7, 64][rng.below(5)];
        let data: Vec<u8> = (0..data_len).map(|_| rng.next() as u8).collect();
        let mut uri = ur_part(
            pick(&mut rng),
            pick(&mut rng),
            pick(&mut rng),
            pick(&mut rng),
            &data,
        );
        if rng.below(4) == 0 {
            let mut bytes = uri.into_bytes();
            mutate(&mut rng, &mut bytes);
            uri = String::from_utf8_lossy(&bytes).into_owned();
        }
        let case_started = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let fresh = UrDecoder::new().receive(&uri).map(|p| p.is_complete());
            let kept = shared.receive(&uri).map(|p| p.is_complete());
            (fresh, kept)
        }));
        slowest = slowest.max(case_started.elapsed());
        match outcome {
            Ok((fresh, kept)) => {
                errors += usize::from(fresh.is_err()) + usize::from(kept.is_err());
                completed += usize::from(fresh == Ok(true)) + usize::from(kept == Ok(true));
            }
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                if panics.len() < 5 {
                    panics.push(format!("case {cases}: {message}; frame {uri}"));
                }
                shared = UrDecoder::new();
            }
        }
        cases += 1;
    }
    std::panic::set_hook(hook);
    eprintln!(
        "UR frames: {cases} cases in {:?}, {errors} refusals, {completed} completions, slowest {slowest:?}, {} panics",
        started.elapsed(),
        panics.len()
    );
    assert!(
        panics.is_empty(),
        "UR decoder panicked:\n{}",
        panics.join("\n")
    );
}
