//! The view-only wallet against hostile key image exports and signed txsets.

mod cold_signing_common;

use cold_signing_common::*;
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE,
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
};
use monero_rust::crypto::generate_key_image;
use monero_rust::epee_compat::{self, ExportedKeyImage};
use monero_rust::key_image_import::{
    verify_key_image_signature, SignedKeyImage, SignedKeyImageImport,
};
use monero_rust::key_image_signing::{
    generate_key_image_ring_signature, import_key_images_from_seed,
    import_signed_key_images_from_seed,
};
use monero_rust::{WalletOutput, WalletState};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

fn view_only_sentinel() -> String {
    let hot = hot_keys();
    format!(
        "viewonly:{}:{}",
        hex::encode(hot.view_secret_key),
        hex::encode(hot.spend_public_key)
    )
}

fn owned_output(tag: &str) -> (Zeroizing<Scalar>, EdwardsPoint, [u8; 32]) {
    let spend = Scalar::from_bytes_mod_order(cold_keys().spend_secret_key);
    let offset = Scalar::from_bytes_mod_order(Keccak256::digest(tag.as_bytes()).into());
    let secret = Zeroizing::new(spend + offset);
    let public = &*secret * ED25519_BASEPOINT_TABLE;
    let image = generate_key_image(&secret).compress().to_bytes();
    (secret, public, image)
}

fn state_output(tx: &str, key: &EdwardsPoint, height: u64) -> WalletOutput {
    WalletOutput {
        tx_hash: tx.repeat(64),
        output_index: 0,
        amount: 1_000_000_000_000,
        key: hex::encode(key.compress().to_bytes()),
        key_offset: String::new(),
        commitment_mask: String::new(),
        subaddress_index: Some((0, 0)),
        payment_id: None,
        received_output_bytes: String::new(),
        global_index: Some(height),
        ring: None,
        block_height: height,
        spent: false,
        spent_height: None,
        key_image: String::new(),
        is_coinbase: false,
        frozen: false,
    }
}

fn signed(tag: &str) -> (EdwardsPoint, SignedKeyImage) {
    let (secret, public, image) = owned_output(tag);
    let (c, r) = generate_key_image_ring_signature(&image, &public, &secret);
    let mut signature = [0u8; 64];
    signature[..32].copy_from_slice(&c);
    signature[32..].copy_from_slice(&r);
    (
        public,
        SignedKeyImage {
            key_image: image,
            signature,
        },
    )
}

#[test]
fn verified_import_refuses_a_valid_image_of_another_output() {
    let (key_a, image_a) = signed("a");
    let (key_b, image_b) = signed("b");
    let mut state = WalletState::new();
    state.replace_outputs(vec![
        state_output("a", &key_a, 10),
        state_output("b", &key_b, 11),
    ]);

    let swapped = SignedKeyImageImport {
        offset: 0,
        key_images: vec![image_b.clone(), image_a.clone()],
    };
    assert!(state
        .assign_verified_key_images_in_export_order(&swapped)
        .is_err());
    assert!(state.outputs().iter().all(|o| o.key_image.is_empty()));

    let honest = SignedKeyImageImport {
        offset: 0,
        key_images: vec![image_a, image_b],
    };
    assert_eq!(
        state
            .assign_verified_key_images_in_export_order(&honest)
            .unwrap(),
        2
    );
}

#[test]
fn unauthenticated_legacy_key_images_are_not_accepted_for_a_known_account() {
    let (key_a, _honest) = signed("a");
    let bogus = "ab".repeat(32);
    #[allow(deprecated)]
    let forged = epee_compat::export_key_images(&[ExportedKeyImage {
        key_image: bogus.clone(),
        tx_hash: "00".repeat(32),
        output_index: 0,
    }])
    .unwrap();

    let imported = import_key_images_from_seed(&view_only_sentinel(), "", &forged);

    if let Ok(images) = &imported {
        let mut state = WalletState::new();
        state.replace_outputs(vec![state_output("a", &key_a, 10)]);
        let assigned = state.import_key_images(0, images);
        eprintln!(
            "forged list accepted: {images:?}; assigned to {assigned} output(s): {:?}",
            state.outputs()[0].key_image
        );
        assert_eq!(state.outputs()[0].key_image, bogus);
    }
    assert!(
        imported.is_err(),
        "an unauthenticated, unsigned key image list was accepted for a known account"
    );
}

#[test]
fn signed_import_refuses_the_unsigned_legacy_list() {
    #[allow(deprecated)]
    let forged = epee_compat::export_key_images(&[ExportedKeyImage {
        key_image: "ab".repeat(32),
        tx_hash: "00".repeat(32),
        output_index: 0,
    }])
    .unwrap();
    let err = import_signed_key_images_from_seed(&view_only_sentinel(), "", &forged).unwrap_err();
    assert!(err.contains("no ring signature"), "{err}");
}

#[test]
#[ignore = "known defect: non-canonical signature scalars verify"]
fn key_image_signature_scalars_must_be_canonical() {
    let (public, mut entry) = signed("canonical");
    let image = CompressedEdwardsY(entry.key_image).decompress().unwrap();
    assert!(verify_key_image_signature(
        &public,
        &image,
        &entry.signature
    ));

    let l: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde,
        0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
    ];
    let mut carry = 0u16;
    for i in 0..32 {
        let sum = u16::from(entry.signature[32 + i]) + u16::from(l[i]) + carry;
        entry.signature[32 + i] = sum as u8;
        carry = sum >> 8;
    }
    assert_eq!(carry, 0, "r + l fits in 32 bytes");
    assert!(
        !verify_key_image_signature(&public, &image, &entry.signature),
        "a non-canonical signature scalar verified; wallet2's sc_check rejects it"
    );
}

#[test]
fn duplicate_one_time_keys_do_not_break_wallet2_ordered_imports() {
    let (key_a, image_a) = signed("a");
    let (key_c, image_c) = signed("c");
    let mut state = WalletState::new();
    state.replace_outputs(vec![
        state_output("a", &key_a, 10),
        state_output("b", &key_a, 11),
        state_output("c", &key_c, 12),
    ]);

    let wallet2_order = SignedKeyImageImport {
        offset: 0,
        key_images: vec![image_a.clone(), image_c.clone()],
    };
    let first = state.assign_verified_key_images_in_export_order(&wallet2_order);

    let both = SignedKeyImageImport {
        offset: 0,
        key_images: vec![image_a.clone(), image_a, image_c],
    };
    let second = state.assign_verified_key_images_in_export_order(&both);

    assert!(
        first.is_ok() || second.is_ok(),
        "no key image export can be imported once two outputs share a one-time key: \
         wallet2 order: {first:?}; with both: {second:?}"
    );
}

#[test]
#[ignore = "known defect: an output that replaces a smaller one with the same one-time key leaves wallet2's export position"]
fn larger_output_to_a_held_key_takes_the_earlier_export_position() {
    let (key_a, image_a) = signed("a");
    let (key_c, image_c) = signed("c");
    let mut larger = state_output("b", &key_a, 13);
    larger.amount *= 2;
    let mut state = WalletState::new();
    state.replace_outputs(vec![
        state_output("a", &key_a, 10),
        state_output("c", &key_c, 12),
        larger,
    ]);

    let wallet2_order = SignedKeyImageImport {
        offset: 0,
        key_images: vec![image_a.clone(), image_c.clone()],
    };
    state
        .assign_verified_key_images_in_export_order(&wallet2_order)
        .expect("wallet2's export of this wallet imports");
    let image_of = |key: &EdwardsPoint| {
        state
            .outputs()
            .iter()
            .find(|o| o.key == hex::encode(key.compress().to_bytes()))
            .map(|o| (o.amount, o.key_image.clone()))
    };
    assert_eq!(
        image_of(&key_a),
        Some((2_000_000_000_000, hex::encode(image_a.key_image)))
    );
    assert_eq!(
        image_of(&key_c),
        Some((1_000_000_000_000, hex::encode(image_c.key_image)))
    );
}

#[test]
fn signed_txset_key_images_text_matches_wallet2() {
    let hot = hot_keys();
    let unsigned = vector_unsigned_txset();
    let wallet2_signed = std::fs::read(vector_path("monero_wallet_rpc_signed_monero_tx")).unwrap();
    let wallet2 =
        epee_compat::parse_signed_monero_txset_summary(&wallet2_signed, &hot.view_secret_key)
            .unwrap();
    let wallet2_text = String::from_utf8(wallet2.ptxes[0].key_images_blob.clone()).unwrap();
    assert!(
        wallet2_text.starts_with('<') && wallet2_text.ends_with("> "),
        "wallet2's own text: {wallet2_text:?}"
    );

    let rebuilt = epee_compat::build_signed_monero_txset(epee_compat::BuildSignedTxSetRequest {
        unsigned_txset: &unsigned,
        view_secret_key: &hot.view_secret_key,
        tx_blobs: vec![&wallet2.ptxes[0].tx_blob],
        key_images: &wallet2.key_images,
        tx_key_images: &wallet2.tx_key_images,
    })
    .unwrap();
    let crate_set =
        epee_compat::parse_signed_monero_txset_summary(&rebuilt, &hot.view_secret_key).unwrap();
    let crate_text = String::from_utf8(crate_set.ptxes[0].key_images_blob.clone()).unwrap();
    assert_eq!(
        crate_text, wallet2_text,
        "the crate's pending_tx::key_images text differs from wallet2's for the same transaction"
    );
}

#[test]
fn signed_txset_reader_checks_the_transaction_against_its_construction_data() {
    use monero_rust::native::{
        preview_unsigned_transaction, sign_unsigned_transaction_with_private_keys,
    };
    let cold = cold_keys();
    let hot = hot_keys();

    let mut tampered = parse_vector_unsigned();
    let burned = 5_000_000_000_000u64;
    tampered.txes[0].change.amount -= burned;
    tampered.txes[0].change_amount -= burned;
    tampered.txes[0].split_destinations[0].amount -= burned;
    let encoded = hex::encode(seal_unsigned_txset(&tampered, &cold.view_secret_key));
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
    assert_eq!(signed.fee, 2_599_200_000 + burned);

    let wallet2_signed = std::fs::read(vector_path("monero_wallet_rpc_signed_monero_tx")).unwrap();
    let wallet2 =
        epee_compat::parse_signed_monero_txset_summary(&wallet2_signed, &hot.view_secret_key)
            .unwrap();
    let blob = hex::decode(&signed.tx_blob).unwrap();
    let forged = epee_compat::build_signed_monero_txset(epee_compat::BuildSignedTxSetRequest {
        unsigned_txset: &vector_unsigned_txset(),
        view_secret_key: &hot.view_secret_key,
        tx_blobs: vec![&blob],
        key_images: &wallet2.key_images,
        tx_key_images: &[],
    })
    .expect("the writer checks the spent key images, not the fee");

    let summary =
        epee_compat::parse_signed_monero_txset_summary(&forged, &hot.view_secret_key).unwrap();
    eprintln!(
        "pending fee {} vs transaction fee {:?}",
        summary.ptxes[0].fee, summary.ptxes[0].rct_fee
    );
    assert_ne!(Some(summary.ptxes[0].fee), summary.ptxes[0].rct_fee);
    let extracted =
        epee_compat::extract_signed_monero_txset_transactions(&forged, &hot.view_secret_key);
    assert!(
        extracted.is_err(),
        "a signed txset whose transaction pays a different fee than its construction data \
         was handed out for relay"
    );
}

#[test]
fn signed_txset_is_tied_to_the_unsigned_txset_it_answers() {
    let hot = hot_keys();
    let wallet2_signed = std::fs::read(vector_path("monero_wallet_rpc_signed_monero_tx")).unwrap();
    let answered = epee_compat::extract_signed_monero_txset_transactions_for_unsigned(
        &wallet2_signed,
        &hot.view_secret_key,
        &vector_unsigned_txset(),
    )
    .expect("wallet2's signed txset answers the vector's unsigned txset");
    assert_eq!(answered.len(), 1);

    let mut other = parse_vector_unsigned();
    other.txes[0].change.amount -= 1;
    other.txes[0].change_amount -= 1;
    other.txes[0].split_destinations[0].amount -= 1;
    let err = epee_compat::extract_signed_monero_txset_transactions_for_unsigned(
        &wallet2_signed,
        &hot.view_secret_key,
        &seal_unsigned_txset(&other, &hot.view_secret_key),
    )
    .expect_err("a signed txset for another request must be refused");
    assert!(
        err.contains("not built from the construction data"),
        "{err}"
    );
}

#[test]
fn signed_txset_with_foreign_key_images_is_refused() {
    let hot = hot_keys();
    let wallet2_signed = std::fs::read(vector_path("monero_wallet_rpc_signed_monero_tx")).unwrap();
    let wallet2 =
        epee_compat::parse_signed_monero_txset_summary(&wallet2_signed, &hot.view_secret_key)
            .unwrap();
    monero_rust::txset_checks::check_signed_monero_txset(&wallet2)
        .expect("wallet2's signed txset passes");

    let mut swapped = wallet2.clone();
    let selected = swapped.ptxes[0].selected_transfer_indices[0] as usize;
    swapped.key_images[selected] = swapped.key_images[selected + 1];
    let err = monero_rust::txset_checks::check_signed_monero_txset(&swapped).unwrap_err();
    assert!(err.contains("ki mismatch"), "{err}");

    let mut other_ring = wallet2.clone();
    other_ring.ptxes[0].construction.sources[0].ring[3].global_output_index += 1;
    let err = monero_rust::txset_checks::check_signed_monero_txset(&other_ring).unwrap_err();
    assert!(err.contains("key offsets mismatch"), "{err}");
}
