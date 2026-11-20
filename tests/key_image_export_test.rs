use serde::Deserialize;

#[derive(Deserialize)]
struct KeyImagePairs {
    count: usize,
    network: String,
    note: String,
    view_key: String,
    wallet_address: String,
    signed_key_images: Vec<SignedKeyImage>,
}

#[derive(Deserialize)]
struct SignedKeyImage {
    key_image: String,
    signature: String,
}

fn load_fixture() -> KeyImagePairs {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/key_image_pairs.json");
    let data = std::fs::read_to_string(&path).expect("failed to read fixture");
    serde_json::from_str(&data).expect("failed to parse fixture JSON")
}

#[test]
fn test_count_matches_entries() {
    let fixture = load_fixture();
    assert_eq!(
        fixture.count,
        fixture.signed_key_images.len(),
        "count field should match actual number of entries"
    );
    assert!(fixture.count > 0, "Should have at least one key image pair");
}

#[test]
fn test_fixture_metadata_pins_v3_key_image_export_context() {
    let fixture = load_fixture();

    assert_eq!(fixture.count, 125);
    assert_eq!(fixture.network, "stagenet");
    assert_eq!(
        fixture.note,
        "File format magic: 'Monero key image export\\x03'"
    );
    assert_eq!(fixture.view_key.len(), 64);
    assert_eq!(
        fixture.wallet_address,
        "58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf"
    );
    assert!(
        hex::decode(&fixture.view_key).is_ok(),
        "view key should be hex"
    );
}

#[test]
fn test_key_image_format() {
    let fixture = load_fixture();
    for (i, pair) in fixture.signed_key_images.iter().enumerate() {
        // Key image should be 32 bytes = 64 hex chars
        assert_eq!(
            pair.key_image.len(),
            64,
            "Key image #{i} should be 64 hex chars (32 bytes), got {}",
            pair.key_image.len()
        );
        // Should be valid hex
        let decoded = hex::decode(&pair.key_image);
        assert!(
            decoded.is_ok(),
            "Key image #{i} should be valid hex: {}",
            pair.key_image
        );
        assert_eq!(decoded.unwrap().len(), 32);
    }
}

#[test]
fn test_signature_format() {
    let fixture = load_fixture();
    for (i, pair) in fixture.signed_key_images.iter().enumerate() {
        // Signature should be 64 bytes = 128 hex chars
        assert_eq!(
            pair.signature.len(),
            128,
            "Signature #{i} should be 128 hex chars (64 bytes), got {}",
            pair.signature.len()
        );
        // Should be valid hex
        let decoded = hex::decode(&pair.signature);
        assert!(
            decoded.is_ok(),
            "Signature #{i} should be valid hex: {}",
            pair.signature
        );
        assert_eq!(decoded.unwrap().len(), 64);
    }
}

#[test]
fn test_hex_round_trip() {
    let fixture = load_fixture();
    for (i, pair) in fixture.signed_key_images.iter().enumerate() {
        // Decode then re-encode key image
        let ki_bytes =
            hex::decode(&pair.key_image).unwrap_or_else(|_| panic!("Key image #{i} decode failed"));
        let ki_reencoded = hex::encode(&ki_bytes);
        assert_eq!(
            ki_reencoded, pair.key_image,
            "Key image #{i} hex round-trip mismatch"
        );

        // Decode then re-encode signature
        let sig_bytes =
            hex::decode(&pair.signature).unwrap_or_else(|_| panic!("Signature #{i} decode failed"));
        let sig_reencoded = hex::encode(&sig_bytes);
        assert_eq!(
            sig_reencoded, pair.signature,
            "Signature #{i} hex round-trip mismatch"
        );
    }
}

#[test]
fn test_key_images_are_unique() {
    let fixture = load_fixture();
    let mut seen = std::collections::HashSet::new();
    for pair in &fixture.signed_key_images {
        assert!(
            seen.insert(&pair.key_image),
            "Duplicate key image found: {}",
            pair.key_image
        );
    }
}
