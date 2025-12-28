use std::path::{Path, PathBuf};

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_rust::{
    epee_compat, extract_key_image_hex, key_image_signing, parse_rpc_export,
    wallet_keys_file::read_keys_file, WalletOutput, WalletState,
};
use monero_serai::ringct::generate_key_image;
use monero_serai::transaction::Transaction;
use serde::Deserialize;
use serde_json::Value;
use zeroize::Zeroizing;

const STANDARD_ADDRESS: &str = "42ey1afDFnn4886T7196doS9GPMzexD9gXpsZJDwVjeRVdFCSoHnv7KPbBeGpzJBzHRCAs9UxqeoyFQMYbqSWYTfJJQAWDm";

fn vector_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/cold_signing_regtest_v0_18_5_0")
        .join(name)
}

#[derive(Deserialize)]
struct KeyImagesRpc {
    offset: u64,
    signed_key_images: Vec<SignedKeyImage>,
}

#[derive(Deserialize)]
struct SignedKeyImage {
    key_image: String,
    signature: String,
}

fn synthetic_output_for_spend_key(
    spend_secret_key: [u8; 32],
    key_offset_byte: u8,
    block_height: u64,
    output_index: u8,
) -> (WalletOutput, String) {
    let spend_scalar = Scalar::from_bytes_mod_order(spend_secret_key);
    let key_offset = Scalar::from_bytes_mod_order([key_offset_byte; 32]);
    let ephemeral_sec = Zeroizing::new(spend_scalar + key_offset);
    let output_key = (&*ephemeral_sec * &ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes();
    let key_image = generate_key_image(&ephemeral_sec).compress().to_bytes();
    let key_image_hex = hex::encode(key_image);

    (
        WalletOutput {
            tx_hash: format!("{key_offset_byte:064x}"),
            output_index,
            amount: 1,
            amount_xmr: "0.000000000001".to_string(),
            key: hex::encode(output_key),
            key_offset: hex::encode(key_offset.to_bytes()),
            commitment_mask: String::new(),
            subaddress_index: Some((0, 0)),
            payment_id: None,
            received_output_bytes: String::new(),
            block_height,
            spent: false,
            spent_height: None,
            key_image: key_image_hex.clone(),
            is_coinbase: false,
            frozen: false,
        },
        key_image_hex,
    )
}

#[test]
fn cold_signing_binary_artifacts_have_current_monero_magic() {
    let outputs =
        std::fs::read(vector_path("outputs")).expect("outputs fixture should be readable");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let signed =
        std::fs::read(vector_path("signed_monero_tx")).expect("signed txset should be readable");

    assert!(outputs.starts_with(epee_compat::OUTPUT_EXPORT_MAGIC));
    assert!(unsigned.starts_with(epee_compat::UNSIGNED_TX_MAGIC));
    assert!(signed.starts_with(epee_compat::SIGNED_TX_MAGIC));
}

#[test]
fn cold_signing_txsets_are_detected_as_monero_wallet2_containers() {
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let signed =
        std::fs::read(vector_path("signed_monero_tx")).expect("signed txset should be readable");

    assert_eq!(
        epee_compat::detect_monero_txset(&unsigned),
        Some(epee_compat::MoneroTxSetKind::Unsigned)
    );
    assert_eq!(
        epee_compat::detect_monero_txset(&signed),
        Some(epee_compat::MoneroTxSetKind::Signed)
    );
    assert_eq!(
        epee_compat::strip_magic(epee_compat::UNSIGNED_TX_MAGIC, &unsigned)
            .expect("unsigned txset should strip")
            .len(),
        unsigned.len() - epee_compat::UNSIGNED_TX_MAGIC.len()
    );
    assert_eq!(
        epee_compat::strip_magic(epee_compat::SIGNED_TX_MAGIC, &signed)
            .expect("signed txset should strip")
            .len(),
        signed.len() - epee_compat::SIGNED_TX_MAGIC.len()
    );
}

#[test]
fn cold_signing_txsets_decrypt_to_wallet2_binary_archives() {
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let signed =
        std::fs::read(vector_path("signed_monero_tx")).expect("signed txset should be readable");

    let unsigned_info = epee_compat::inspect_monero_txset(&unsigned, &hot.view_secret_key)
        .expect("unsigned txset should decrypt with generated view key");
    let signed_info = epee_compat::inspect_monero_txset(&signed, &hot.view_secret_key)
        .expect("signed txset should decrypt with generated view key");

    assert_eq!(unsigned_info.kind, epee_compat::MoneroTxSetKind::Unsigned);
    assert_eq!(signed_info.kind, epee_compat::MoneroTxSetKind::Signed);
    assert_eq!(unsigned_info.archive_version, 2);
    assert_eq!(signed_info.archive_version, 0);
    assert_eq!(unsigned_info.transaction_count, 1);
    assert_eq!(signed_info.transaction_count, 1);
    assert_eq!(
        unsigned_info.encrypted_payload_len,
        unsigned.len() - epee_compat::UNSIGNED_TX_MAGIC.len()
    );
    assert_eq!(
        signed_info.encrypted_payload_len,
        signed.len() - epee_compat::SIGNED_TX_MAGIC.len()
    );
    assert_eq!(
        unsigned_info.decrypted_archive_len + 72,
        unsigned_info.encrypted_payload_len
    );
    assert_eq!(
        signed_info.decrypted_archive_len + 72,
        signed_info.encrypted_payload_len
    );
    assert!(unsigned_info.decrypted_archive_len > 0);
    assert!(signed_info.decrypted_archive_len > unsigned_info.decrypted_archive_len);
}

#[test]
fn cold_signing_unsigned_txset_parses_wallet2_construction_summary() {
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let outputs =
        std::fs::read(vector_path("outputs")).expect("output export fixture should be readable");
    let exported_outputs = epee_compat::parse_output_export(&outputs, &hot.view_secret_key)
        .expect("output export should parse with generated view key");
    let transfer_description: Value = serde_json::from_str(
        &std::fs::read_to_string(vector_path("transfer_description.json"))
            .expect("transfer description should be readable"),
    )
    .expect("transfer description should be valid JSON");
    let summary = epee_compat::parse_unsigned_monero_txset_summary(&unsigned, &hot.view_secret_key)
        .expect("unsigned txset construction summary should parse");
    let transfer_desc = &transfer_description["desc"][0];

    assert_eq!(summary.archive_version, 2);
    assert_eq!(summary.txes.len(), 1);
    // Keep the wallet2 tuple bounds raw here; this generated txset carries
    // construction data, while exported output records live in the outputs file.
    assert_eq!(summary.new_transfer_first, 80);
    assert_eq!(summary.new_transfer_second, 80);
    assert_eq!(summary.new_transfers.len(), 0);

    let tx = &summary.txes[0];
    assert_eq!(tx.source_count, 2);
    assert_eq!(tx.source_ring_sizes, vec![16, 16]);
    assert_eq!(tx.sources.len(), 2);
    let expected_real_global_indices = [0, 20];
    let expected_real_ctkeys = [
        "c6f77006ef10753eae14f1203bb0f788f7fa917bdc6a7e1121d0c6650a8819f1a1a7a42155f0abff0353a6008eda2a9b16d9ffcf7584a38933cce3e3976987cd",
        "affa3f56d5fb2746aaace6b16249432343dbcf0056a1d501a12786157cd7dd9a39339ac52a1194790b1bb5db0b119d403a1d5dcc4db4f8819fca4d425d5b2614",
    ];
    for (index, source) in tx.sources.iter().enumerate() {
        let expected = &transfer_desc["sources"][index];
        assert_eq!(
            source.amount,
            expected["amount"]
                .as_u64()
                .expect("source amount should be present"),
            "source #{index}"
        );
        assert_eq!(
            source.rct,
            expected["rct"].as_bool().unwrap(),
            "source #{index}"
        );
        assert!(
            source.real_output < source.ring.len() as u64,
            "source #{index}"
        );
        assert_eq!(source.real_output_in_tx_index, 0, "source #{index}");
        assert_eq!(
            source.real_out_additional_tx_keys.len(),
            0,
            "source #{index}"
        );
        let real = &source.ring[source.real_output as usize];
        assert_eq!(
            real.global_output_index, expected_real_global_indices[index],
            "source #{index}"
        );
        let exported_output = &exported_outputs.outputs[real.global_output_index as usize];
        assert_eq!(
            real.output_public_key, exported_output.output_public_key,
            "source #{index}"
        );
        assert_eq!(source.amount, exported_output.amount, "source #{index}");
        assert_eq!(
            source.real_out_tx_key, exported_output.tx_public_key,
            "source #{index}"
        );
        assert_eq!(
            source.real_output_in_tx_index, exported_output.internal_output_index,
            "source #{index}"
        );
        assert_eq!(
            format!(
                "{}{}",
                hex::encode(real.output_public_key),
                hex::encode(real.commitment)
            ),
            expected_real_ctkeys[index],
            "source #{index}"
        );
    }
    assert_eq!(tx.change_amount, 69_364_735_717_119);
    assert_eq!(tx.change.spend_public_key, hot.spend_public_key);
    assert_eq!(tx.change.view_public_key, hot.view_public_key);
    assert!(!tx.change.is_subaddress);
    assert!(!tx.change.is_integrated);
    assert_eq!(tx.change.original_len, 0);
    assert_eq!(tx.split_destination_count, 2);
    assert_eq!(tx.split_destination_total_amount, 70_364_735_717_119);
    assert_eq!(tx.split_destinations.len(), 2);
    assert_eq!(tx.split_destinations[0].amount, tx.change_amount);
    assert_eq!(tx.split_destinations[0].original_len, 0);
    assert!(tx.split_destinations[0].original.is_empty());
    assert_eq!(
        tx.split_destinations[0].spend_public_key,
        hot.spend_public_key
    );
    assert_eq!(
        tx.split_destinations[0].view_public_key,
        hot.view_public_key
    );
    assert_eq!(tx.split_destinations[1].amount, 1_000_000_000_000);
    assert_eq!(
        tx.split_destinations[1].original_len,
        STANDARD_ADDRESS.len()
    );
    assert_eq!(
        tx.split_destinations[1].original,
        STANDARD_ADDRESS.as_bytes()
    );
    assert_eq!(
        tx.split_destinations[1].spend_public_key,
        hot.spend_public_key
    );
    assert_eq!(
        tx.split_destinations[1].view_public_key,
        hot.view_public_key
    );
    assert_eq!(tx.selected_transfer_count, 2);
    assert_eq!(tx.selected_transfer_indices, vec![0, 20]);
    assert_eq!(tx.extra_len, 44);
    assert_eq!(
        hex::encode(&tx.extra),
        transfer_desc["extra"]
            .as_str()
            .expect("transfer extra should be present")
    );
    assert_eq!(tx.unlock_time, 0);
    assert_eq!(tx.construction_flags, 0x03);
    assert!(tx.use_rct);
    assert!(tx.use_view_tags);
    assert_eq!(tx.rct_range_proof_type, 3);
    assert_eq!(tx.rct_bp_version, 4);
    assert_eq!(tx.destination_count, 1);
    assert_eq!(tx.destination_total_amount, 1_000_000_000_000);
    assert_eq!(tx.destinations.len(), 1);
    assert_eq!(tx.destinations[0].amount, 1_000_000_000_000);
    assert_eq!(tx.destinations[0].original_len, STANDARD_ADDRESS.len());
    assert_eq!(tx.destinations[0].original, STANDARD_ADDRESS.as_bytes());
    assert_eq!(tx.destinations[0].spend_public_key, hot.spend_public_key);
    assert_eq!(tx.destinations[0].view_public_key, hot.view_public_key);
    assert!(!tx.destinations[0].is_subaddress);
    assert!(!tx.destinations[0].is_integrated);
    assert_eq!(tx.subaddr_account, 0);
    assert_eq!(tx.subaddr_indices, vec![0]);
    assert_eq!(
        tx.destination_total_amount,
        transfer_description["summary"]["recipients"][0]["amount"]
            .as_u64()
            .expect("recipient amount should be present")
    );
    assert_eq!(
        tx.change_amount,
        transfer_description["summary"]["change_amount"]
            .as_u64()
            .expect("change amount should be present")
    );
}

#[test]
fn cold_signing_unsigned_txset_derives_source_key_offsets_for_cold_seed() {
    let cold = read_keys_file(&vector_path("cold_full.keys"), "")
        .expect("full cold wallet keys should parse");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let key_images_rpc: KeyImagesRpc = serde_json::from_str(
        &std::fs::read_to_string(vector_path("key_images_rpc.json"))
            .expect("key image RPC fixture should be readable"),
    )
    .expect("key image RPC fixture should parse");

    let result =
        monero_rust::tx_builder::native::derive_wallet2_unsigned_source_key_offsets_from_keys(
            cold.spend_secret_key,
            cold.view_secret_key,
            &unsigned,
        )
        .expect("wallet2 source key offsets should derive from the cold seed");

    assert_eq!(result.archive_version, 2);
    assert_eq!(result.transaction_count, 1);
    assert_eq!(result.source_key_offsets.len(), 2);
    let expected_global_indices = [0usize, 20usize];
    let expected_amounts = [35_184_338_534_400u64, 35_182_996_382_719u64];
    for (index, source) in result.source_key_offsets.iter().enumerate() {
        assert_eq!(source.construction_index, 0);
        assert_eq!(source.source_index, index);
        assert_eq!(
            source.real_global_output_index as usize,
            expected_global_indices[index]
        );
        assert_eq!(source.amount, expected_amounts[index]);
        assert_eq!(source.subaddress_index, Some((0, 0)));
        assert_eq!(source.key_offset.len(), 64);
        assert_eq!(source.output_public_key.len(), 64);
        assert_eq!(source.tx_public_key.len(), 64);
        assert_eq!(
            source.key_image,
            key_images_rpc.signed_key_images[expected_global_indices[index]].key_image
        );
    }

    let wrong_spend =
        monero_rust::tx_builder::native::derive_wallet2_unsigned_source_key_offsets_from_keys(
            [0u8; 32],
            cold.view_secret_key,
            &unsigned,
        )
        .expect_err("wrong spend key must not claim wallet2 unsigned txset sources");
    assert!(wrong_spend.contains("does not belong"));
}

#[test]
fn cold_signing_unsigned_txset_converts_to_app_unsigned_payload() {
    let cold = read_keys_file(&vector_path("cold_full.keys"), "")
        .expect("full cold wallet keys should parse");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let transfer_description: Value = serde_json::from_str(
        &std::fs::read_to_string(vector_path("transfer_description.json"))
            .expect("transfer description should be readable"),
    )
    .expect("transfer description should be valid JSON");
    let key_images_rpc: KeyImagesRpc = serde_json::from_str(
        &std::fs::read_to_string(vector_path("key_images_rpc.json"))
            .expect("key image RPC fixture should be readable"),
    )
    .expect("key image RPC fixture should parse");

    let converted =
        monero_rust::tx_builder::native::create_unsigned_transaction_from_wallet2_txset_keys(
            cold.spend_secret_key,
            cold.view_secret_key,
            &unsigned,
            "mainnet",
        )
        .expect("wallet2 unsigned txset should convert to app unsigned payload");

    assert_eq!(
        converted.fee,
        transfer_description["summary"]["fee"]
            .as_u64()
            .expect("fee should be present")
    );
    assert_eq!(
        converted.recipients,
        vec![(STANDARD_ADDRESS.to_string(), 1_000_000_000_000)]
    );

    let unsigned_bytes =
        hex::decode(&converted.unsigned_tx_hex).expect("converted unsigned tx should be hex");
    let app_unsigned =
        monero_serai::wallet::UnsignedTransaction::read(&mut std::io::Cursor::new(unsigned_bytes))
            .expect("converted unsigned tx should parse as app payload");
    assert_eq!(app_unsigned.inputs.len(), 2);
    assert_eq!(app_unsigned.fee, converted.fee);

    let spend = Zeroizing::new(Scalar::from_bytes_mod_order(cold.spend_secret_key));
    let mut rng = rand::rngs::OsRng;
    let (tx, _, _) = monero_serai::wallet::sign_offline(&mut rng, &spend, app_unsigned)
        .expect("converted wallet2 unsigned payload should sign offline");
    let tx_bytes = tx.serialize();
    Transaction::read(&mut std::io::Cursor::new(tx_bytes.clone()))
        .expect("signed converted tx should reparse");
    let signed_key_images = monero_rust::scanner::extract_key_images_from_raw_tx(&tx_bytes);
    assert_eq!(signed_key_images.len(), 2);
    for expected_index in [0usize, 20usize] {
        assert!(
            signed_key_images.contains(&key_images_rpc.signed_key_images[expected_index].key_image),
            "missing key image for selected output {expected_index}"
        );
    }
}

#[test]
fn cold_signing_signed_txset_parses_wallet2_pending_summary() {
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let signed =
        std::fs::read(vector_path("signed_monero_tx")).expect("signed txset should be readable");
    let transfer_description: Value = serde_json::from_str(
        &std::fs::read_to_string(vector_path("transfer_description.json"))
            .expect("transfer description should be readable"),
    )
    .expect("transfer description should be valid JSON");
    let metadata: Value = serde_json::from_str(
        &std::fs::read_to_string(vector_path("metadata.json"))
            .expect("metadata should be readable"),
    )
    .expect("metadata should be valid JSON");
    let key_images_rpc: KeyImagesRpc = serde_json::from_str(
        &std::fs::read_to_string(vector_path("key_images_rpc.json"))
            .expect("key-image RPC fixture should be readable"),
    )
    .expect("key-image RPC fixture should parse");

    let summary = epee_compat::parse_signed_monero_txset_summary(&signed, &hot.view_secret_key)
        .expect("signed txset pending summary should parse");

    assert_eq!(summary.archive_version, 0);
    assert_eq!(summary.ptxes.len(), 1);
    assert_eq!(summary.key_image_count, 80);
    assert_eq!(summary.key_images.len(), 80);
    for (index, key_image) in summary.key_images.iter().enumerate() {
        assert_eq!(
            hex::encode(key_image),
            key_images_rpc.signed_key_images[index].key_image,
            "signed txset key image #{index}"
        );
    }
    assert_eq!(summary.tx_key_image_count, 2);
    assert_eq!(summary.tx_key_images.len(), 2);
    for (index, pair) in summary.tx_key_images.iter().enumerate() {
        assert!(
            !pair.public_key.iter().all(|byte| *byte == 0),
            "signed txset tx-key/image pair #{index} has zero public key"
        );
        assert!(
            !pair.key_image.iter().all(|byte| *byte == 0),
            "signed txset tx-key/image pair #{index} has zero key image"
        );
    }

    let ptx = &summary.ptxes[0];
    let expected_tx_hash = metadata["flow"]["signed_tx_hash_list"][0]
        .as_str()
        .expect("signed tx hash should be present");
    assert_eq!(hex::encode(ptx.tx_hash), expected_tx_hash);
    assert!(!ptx.tx_blob.is_empty());
    let mut tx_blob_cursor = ptx.tx_blob.as_slice();
    let reparsed_tx = Transaction::read(&mut tx_blob_cursor)
        .expect("extracted signed tx blob should parse as monero-serai transaction");
    assert!(tx_blob_cursor.is_empty());
    assert_eq!(hex::encode(reparsed_tx.hash()), expected_tx_hash);
    assert_eq!(reparsed_tx.serialize(), ptx.tx_blob);
    let extracted =
        epee_compat::extract_signed_monero_txset_transactions(&signed, &hot.view_secret_key)
            .expect("signed txset should extract raw transactions");
    assert_eq!(extracted.len(), 1);
    assert_eq!(hex::encode(extracted[0].tx_hash), expected_tx_hash);
    assert_eq!(extracted[0].tx_blob, ptx.tx_blob);
    assert_eq!(ptx.tx_version, 2);
    assert_eq!(ptx.tx_unlock_time, 0);
    assert_eq!(ptx.tx_input_count, 2);
    assert_eq!(ptx.tx_input_ring_sizes, vec![16, 16]);
    assert_eq!(ptx.tx_output_count, 2);
    assert_eq!(ptx.tx_extra_len, 44);
    assert_eq!(ptx.rct_type, Some(6));
    assert_eq!(ptx.rct_fee, Some(2_599_200_000));
    assert_eq!(ptx.dust, 0);
    assert_eq!(ptx.fee, 2_599_200_000);
    assert!(!ptx.dust_added_to_fee);
    assert_eq!(ptx.change_amount, 69_364_735_717_119);
    assert_eq!(ptx.selected_transfer_count, 2);
    assert_eq!(ptx.selected_transfer_indices, vec![0, 20]);
    assert_eq!(ptx.construction.selected_transfer_indices, vec![0, 20]);
    assert!(ptx.key_images_len > 0);
    assert_eq!(ptx.key_images_len, ptx.key_images_blob.len());
    assert!(!ptx.tx_key_is_zero);
    assert!(!ptx.tx_key.iter().all(|byte| *byte == 0));
    assert_eq!(ptx.additional_tx_key_count, 0);
    assert!(ptx.additional_tx_keys.is_empty());
    assert_eq!(ptx.destination_count, 1);
    assert_eq!(ptx.destination_total_amount, 1_000_000_000_000);
    assert_eq!(ptx.destinations.len(), 1);
    assert_eq!(ptx.destinations[0].amount, 1_000_000_000_000);
    assert_eq!(ptx.destinations[0].original_len, STANDARD_ADDRESS.len());
    assert_eq!(ptx.destinations[0].original, STANDARD_ADDRESS.as_bytes());
    assert_eq!(ptx.destinations[0].spend_public_key, hot.spend_public_key);
    assert_eq!(ptx.destinations[0].view_public_key, hot.view_public_key);
    assert!(!ptx.destinations[0].is_subaddress);
    assert!(!ptx.destinations[0].is_integrated);
    assert_eq!(ptx.multisig_sig_count, 0);
    assert_eq!(
        ptx.destination_total_amount,
        transfer_description["summary"]["recipients"][0]["amount"]
            .as_u64()
            .expect("recipient amount should be present")
    );
    assert_eq!(
        ptx.change_amount,
        transfer_description["summary"]["change_amount"]
            .as_u64()
            .expect("change amount should be present")
    );
    assert_eq!(ptx.construction.source_count, 2);
    assert_eq!(ptx.construction.source_ring_sizes, vec![16, 16]);
    assert_eq!(
        ptx.construction.sources,
        summary_txset_sources_for_signed_fixture()
    );
    assert_eq!(
        ptx.construction.destination_total_amount,
        ptx.destination_total_amount
    );
    assert_eq!(ptx.construction.change_amount, ptx.change_amount);
}

#[test]
fn cold_signing_signed_txset_builder_roundtrips_wallet2_summary() {
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let signed =
        std::fs::read(vector_path("signed_monero_tx")).expect("signed txset should be readable");

    let signed_summary =
        epee_compat::parse_signed_monero_txset_summary(&signed, &hot.view_secret_key)
            .expect("signed txset pending summary should parse");
    let source_ptx = signed_summary
        .ptxes
        .first()
        .expect("fixture should have one pending tx");

    let rebuilt = epee_compat::build_signed_monero_txset(epee_compat::BuildSignedTxSetRequest {
        unsigned_txset: &unsigned,
        view_secret_key: &hot.view_secret_key,
        tx_blob: &source_ptx.tx_blob,
        key_images: &signed_summary.key_images,
        tx_key_images: &signed_summary.tx_key_images,
    })
    .expect("signed txset should rebuild from parsed wallet2 fields");

    let rebuilt_summary =
        epee_compat::parse_signed_monero_txset_summary(&rebuilt, &hot.view_secret_key)
            .expect("rebuilt signed txset should parse");
    assert_eq!(rebuilt_summary.archive_version, 0);
    assert_eq!(rebuilt_summary.ptxes.len(), 1);
    assert_eq!(rebuilt_summary.key_images, signed_summary.key_images);
    assert_eq!(rebuilt_summary.tx_key_images, signed_summary.tx_key_images);

    let rebuilt_ptx = &rebuilt_summary.ptxes[0];
    assert_eq!(rebuilt_ptx.tx_hash, source_ptx.tx_hash);
    assert_eq!(rebuilt_ptx.tx_blob, source_ptx.tx_blob);
    assert_eq!(rebuilt_ptx.fee, source_ptx.fee);
    assert_eq!(rebuilt_ptx.dust, 0);
    assert!(!rebuilt_ptx.dust_added_to_fee);
    assert_eq!(rebuilt_ptx.change_amount, source_ptx.change_amount);
    assert_eq!(
        rebuilt_ptx.selected_transfer_indices,
        source_ptx.selected_transfer_indices
    );
    assert_eq!(
        rebuilt_ptx.destination_total_amount,
        source_ptx.destination_total_amount
    );
    assert_eq!(rebuilt_ptx.destinations, source_ptx.destinations);
    assert_eq!(rebuilt_ptx.construction, source_ptx.construction);
    assert_eq!(rebuilt_ptx.tx_key, source_ptx.tx_key);

    let extracted =
        epee_compat::extract_signed_monero_txset_transactions(&rebuilt, &hot.view_secret_key)
            .expect("rebuilt signed txset should expose raw tx");
    assert_eq!(extracted.len(), 1);
    assert_eq!(extracted[0].tx_hash, source_ptx.tx_hash);
    assert_eq!(extracted[0].tx_blob, source_ptx.tx_blob);
}

#[test]
fn cold_signing_signed_txset_builder_rejects_sparse_key_image_vector() {
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let signed =
        std::fs::read(vector_path("signed_monero_tx")).expect("signed txset should be readable");

    let signed_summary =
        epee_compat::parse_signed_monero_txset_summary(&signed, &hot.view_secret_key)
            .expect("signed txset pending summary should parse");
    let source_ptx = signed_summary
        .ptxes
        .first()
        .expect("fixture should have one pending tx");
    let sparse_key_images = &signed_summary.key_images[..2];

    let err = epee_compat::build_signed_monero_txset(epee_compat::BuildSignedTxSetRequest {
        unsigned_txset: &unsigned,
        view_secret_key: &hot.view_secret_key,
        tx_blob: &source_ptx.tx_blob,
        key_images: sparse_key_images,
        tx_key_images: &signed_summary.tx_key_images,
    })
    .expect_err("sparse key-image vector should be rejected");

    assert!(
        err.contains("selected transfer index 20"),
        "unexpected error: {err}"
    );
}

fn summary_txset_sources_for_signed_fixture() -> Vec<epee_compat::TxSourceEntrySummary> {
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    epee_compat::parse_unsigned_monero_txset_summary(&unsigned, &hot.view_secret_key)
        .expect("unsigned txset construction summary should parse")
        .txes
        .into_iter()
        .next()
        .expect("unsigned txset should contain one transaction")
        .sources
}

#[test]
fn cold_signing_wallet2_unsigned_txset_signs_with_imported_cold_wallet_keys() {
    let cold = read_keys_file(&vector_path("cold_full.keys"), "")
        .expect("full cold wallet keys should parse");
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let unsigned_hex = hex::encode(unsigned);
    let key_images_rpc: KeyImagesRpc = serde_json::from_str(
        &std::fs::read_to_string(vector_path("key_images_rpc.json"))
            .expect("key-image RPC fixture should be readable"),
    )
    .expect("key-image RPC fixture should parse");

    let signed = monero_rust::native::sign_unsigned_transaction_with_private_keys(
        cold.spend_secret_key,
        cold.view_secret_key,
        &unsigned_hex,
        "mainnet",
    )
    .expect("wallet2 unsigned txset should sign through app signer");
    assert_eq!(signed.fee, 2_599_200_000);
    assert!(signed.tx_id.len() == 64);
    assert!(!signed.tx_blob.is_empty());
    let tx_bytes = hex::decode(&signed.tx_blob).expect("signed tx blob should be hex");
    Transaction::read(&mut std::io::Cursor::new(tx_bytes))
        .expect("signed wallet2-converted tx should parse");
    assert_eq!(signed.spent_key_images.len(), 2);
    for expected_index in [0usize, 20usize] {
        assert!(
            signed
                .spent_key_images
                .contains(&key_images_rpc.signed_key_images[expected_index].key_image),
            "missing key image for selected output {expected_index}"
        );
    }
}

#[test]
fn cold_signing_wallet2_unsigned_txset_fails_app_signer_for_wrong_seed() {
    let unsigned = std::fs::read(vector_path("unsigned_monero_tx"))
        .expect("unsigned txset should be readable");
    let unsigned_hex = hex::encode(unsigned);

    let err = monero_rust::native::sign_unsigned_transaction(
        "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime",
        &unsigned_hex,
        "mainnet",
    )
    .expect_err("wrong seed should not sign wallet2 unsigned txset");

    assert!(
        err.contains("Failed to convert wallet2 unsigned txset"),
        "unexpected error: {err}"
    );
}

#[test]
fn cold_signing_wallet_files_parse_as_full_and_view_only_pair() {
    let cold = read_keys_file(&vector_path("cold_full.keys"), "")
        .expect("full cold wallet keys should parse");
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");

    assert!(!cold.watch_only);
    assert!(hot.watch_only);
    assert!(cold.mnemonic.is_some());
    assert!(hot.mnemonic.is_none());
    assert_eq!(hot.spend_secret_key, [0u8; 32]);
    assert_eq!(cold.spend_public_key, hot.spend_public_key);
    assert_eq!(cold.view_public_key, hot.view_public_key);
    assert_eq!(cold.view_secret_key, hot.view_secret_key);
}

#[test]
fn cold_signing_output_export_decrypts_for_generated_wallet() {
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let outputs =
        std::fs::read(vector_path("outputs")).expect("output export fixture should be readable");

    let preview = epee_compat::inspect_output_export(&outputs, &hot.view_secret_key)
        .expect("output export should decrypt with generated view key");

    assert_eq!(preview.public_spend_key, hot.spend_public_key);
    assert_eq!(preview.public_view_key, hot.view_public_key);
    assert_eq!(preview.archive_body_len, 6244);
}

#[test]
fn cold_signing_output_export_parses_exported_transfer_details() {
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let outputs =
        std::fs::read(vector_path("outputs")).expect("output export fixture should be readable");

    let parsed = epee_compat::parse_output_export(&outputs, &hot.view_secret_key)
        .expect("output export should parse with generated view key");

    assert_eq!(parsed.public_spend_key, hot.spend_public_key);
    assert_eq!(parsed.public_view_key, hot.view_public_key);
    assert_eq!(parsed.offset, 0);
    assert_eq!(parsed.total_outputs, 80);
    assert_eq!(parsed.outputs.len(), 80);

    for (index, output) in parsed.outputs.iter().enumerate() {
        assert_eq!(output.internal_output_index, 0, "output #{index}");
        assert_eq!(output.global_output_index, index as u64, "output #{index}");
        assert_eq!(output.additional_tx_keys.len(), 0, "output #{index}");
        assert_eq!(output.subaddress_major, 0, "output #{index}");
        assert_eq!(output.subaddress_minor, 0, "output #{index}");
        assert!(output.amount > 0, "output #{index}");
        assert!(!output.flags.spent, "output #{index}");
        assert!(!output.flags.frozen, "output #{index}");
        assert!(!output.flags.key_image_known, "output #{index}");
        assert!(output.flags.key_image_request, "output #{index}");
        assert!(
            !output.output_public_key.iter().all(|byte| *byte == 0),
            "output #{index}"
        );
        assert!(
            !output.tx_public_key.iter().all(|byte| *byte == 0),
            "output #{index}"
        );
    }
}

#[test]
fn cold_signing_metadata_matches_generated_flow() {
    let data =
        std::fs::read_to_string(vector_path("metadata.json")).expect("metadata should be readable");
    let metadata: Value = serde_json::from_str(&data).expect("metadata should be valid JSON");

    assert_eq!(metadata["schema"], "monero-rust cold signing vector v1");
    assert_eq!(metadata["network"], "regtest");
    assert_eq!(metadata["wallet"]["standard_address"], STANDARD_ADDRESS);
    assert_eq!(metadata["wallet"]["cold_restore_address"], STANDARD_ADDRESS);
    assert_eq!(metadata["flow"]["blocks_mined"], 80);
    assert_eq!(metadata["flow"]["incoming_transfer_count"], 80);
    assert_eq!(metadata["flow"]["outputs_imported"], 80);
    assert_eq!(metadata["flow"]["key_image_count"], 80);
    assert_eq!(metadata["flow"]["key_image_offset"], 0);
    assert_eq!(metadata["flow"]["ring_size"], 16);
    assert_eq!(
        metadata["flow"]["transfer_amount_atomic"],
        1_000_000_000_000u64
    );
    assert_eq!(
        metadata["flow"]["signed_tx_hash_list"],
        metadata["flow"]["submitted_tx_hash_list"]
    );

    for name in [
        "cold_full.keys",
        "hot_view_only.keys",
        "key_images_rpc.json",
        "outputs",
        "signed_monero_tx",
        "transfer_description.json",
        "unsigned_monero_tx",
    ] {
        let expected = metadata["artifacts"][name]["bytes"]
            .as_u64()
            .unwrap_or_else(|| panic!("missing byte count for {name}"));
        let actual = std::fs::metadata(vector_path(name))
            .unwrap_or_else(|_| panic!("missing artifact {name}"))
            .len();
        assert_eq!(actual, expected, "{name} byte count should match metadata");
    }
}

#[test]
fn cold_signing_key_image_rpc_vector_has_expected_shape() {
    let data = std::fs::read_to_string(vector_path("key_images_rpc.json"))
        .expect("key-image RPC fixture should be readable");
    let key_images: KeyImagesRpc =
        serde_json::from_str(&data).expect("key-image RPC fixture should parse");

    assert_eq!(key_images.offset, 0);
    assert_eq!(key_images.signed_key_images.len(), 80);
    for (index, signed) in key_images.signed_key_images.iter().enumerate() {
        assert_eq!(signed.key_image.len(), 64, "key image #{index}");
        assert_eq!(signed.signature.len(), 128, "signature #{index}");
        assert_eq!(
            hex::decode(&signed.key_image)
                .expect("valid key-image hex")
                .len(),
            32
        );
        assert_eq!(
            hex::decode(&signed.signature)
                .expect("valid signature hex")
                .len(),
            64
        );
    }
}

#[test]
fn cold_signing_keys_build_view_only_sentinel_used_by_app_imports() {
    let cold = read_keys_file(&vector_path("cold_full.keys"), "")
        .expect("full cold wallet keys should parse");
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let metadata: Value = serde_json::from_str(
        &std::fs::read_to_string(vector_path("metadata.json"))
            .expect("metadata should be readable"),
    )
    .expect("metadata should be valid JSON");

    let sentinel = format!(
        "viewonly:{}:{}",
        hex::encode(hot.view_secret_key),
        hex::encode(hot.spend_public_key)
    );

    assert_eq!(
        hex::encode(hot.view_secret_key),
        metadata["wallet"]["view_key"]
    );
    assert_eq!(
        hex::encode(cold.view_secret_key),
        metadata["wallet"]["view_key"]
    );
    assert_eq!(sentinel.len(), "viewonly::".len() + 64 + 64);
    assert!(monero_rust::parse_view_only_keys(&sentinel).is_some());
}

#[test]
fn cold_signing_key_image_rpc_vector_flows_through_app_import_state() {
    let data = std::fs::read_to_string(vector_path("key_images_rpc.json"))
        .expect("key-image RPC fixture should be readable");
    let (offset, signed_key_images) =
        parse_rpc_export(&data).expect("key-image RPC fixture should parse via app API");
    let key_images = extract_key_image_hex(&signed_key_images);

    assert_eq!(offset, 0);
    assert_eq!(key_images.len(), 80);

    let mut state = WalletState::new();
    state.replace_outputs(
        (0..key_images.len())
            .map(|index| WalletOutput {
                tx_hash: format!("{index:064x}"),
                output_index: 0,
                amount: 1,
                amount_xmr: "0.000000000001".to_string(),
                key: String::new(),
                key_offset: String::new(),
                commitment_mask: String::new(),
                subaddress_index: Some((0, 0)),
                payment_id: None,
                received_output_bytes: String::new(),
                block_height: index as u64,
                spent: false,
                spent_height: None,
                key_image: String::new(),
                is_coinbase: true,
                frozen: false,
            })
            .collect(),
    );

    let updated = state.import_key_images(offset, &key_images);

    assert_eq!(updated, key_images.len());
    let restored_outputs = state.outputs();
    assert_eq!(restored_outputs.len(), key_images.len());
    for (output, key_image) in restored_outputs.iter().zip(key_images) {
        assert_eq!(output.key_image, key_image);
    }
}

#[test]
fn cold_signing_full_export_imports_through_view_only_sentinel() {
    let cold = read_keys_file(&vector_path("cold_full.keys"), "")
        .expect("full cold wallet keys should parse");
    let hot = read_keys_file(&vector_path("hot_view_only.keys"), "")
        .expect("hot view-only wallet keys should parse");
    let mnemonic = cold
        .mnemonic
        .as_deref()
        .expect("full cold wallet should include mnemonic");
    let view_only_sentinel = format!(
        "viewonly:{}:{}",
        hex::encode(hot.view_secret_key),
        hex::encode(hot.spend_public_key)
    );

    let (later, later_ki) = synthetic_output_for_spend_key(cold.spend_secret_key, 7, 20, 0);
    let (earlier, earlier_ki) = synthetic_output_for_spend_key(cold.spend_secret_key, 3, 10, 1);
    let mut export_order = vec![later.clone(), earlier.clone()];
    export_order.sort_by(|a, b| {
        a.block_height
            .cmp(&b.block_height)
            .then(a.output_index.cmp(&b.output_index))
    });

    let exported = key_image_signing::export_key_images_from_outputs(mnemonic, "", &export_order)
        .expect("full wallet key images should export");
    assert!(exported.starts_with(epee_compat::KEY_IMAGES_MAGIC));

    let imported =
        key_image_signing::import_key_images_from_seed(&view_only_sentinel, "", &exported)
            .expect("view-only sentinel should decrypt full-wallet key-image export");
    assert_eq!(imported, vec![earlier_ki.clone(), later_ki.clone()]);

    let mut view_only_outputs = export_order;
    for output in &mut view_only_outputs {
        output.key_image.clear();
    }

    let mut state = WalletState::new();
    state.replace_outputs(view_only_outputs);
    let updated = state.import_key_images(0, &imported);
    let outputs = state.outputs();

    assert_eq!(updated, 2);
    assert_eq!(outputs[0].key_image, earlier_ki);
    assert_eq!(outputs[1].key_image, later_ki);
}
