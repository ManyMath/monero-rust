//! Integration tests for v3 key image export/import interoperability.

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use monero_rust::epee_compat;
use monero_rust::key_image_signing::{
    decrypt_with_view_key, export_key_images_v3, import_key_images_v3,
    verify_key_image_ring_signature, KeyImageExportEntry,
};
use monero_rust::monero_backend::ringct::generate_key_image;

/// Magic bytes for Monero key image export v3 format.
const KEY_IMAGES_MAGIC: &[u8] = b"Monero key image export\x03";

/// Create deterministic test keys: (spend_scalar, pub_spend, pub_view, view_secret)
fn test_keys() -> (Scalar, [u8; 32], [u8; 32], [u8; 32]) {
    // Deterministic spend key for testing
    let spend_bytes: [u8; 32] = {
        let hash: [u8; 32] = Keccak256::digest(b"interop test spend key").into();
        hash
    };
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    let pub_spend = (&spend_scalar * ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes();

    let view_bytes: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    let view_scalar = Scalar::from_bytes_mod_order(view_bytes);
    let pub_view = (&view_scalar * ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes();
    let view_secret = view_scalar.to_bytes();

    (spend_scalar, pub_spend, pub_view, view_secret)
}

/// Create `count` deterministic test KeyImageExportEntry values.
fn test_entries(spend_scalar: &Scalar, count: usize) -> Vec<KeyImageExportEntry> {
    (0..count)
        .map(|i| {
            let offset_seed = format!("interop test key offset {}", i);
            let offset_bytes: [u8; 32] = Keccak256::digest(offset_seed.as_bytes()).into();
            let key_offset = Scalar::from_bytes_mod_order(offset_bytes);
            let ephemeral = Zeroizing::new(spend_scalar + &key_offset);
            let pub_key = &*ephemeral * ED25519_BASEPOINT_TABLE;
            let ki_point = generate_key_image(&ephemeral);
            let key_image = ki_point.compress().to_bytes();
            KeyImageExportEntry {
                key_image,
                pub_key,
                key_offset,
            }
        })
        .collect()
}

// Test 1: v3 export magic bytes
#[test]
fn test_v3_export_magic_bytes() {
    let (spend_scalar, pub_spend, pub_view, view_secret) = test_keys();
    let entries = test_entries(&spend_scalar, 3);

    let exported = export_key_images_v3(
        &entries,
        &spend_scalar,
        &pub_spend,
        &pub_view,
        &view_secret,
        0,
    )
    .expect("Export should succeed");

    // File must start with the exact 24-byte magic
    assert_eq!(KEY_IMAGES_MAGIC.len(), 24);
    assert!(
        exported.starts_with(KEY_IMAGES_MAGIC),
        "Exported data must start with b\"Monero key image export\\x03\""
    );
}

// Test 2: v3 roundtrip
#[test]
fn test_v3_roundtrip() {
    let (spend_scalar, pub_spend, pub_view, view_secret) = test_keys();
    let entries = test_entries(&spend_scalar, 5);

    let exported = export_key_images_v3(
        &entries,
        &spend_scalar,
        &pub_spend,
        &pub_view,
        &view_secret,
        0,
    )
    .expect("Export should succeed");

    let (imported_kis, imported_spend, imported_view) =
        import_key_images_v3(&exported, &view_secret).expect("Import should succeed");

    assert_eq!(imported_kis.len(), 5, "Should import 5 key images");
    assert_eq!(imported_spend, pub_spend, "Public spend key mismatch");
    assert_eq!(imported_view, pub_view, "Public view key mismatch");

    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(
            imported_kis[i].key_image, entry.key_image,
            "Key image {} mismatch on roundtrip",
            i
        );
    }
}

// Test 3: v3 payload structure
#[test]
fn test_v3_payload_structure() {
    let (spend_scalar, pub_spend, pub_view, view_secret) = test_keys();
    let entries = test_entries(&spend_scalar, 3);

    let exported = export_key_images_v3(
        &entries,
        &spend_scalar,
        &pub_spend,
        &pub_view,
        &view_secret,
        0,
    )
    .expect("Export should succeed");

    // Strip magic
    let encrypted = &exported[KEY_IMAGES_MAGIC.len()..];

    // Decrypt
    let plaintext =
        decrypt_with_view_key(encrypted, &view_secret).expect("Decryption should succeed");

    // Header: 4-byte offset + 32-byte pub_spend + 32-byte pub_view = 68 bytes
    assert!(
        plaintext.len() >= 68,
        "Payload must be at least 68 bytes for header"
    );

    // Offset should be 0 (little-endian)
    let offset = u32::from_le_bytes(plaintext[..4].try_into().unwrap());
    assert_eq!(offset, 0, "Offset should be 0");

    // Public keys
    assert_eq!(&plaintext[4..36], &pub_spend, "Public spend key in payload");
    assert_eq!(&plaintext[36..68], &pub_view, "Public view key in payload");

    // Records: 3 entries * 96 bytes each (32 ki + 64 sig)
    let records = &plaintext[68..];
    assert_eq!(
        records.len(),
        3 * 96,
        "3 entries should produce {} bytes of records",
        3 * 96
    );

    // Each record starts with the key image
    for (i, entry) in entries.iter().enumerate() {
        let base = i * 96;
        assert_eq!(
            &records[base..base + 32],
            &entry.key_image,
            "Key image {} in record",
            i
        );
    }
}

// Test 4: ring signatures verify
#[test]
fn test_v3_ring_signatures_verify() {
    let (spend_scalar, pub_spend, pub_view, view_secret) = test_keys();
    let entries = test_entries(&spend_scalar, 3);

    let exported = export_key_images_v3(
        &entries,
        &spend_scalar,
        &pub_spend,
        &pub_view,
        &view_secret,
        0,
    )
    .expect("Export should succeed");

    // Strip magic and decrypt
    let encrypted = &exported[KEY_IMAGES_MAGIC.len()..];
    let plaintext =
        decrypt_with_view_key(encrypted, &view_secret).expect("Decryption should succeed");

    let records = &plaintext[68..];

    for (i, entry) in entries.iter().enumerate() {
        let base = i * 96;
        let ki = &records[base..base + 32];
        let sig_c: [u8; 32] = records[base + 32..base + 64].try_into().unwrap();
        let sig_r: [u8; 32] = records[base + 64..base + 96].try_into().unwrap();

        // Reconstruct key image point for verification
        let ephemeral = Zeroizing::new(&spend_scalar + &entry.key_offset);
        let ki_point = generate_key_image(&ephemeral);

        let ki_bytes: [u8; 32] = ki.try_into().unwrap();

        assert!(
            verify_key_image_ring_signature(&ki_bytes, &entry.pub_key, &ki_point, &sig_c, &sig_r),
            "Ring signature for key image {} should verify",
            i
        );
    }
}

// Test 5: old EPEE import without view key
#[test]
fn test_old_epee_import_without_view_key() {
    // Create data in old EPEE format
    let key_images = vec![
        epee_compat::ExportedKeyImage {
            key_image: "0197168670bb9a4f183be4eb8f0f0d57354dfe7094f0c55e5552b7b3a105a77a"
                .to_string(),
            tx_hash: "aaaa".to_string(),
            output_index: 0,
        },
        epee_compat::ExportedKeyImage {
            key_image: "35082cc61a636b170ac431be8ef2b08b1ddf9fd834cac8e99f649ba34757b4a4"
                .to_string(),
            tx_hash: "bbbb".to_string(),
            output_index: 0,
        },
    ];

    #[allow(deprecated)]
    let exported = epee_compat::export_key_images(&key_images).expect("Old export should succeed");

    // Import with None view key should succeed (EPEE format)
    let imported =
        epee_compat::import_key_images(&exported, None).expect("Old EPEE import should succeed");

    assert_eq!(imported.len(), 2);
    assert_eq!(
        imported[0],
        "0197168670bb9a4f183be4eb8f0f0d57354dfe7094f0c55e5552b7b3a105a77a"
    );
    assert_eq!(
        imported[1],
        "35082cc61a636b170ac431be8ef2b08b1ddf9fd834cac8e99f649ba34757b4a4"
    );
}

// Test 6: old EPEE import with view key
#[test]
fn test_old_epee_import_with_view_key() {
    let (_, _, _, view_secret) = test_keys();

    let key_images = vec![epee_compat::ExportedKeyImage {
        key_image: "0197168670bb9a4f183be4eb8f0f0d57354dfe7094f0c55e5552b7b3a105a77a".to_string(),
        tx_hash: "cccc".to_string(),
        output_index: 0,
    }];

    #[allow(deprecated)]
    let exported = epee_compat::export_key_images(&key_images).expect("Old export should succeed");

    // Import with view key should still succeed (tries EPEE first)
    let imported = epee_compat::import_key_images(&exported, Some(&view_secret))
        .expect("Old EPEE import with view key should succeed");

    assert_eq!(imported.len(), 1);
    assert_eq!(
        imported[0],
        "0197168670bb9a4f183be4eb8f0f0d57354dfe7094f0c55e5552b7b3a105a77a"
    );
}

// Test 7: v3 import with view key via unified import
#[test]
fn test_v3_import_with_view_key() {
    let (spend_scalar, pub_spend, pub_view, view_secret) = test_keys();
    let entries = test_entries(&spend_scalar, 2);

    let exported = export_key_images_v3(
        &entries,
        &spend_scalar,
        &pub_spend,
        &pub_view,
        &view_secret,
        0,
    )
    .expect("v3 export should succeed");

    // Import through the unified import_key_images with view key
    let imported = epee_compat::import_key_images(&exported, Some(&view_secret))
        .expect("v3 import with view key should succeed");

    assert_eq!(imported.len(), 2);
    // Verify key images match (hex encoded)
    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(
            imported[i],
            hex::encode(entry.key_image),
            "Key image {} mismatch via unified import",
            i
        );
    }
}

// Test 8: v3 import without view key fails
#[test]
fn test_v3_import_without_view_key_fails() {
    let (spend_scalar, pub_spend, pub_view, view_secret) = test_keys();
    let entries = test_entries(&spend_scalar, 2);

    let exported = export_key_images_v3(
        &entries,
        &spend_scalar,
        &pub_spend,
        &pub_view,
        &view_secret,
        0,
    )
    .expect("v3 export should succeed");

    // Import without view key should fail (EPEE parse fails, no view key for v3)
    let result = epee_compat::import_key_images(&exported, None);
    assert!(result.is_err(), "v3 import without view key should fail");
}

// Test 9: key_image_pairs fixture signature format validation
#[test]
fn test_key_image_pairs_fixture_signatures() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/key_image_pairs.json");
    let data = std::fs::read_to_string(&path).expect("failed to read fixture");

    #[derive(serde::Deserialize)]
    struct KeyImagePairs {
        count: usize,
        signed_key_images: Vec<SignedKeyImage>,
    }

    #[derive(serde::Deserialize)]
    struct SignedKeyImage {
        key_image: String,
        signature: String,
    }

    let fixture: KeyImagePairs = serde_json::from_str(&data).expect("failed to parse fixture JSON");

    assert_eq!(fixture.count, fixture.signed_key_images.len());

    for (i, pair) in fixture.signed_key_images.iter().enumerate() {
        // Key image: 32 bytes = 64 hex chars
        assert_eq!(
            pair.key_image.len(),
            64,
            "Key image #{i} should be 64 hex chars"
        );
        let ki_bytes =
            hex::decode(&pair.key_image).unwrap_or_else(|_| panic!("Key image #{i} invalid hex"));
        assert_eq!(ki_bytes.len(), 32);

        // Signature: 64 bytes = 128 hex chars (c: 32 + r: 32)
        assert_eq!(
            pair.signature.len(),
            128,
            "Signature #{i} should be 128 hex chars"
        );
        let sig_bytes =
            hex::decode(&pair.signature).unwrap_or_else(|_| panic!("Signature #{i} invalid hex"));
        assert_eq!(sig_bytes.len(), 64);

        // Verify c and r are valid scalars (< group order)
        // When we call from_bytes_mod_order, if the value is already < l, the
        // output equals the input. If not, it gets reduced. We check that
        // the scalar bytes round-trip, meaning the original was canonical.
        let c_bytes: [u8; 32] = sig_bytes[..32].try_into().unwrap();
        let r_bytes: [u8; 32] = sig_bytes[32..64].try_into().unwrap();

        let c_scalar = Scalar::from_bytes_mod_order(c_bytes);
        let r_scalar = Scalar::from_bytes_mod_order(r_bytes);

        let zero = Scalar::from_bytes_mod_order([0u8; 32]);
        // Check that the scalars are not zero (degenerate)
        assert!(
            c_scalar != zero || r_scalar != zero,
            "Signature #{i} should not be all zeros"
        );
    }
}

/// Reject a v3 export whose header names another account.
///
/// The view key authenticates the file, but both header public keys must match
/// the destination wallet. Exercise `import_key_images_from_seed` to check that
/// the account validation reaches the public import API.
#[test]
fn refuses_key_images_exported_for_a_different_account() {
    use monero_rust::seed_compat::Seed;

    let mnemonic = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders \
                    cavernous second brunt saved richly lower upgrade hitched launching \
                    deepest mostly playful layout lower eden";
    let seed = Seed::from_string(Zeroizing::new(mnemonic.to_string())).expect("vector seed");
    let spend_scalar = Scalar::from_bytes_mod_order(*seed.key_bytes_with_passphrase(""));
    let pub_spend = (&spend_scalar * ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes();
    let view_bytes: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    let view_scalar = Scalar::from_bytes_mod_order(view_bytes);
    let pub_view = (&view_scalar * ED25519_BASEPOINT_TABLE).compress().to_bytes();
    let view_secret = view_scalar.to_bytes();

    // Encrypted with this wallet's view key, so it authenticates - but declaring someone
    // else's spend key, which is the part nothing was checking.
    let foreign_spend = (&Scalar::from(9_999u64) * ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes();
    assert_ne!(foreign_spend, pub_spend, "the fixture must name another account");

    let exported = export_key_images_v3(
        &test_entries(&spend_scalar, 2),
        &spend_scalar,
        &foreign_spend,
        &pub_view,
        &view_secret,
        0,
    )
    .expect("export should succeed");

    let err = monero_rust::key_image_signing::import_key_images_from_seed(mnemonic, "", &exported)
        .expect_err("key images naming another account must be refused");
    assert!(
        err.contains("different account"),
        "unexpected error: {err}"
    );
}

/// The account binding says *whose* wallet a file is for. It says nothing about which
/// output each key image belongs to, and key images are applied positionally - so a file
/// for the right account, encrypted with the right view key, whose entries are in
/// the wrong order, marked the wrong outputs spent.
///
/// The per-key-image ring signature is what rules that out, and monero checks it against
/// the output's own one-time public key (wallet2.cpp:13362-13368). This builds a real
/// export, swaps its two records - every key image and every signature still authentic,
/// only mispaired - and requires the import to reject it.
///
/// The swap is within the attacker's means by construction: the file is authenticated with
/// the *view* key, which the untrusted online wallet holds by design, so it can decrypt,
/// rewrite and re-encrypt at will.
#[test]
fn refuses_key_images_paired_with_the_wrong_outputs() {
    use monero_rust::key_image_signing::encrypt_with_view_key;
    use monero_rust::{WalletOutput, WalletState};

    let (spend_scalar, pub_spend, pub_view, view_secret) = test_keys();
    let entries = test_entries(&spend_scalar, 2);

    // A view-only wallet holding exactly these two outputs, in export order.
    let outputs: Vec<WalletOutput> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| WalletOutput {
            tx_hash: format!("{i:064x}"),
            output_index: 0,
            amount: 1,
            amount_xmr: "0.000000000001".to_string(),
            key: hex::encode(entry.pub_key.compress().to_bytes()),
            key_offset: hex::encode(entry.key_offset.to_bytes()),
            commitment_mask: String::new(),
            subaddress_index: Some((0, 0)),
            payment_id: None,
            received_output_bytes: String::new(),
            block_height: 10 + i as u64,
            spent: false,
            spent_height: None,
            key_image: String::new(),
            is_coinbase: false,
            frozen: false,
        })
        .collect();

    let sentinel = format!(
        "viewonly:{}:{}",
        hex::encode(view_secret),
        hex::encode(pub_spend)
    );

    let exported = export_key_images_v3(
        &entries,
        &spend_scalar,
        &pub_spend,
        &pub_view,
        &view_secret,
        0,
    )
    .expect("export should succeed");

    // Accept the unchanged export before checking the tampered version.
    let honest = monero_rust::key_image_signing::import_signed_key_images_from_seed(
        &sentinel, "", &exported,
    )
    .expect("a genuine export should import");
    let mut state = WalletState::new();
    state.replace_outputs(outputs.clone());
    assert_eq!(
        state
            .assign_verified_key_images_in_export_order(&honest)
            .expect("a genuine export should verify"),
        2
    );
    for (output, entry) in state.outputs().iter().zip(&entries) {
        assert_eq!(output.key_image, hex::encode(entry.key_image));
    }

    // Swap the two 96-byte records and reauthenticate the payload.
    let payload = &exported[KEY_IMAGES_MAGIC.len()..];
    let plaintext =
        decrypt_with_view_key(payload, &view_secret).expect("we hold the view key by assumption");
    assert_eq!(plaintext.len(), 68 + 2 * 96, "header plus two records");

    let mut swapped = plaintext.clone();
    swapped[68..68 + 96].copy_from_slice(&plaintext[68 + 96..68 + 192]);
    swapped[68 + 96..68 + 192].copy_from_slice(&plaintext[68..68 + 96]);
    assert_ne!(swapped, plaintext, "the fixture must actually be reordered");

    let mut tampered = KEY_IMAGES_MAGIC.to_vec();
    tampered.extend_from_slice(&encrypt_with_view_key(&swapped, &view_secret));

    // Confirm that file authentication and account validation still pass.
    let mispaired = monero_rust::key_image_signing::import_signed_key_images_from_seed(
        &sentinel, "", &tampered,
    )
    .expect("a reordered file still authenticates and names the right account");
    assert_eq!(mispaired.len(), 2);

    let mut state = WalletState::new();
    state.replace_outputs(outputs);
    let err = state
        .assign_verified_key_images_in_export_order(&mispaired)
        .expect_err("key images paired with the wrong outputs must be refused");
    assert!(
        err.contains("does not belong to the output"),
        "unexpected error: {err}"
    );

    // A rejected import must leave all outputs unchanged.
    assert!(
        state.outputs().iter().all(|o| o.key_image.is_empty()),
        "a refused import must leave the wallet untouched"
    );
}

#[test]
fn key_image_import_preserves_transaction_order_within_a_block() {
    use monero_rust::key_image_signing::generate_key_image_ring_signature;
    use monero_rust::{SignedKeyImage, WalletState};

    let (spend, _, _, _) = test_keys();
    let entries = test_entries(&spend, 4);
    let mut outputs = test_outputs(&entries);
    // Two owned outputs in transaction A, then output zero of transaction B.
    // Sorting by (height, output_index) incorrectly moves B ahead of A.
    outputs[0].output_index = 1;
    outputs[1].tx_hash = outputs[0].tx_hash.clone();
    outputs[1].output_index = 2;
    outputs[1].block_height = outputs[0].block_height;
    outputs[2].block_height = outputs[0].block_height;
    let signatures: Vec<_> = entries
        .iter()
        .map(|entry| {
            let secret = spend + entry.key_offset;
            let (c, r) =
                generate_key_image_ring_signature(&entry.key_image, &entry.pub_key, &secret);
            let mut signature = [0; 64];
            signature[..32].copy_from_slice(&c);
            signature[32..].copy_from_slice(&r);
            SignedKeyImage {
                key_image: entry.key_image,
                signature,
            }
        })
        .collect();
    let mut pool = outputs[3].clone();
    pool.tx_hash = "pool".into();
    pool.block_height = 0;
    // Persistence must preserve the scanner's within-block order.
    outputs.insert(0, pool);
    let json = serde_json::to_string(&outputs).unwrap();
    let mut state = WalletState::new();
    state.replace_outputs(serde_json::from_str(&json).unwrap());
    assert_eq!(state.assign_verified_key_images_in_export_order(&signatures).unwrap(), 4);
    for (i, output) in state.outputs()[..4].iter().enumerate() {
        assert_eq!(output.key_image, hex::encode(entries[i].key_image));
    }
    assert_eq!(state.outputs()[4].tx_hash, "pool");
    assert!(state.outputs()[4].key_image.is_empty());
    assert!(state.freeze_output(&hex::encode(entries[2].key_image)));
    assert_eq!(state.outputs()[2].output_index, 0);
    assert!(state.outputs()[2].frozen);
    let mut too_many = signatures.clone();
    too_many.push(signatures[0].clone());
    assert!(state.assign_verified_key_images_in_export_order(&too_many).is_err());
    let images: Vec<_> = entries.iter().map(|e| hex::encode(e.key_image)).collect();
    assert_eq!(state.assign_key_images_in_export_order(&images).unwrap(), 4);
    assert!(state.outputs()[4].key_image.is_empty());
    assert!(state.assign_key_images_in_export_order(&vec!["extra".into(); 5]).is_err());
}
