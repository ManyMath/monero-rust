//! The crate side of `scripts/cold_signing_wallet2_interop.py`, which drives the
//! ignored test `tool` through `COLD_SIGNING_CMD`.

mod cold_signing_common;

use cold_signing_common::*;
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE,
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
};
use monero_rust::crypto::generate_key_image;
use monero_rust::epee_compat::{self, ExportedTransferDetails, SignedTxSetKeyImagePair};
use monero_rust::key_image_signing::{export_key_images_v3, KeyImageExportEntry};
use monero_rust::native::{
    preview_unsigned_transaction, sign_unsigned_transaction_with_private_keys,
};
use monero_rust::{WalletOutput, WalletState};
use serde_json::json;
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn env_key(name: &str) -> [u8; 32] {
    hex::decode(env(name))
        .expect("hex key")
        .try_into()
        .expect("32-byte key")
}

fn read_wallet2(name: &str) -> Vec<u8> {
    let data = std::fs::read(env(name)).unwrap();
    epee_compat::read_wallet2_file(&data)
        .expect("wallet2 file")
        .into_owned()
}

fn emit(value: serde_json::Value) {
    println!("COLD_SIGNING_JSON {value}");
}

fn hash_to_scalar(data: &[u8]) -> Scalar {
    Scalar::from_bytes_mod_order(Keccak256::digest(data).into())
}

fn key_offset_of(
    transfer: &ExportedTransferDetails,
    spend: &Scalar,
    view: &[u8; 32],
) -> Option<Scalar> {
    let view_scalar = Scalar::from_bytes_mod_order(*view);
    let mut subaddress = Scalar::ZERO;
    if transfer.subaddress_major != 0 || transfer.subaddress_minor != 0 {
        let mut material = b"SubAddr\0".to_vec();
        material.extend_from_slice(view);
        material.extend_from_slice(&(transfer.subaddress_major as u32).to_le_bytes());
        material.extend_from_slice(&(transfer.subaddress_minor as u32).to_le_bytes());
        subaddress = hash_to_scalar(&material);
    }
    let mut candidates = vec![transfer.tx_public_key];
    candidates.extend(transfer.additional_tx_keys.iter().copied());
    for tx_key in candidates {
        let Some(point) = CompressedEdwardsY(tx_key).decompress() else {
            continue;
        };
        let mut material = (view_scalar * point)
            .mul_by_cofactor()
            .compress()
            .to_bytes()
            .to_vec();
        varint(transfer.internal_output_index, &mut material);
        let offset = hash_to_scalar(&material) + subaddress;
        let public = (&(spend + offset) * ED25519_BASEPOINT_TABLE)
            .compress()
            .to_bytes();
        if public == transfer.output_public_key {
            return Some(offset);
        }
    }
    None
}

fn key_image_entries(
    transfers: &[ExportedTransferDetails],
    spend: &Scalar,
    view: &[u8; 32],
) -> Vec<KeyImageExportEntry> {
    transfers
        .iter()
        .enumerate()
        .map(|(index, transfer)| {
            let key_offset = key_offset_of(transfer, spend, view)
                .unwrap_or_else(|| panic!("exported output {index} is not this wallet's"));
            let secret = Zeroizing::new(spend + key_offset);
            let pub_key: EdwardsPoint = CompressedEdwardsY(transfer.output_public_key)
                .decompress()
                .expect("output key");
            KeyImageExportEntry {
                key_image: generate_key_image(&secret).compress().to_bytes(),
                pub_key,
                key_offset,
            }
        })
        .collect()
}

fn outputs_for_state(transfers: &[ExportedTransferDetails]) -> Vec<WalletOutput> {
    transfers
        .iter()
        .enumerate()
        .map(|(index, transfer)| WalletOutput {
            tx_hash: format!("{index:064x}"),
            output_index: transfer.internal_output_index,
            amount: transfer.amount,
            key: hex::encode(transfer.output_public_key),
            key_offset: String::new(),
            commitment_mask: String::new(),
            subaddress_index: Some((
                transfer.subaddress_major as u32,
                transfer.subaddress_minor as u32,
            )),
            payment_id: None,
            received_output_bytes: String::new(),
            global_index: Some(transfer.global_output_index),
            ring: None,
            block_height: index as u64 + 1,
            spent: false,
            spent_height: None,
            key_image: String::new(),
            is_coinbase: false,
            frozen: false,
        })
        .collect()
}

fn cmd_craft() {
    let out = std::path::PathBuf::from(env("COLD_SIGNING_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let view = cold_keys().view_secret_key;
    let write = |name: &str, set: &epee_compat::ParsedUnsignedTxSetSummary| {
        std::fs::write(out.join(name), seal_unsigned_txset(set, &view)).unwrap();
    };
    let foreign = "44AFFq5kSiGBoZ4NMDwYtN18obc8AemS33DBLWs3H7otXft3XjrpDtQGv7SqSsaBYBb98uNbr2VBBEt7f2wfn3RVGQBEP3A";

    write("control", &parse_vector_unsigned());

    let mut set = parse_vector_unsigned();
    set.txes[0].unlock_time = 1_000_000;
    write("unlock_time", &set);

    let mut set = parse_vector_unsigned();
    set.txes[0].destinations[0].original = foreign.as_bytes().to_vec();
    set.txes[0].destinations[0].original_len = foreign.len();
    set.txes[0].split_destinations[1] = set.txes[0].destinations[0].clone();
    write("original_mismatch", &set);

    let mut set = parse_vector_unsigned();
    {
        let parsed =
            monero_wallet::address::MoneroAddress::from_str_with_unchecked_network(foreign)
                .unwrap();
        let spend: EdwardsPoint = parsed.spend().into();
        let view_key: EdwardsPoint = parsed.view().into();
        let entry = &mut set.txes[0].split_destinations[1];
        entry.original = foreign.as_bytes().to_vec();
        entry.original_len = foreign.len();
        entry.spend_public_key = spend.compress().to_bytes();
        entry.view_public_key = view_key.compress().to_bytes();
    }
    write("split_mismatch", &set);

    let mut set = parse_vector_unsigned();
    let duplicate = set.txes[0].clone();
    set.txes.push(duplicate);
    write("duplicate_input", &set);

    let mut set = parse_vector_unsigned();
    set.txes[0].construction_flags = 0x01;
    write("no_view_tags", &set);

    emit(json!({"written": out}));
}

fn cmd_parse_outputs() {
    let data = read_wallet2("COLD_SIGNING_OUTPUTS");
    let view = env_key("COLD_SIGNING_VIEW");
    match epee_compat::parse_output_export(&data, &view) {
        Ok(parsed) => emit(json!({
            "ok": true,
            "offset": parsed.offset,
            "total": parsed.total_outputs,
            "count": parsed.outputs.len(),
            "spend_public": hex::encode(parsed.public_spend_key),
            "view_public": hex::encode(parsed.public_view_key),
            "amounts": parsed.outputs.iter().map(|o| o.amount).collect::<Vec<_>>(),
            "pubkeys": parsed.outputs.iter().map(|o| hex::encode(o.output_public_key)).collect::<Vec<_>>(),
            "subaddresses": parsed.outputs.iter().map(|o| (o.subaddress_major, o.subaddress_minor)).collect::<Vec<_>>(),
        })),
        Err(error) => emit(json!({"ok": false, "error": error})),
    }
}

fn cmd_export_key_images() {
    let data = read_wallet2("COLD_SIGNING_OUTPUTS");
    let view = env_key("COLD_SIGNING_VIEW");
    let spend = Scalar::from_bytes_mod_order(env_key("COLD_SIGNING_SPEND"));
    let parsed = epee_compat::parse_output_export(&data, &view).expect("output export");
    let entries = key_image_entries(&parsed.outputs, &spend, &view);
    let offset: u32 = std::env::var("COLD_SIGNING_OFFSET")
        .ok()
        .map_or(0, |v| v.parse().unwrap());
    let exported = export_key_images_v3(
        &entries[offset as usize..],
        &spend,
        &parsed.public_spend_key,
        &parsed.public_view_key,
        &view,
        offset,
    )
    .expect("export");
    if std::env::var("COLD_SIGNING_ASCII").is_ok() {
        let armoured = epee_compat::write_wallet2_ascii_file(&exported);
        std::fs::write(env("COLD_SIGNING_OUT"), armoured).unwrap();
    } else {
        std::fs::write(env("COLD_SIGNING_OUT"), &exported).unwrap();
    }
    emit(json!({
        "ok": true,
        "count": entries.len() - offset as usize,
        "offset": offset,
        "key_images": entries.iter().map(|e| hex::encode(e.key_image)).collect::<Vec<_>>(),
    }));
}

fn cmd_import_key_images() {
    let outputs = read_wallet2("COLD_SIGNING_OUTPUTS");
    let view = env_key("COLD_SIGNING_VIEW");
    let parsed = epee_compat::parse_output_export(&outputs, &view).expect("output export");
    let data = read_wallet2("COLD_SIGNING_KEY_IMAGES");
    let import = match epee_compat::import_signed_key_images_for_account(
        &data,
        Some(&view),
        Some((&parsed.public_spend_key, &parsed.public_view_key)),
    ) {
        Ok(import) => import,
        Err(error) => return emit(json!({"ok": false, "stage": "parse", "error": error})),
    };
    let mut state = WalletState::new();
    state.replace_outputs(outputs_for_state(&parsed.outputs));
    match state.assign_verified_key_images_in_export_order(&import) {
        Ok(count) => emit(json!({
            "ok": true,
            "offset": import.offset,
            "assigned": count,
            "key_images": state.outputs().iter().map(|o| o.key_image.clone()).collect::<Vec<_>>(),
        })),
        Err(error) => emit(json!({"ok": false, "stage": "verify", "error": error})),
    }
}

fn cmd_preview() {
    let unsigned = read_wallet2("COLD_SIGNING_UNSIGNED");
    let outcome = preview_unsigned_transaction(
        env_key("COLD_SIGNING_SPEND"),
        env_key("COLD_SIGNING_VIEW"),
        &hex::encode(unsigned),
        &env("COLD_SIGNING_NETWORK"),
    );
    match outcome {
        Ok(preview) => emit(json!({"ok": true, "preview": preview})),
        Err(error) => emit(json!({"ok": false, "error": error})),
    }
}

fn cmd_sign() {
    let unsigned = read_wallet2("COLD_SIGNING_UNSIGNED");
    let spend_bytes = env_key("COLD_SIGNING_SPEND");
    let view = env_key("COLD_SIGNING_VIEW");
    let network = env("COLD_SIGNING_NETWORK");
    let encoded = hex::encode(&unsigned);
    let preview = match preview_unsigned_transaction(spend_bytes, view, &encoded, &network) {
        Ok(preview) => preview,
        Err(error) => return emit(json!({"ok": false, "stage": "preview", "error": error})),
    };
    let approval = preview.approve(u64::MAX, u64::MAX).unwrap();
    let signed = match sign_unsigned_transaction_with_private_keys(
        spend_bytes,
        view,
        &encoded,
        &network,
        &approval,
    ) {
        Ok(signed) => signed,
        Err(error) => {
            return emit(json!({"ok": false, "stage": "sign", "error": error, "preview": preview}))
        }
    };

    let outputs = read_wallet2("COLD_SIGNING_OUTPUTS");
    let parsed = epee_compat::parse_output_export(&outputs, &view).expect("output export");
    let spend = Scalar::from_bytes_mod_order(spend_bytes);
    let key_images: Vec<[u8; 32]> = key_image_entries(&parsed.outputs, &spend, &view)
        .iter()
        .map(|entry| entry.key_image)
        .collect();
    let mut tx_key_images = Vec::new();
    let blobs: Vec<Vec<u8>> = signed
        .transactions
        .iter()
        .map(|tx| hex::decode(&tx.tx_blob).unwrap())
        .collect();
    for tx in &signed.transactions {
        for output in &tx.change_outputs {
            tx_key_images.push(SignedTxSetKeyImagePair {
                public_key: hex::decode(&output.key).unwrap().try_into().unwrap(),
                key_image: hex::decode(&output.key_image).unwrap().try_into().unwrap(),
            });
        }
    }
    let built = epee_compat::build_signed_monero_txset(epee_compat::BuildSignedTxSetRequest {
        unsigned_txset: &unsigned,
        view_secret_key: &view,
        tx_blobs: blobs.iter().map(Vec::as_slice).collect(),
        key_images: &key_images,
        tx_key_images: &tx_key_images,
    });
    match built {
        Ok(bytes) => {
            std::fs::write(env("COLD_SIGNING_OUT"), &bytes).unwrap();
            emit(json!({
                "ok": true,
                "preview": preview,
                "tx_ids": signed.transactions.iter().map(|t| t.tx_id.clone()).collect::<Vec<_>>(),
                "fees": signed.transactions.iter().map(|t| t.fee).collect::<Vec<_>>(),
                "tx_blobs": signed.transactions.iter().map(|t| t.tx_blob.clone()).collect::<Vec<_>>(),
            }));
        }
        Err(error) => emit(json!({"ok": false, "stage": "build", "error": error})),
    }
}

fn cmd_parse_signed() {
    let data = read_wallet2("COLD_SIGNING_SIGNED");
    let view = env_key("COLD_SIGNING_VIEW");
    let extracted = match std::env::var("COLD_SIGNING_UNSIGNED") {
        Ok(unsigned) => epee_compat::extract_signed_monero_txset_transactions_for_unsigned(
            &data,
            &view,
            &epee_compat::read_wallet2_file(&std::fs::read(unsigned).unwrap()).unwrap(),
        ),
        Err(_) => epee_compat::extract_signed_monero_txset_transactions(&data, &view),
    };
    match extracted {
        Ok(transactions) => emit(json!({
            "ok": true,
            "tx_hashes": transactions.iter().map(|t| hex::encode(t.tx_hash)).collect::<Vec<_>>(),
        })),
        Err(error) => emit(json!({"ok": false, "error": error})),
    }
}

fn cmd_parse_unsigned() {
    let data = read_wallet2("COLD_SIGNING_UNSIGNED");
    let view = env_key("COLD_SIGNING_VIEW");
    match epee_compat::parse_unsigned_monero_txset_summary(&data, &view) {
        Ok(summary) => emit(json!({
            "ok": true,
            "txes": summary.txes.len(),
            "new_transfers": summary.new_transfers.len(),
            "accounts": summary.txes.iter().map(|t| t.subaddr_account).collect::<Vec<_>>(),
            "subaddr_indices": summary.txes.iter().map(|t| t.subaddr_indices.clone()).collect::<Vec<_>>(),
            "originals": summary.txes.iter().map(|t| t.destinations.iter().map(|d| String::from_utf8_lossy(&d.original).to_string()).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "unlock_times": summary.txes.iter().map(|t| t.unlock_time).collect::<Vec<_>>(),
            "rct_configs": summary.txes.iter().map(|t| (t.rct_range_proof_type, t.rct_bp_version)).collect::<Vec<_>>(),
        })),
        Err(error) => emit(json!({"ok": false, "error": error})),
    }
}

fn cmd_read_keys() {
    let path = std::path::PathBuf::from(env("COLD_SIGNING_KEYS_FILE"));
    let password = std::env::var("COLD_SIGNING_PASSWORD").unwrap_or_default();
    match monero_rust::wallet_keys_file::read_keys_file(&path, &password) {
        Ok(keys) => emit(json!({
            "ok": true,
            "spend_public": hex::encode(keys.spend_public_key),
            "view_public": hex::encode(keys.view_public_key),
            "view_secret": hex::encode(keys.view_secret_key),
            "spend_secret": hex::encode(keys.spend_secret_key),
            "watch_only": keys.watch_only,
            "nettype": keys.nettype,
            "mnemonic": keys.mnemonic,
        })),
        Err(error) => emit(json!({"ok": false, "error": error})),
    }
}

fn cmd_rewrite_keys() {
    let path = std::path::PathBuf::from(env("COLD_SIGNING_KEYS_FILE"));
    let password = std::env::var("COLD_SIGNING_PASSWORD").unwrap_or_default();
    let keys = monero_rust::wallet_keys_file::read_keys_file(&path, &password).expect("read");
    let new_password = std::env::var("COLD_SIGNING_NEW_PASSWORD").unwrap_or_default();
    monero_rust::wallet_keys_file::write_keys_file(
        &std::path::PathBuf::from(env("COLD_SIGNING_OUT")),
        &new_password,
        &keys,
    )
    .expect("write");
    emit(json!({"ok": true}));
}

#[test]
#[ignore = "driven by scripts/cold_signing_wallet2_interop.py"]
fn tool() {
    match env("COLD_SIGNING_CMD").as_str() {
        "craft" => cmd_craft(),
        "parse-outputs" => cmd_parse_outputs(),
        "export-key-images" => cmd_export_key_images(),
        "import-key-images" => cmd_import_key_images(),
        "preview" => cmd_preview(),
        "sign" => cmd_sign(),
        "parse-signed" => cmd_parse_signed(),
        "parse-unsigned" => cmd_parse_unsigned(),
        "read-keys" => cmd_read_keys(),
        "rewrite-keys" => cmd_rewrite_keys(),
        other => panic!("unknown COLD_SIGNING_CMD {other}"),
    }
}
