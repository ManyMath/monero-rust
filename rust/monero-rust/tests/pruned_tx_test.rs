use monero_rust::monero_backend::transaction::{Input, Transaction};
use sha3::{Digest, Keccak256};

#[derive(serde::Deserialize)]
struct PrunedTxFixture {
    as_hex_full: String,
    pruned_as_hex: String,
    prunable_hash: String,
    txid: String,
}

fn load_fixture() -> PrunedTxFixture {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/pruned_tx_fixture_0.json");
    let data = std::fs::read_to_string(&path).expect("failed to read fixture");
    serde_json::from_str(&data).expect("failed to parse fixture JSON")
}

#[test]
fn test_full_tx_parses_successfully() {
    let fixture = load_fixture();
    let bytes = hex::decode(&fixture.as_hex_full).expect("failed to decode as_hex_full");
    let tx = Transaction::read::<&[u8]>(&mut bytes.as_ref())
        .expect("Transaction::read() should succeed on full unpruned TX");

    // Verify the transaction hash matches the expected txid
    let computed_hash = hex::encode(tx.hash());
    assert_eq!(computed_hash, fixture.txid, "TX hash mismatch");
}

#[test]
fn test_key_images_extractable_from_pruned() {
    let fixture = load_fixture();
    let pruned_bytes = hex::decode(&fixture.pruned_as_hex).expect("failed to decode pruned_as_hex");

    // Parse just the prefix + RctBase from pruned data
    let mut cursor = std::io::Cursor::new(&pruned_bytes);
    let prefix = monero_rust::monero_backend::transaction::TransactionPrefix::read(&mut cursor)
        .expect("prefix should parse from pruned data");

    // Extract key images from the parsed prefix
    let key_images: Vec<String> = prefix
        .inputs
        .iter()
        .filter_map(|input| match input {
            Input::ToKey { key_image, .. } => Some(hex::encode(key_image.compress().to_bytes())),
            _ => None,
        })
        .collect();

    assert!(
        !key_images.is_empty(),
        "Should extract at least one key image"
    );

    // Verify they are 64 hex chars (32 bytes)
    for ki in &key_images {
        assert_eq!(ki.len(), 64, "Key image should be 32 bytes (64 hex chars)");
    }

    // Cross-check: parse full TX and verify same key images
    let full_bytes = hex::decode(&fixture.as_hex_full).expect("decode full");
    let full_tx = Transaction::read::<&[u8]>(&mut full_bytes.as_ref()).expect("parse full");

    let full_key_images: Vec<String> = full_tx
        .prefix
        .inputs
        .iter()
        .filter_map(|input| match input {
            Input::ToKey { key_image, .. } => Some(hex::encode(key_image.compress().to_bytes())),
            _ => None,
        })
        .collect();

    assert_eq!(
        key_images, full_key_images,
        "Key images from pruned and full TX should match"
    );
}

#[test]
fn test_prunable_hash_matches_keccak256() {
    let fixture = load_fixture();

    // Parse the full transaction to extract the prunable portion
    let full_bytes = hex::decode(&fixture.as_hex_full).expect("decode full");
    let tx = Transaction::read::<&[u8]>(&mut full_bytes.as_ref()).expect("parse full tx");

    // Serialize the prunable portion and compute its Keccak-256 hash
    let prunable_bytes = tx.rct_signatures.prunable.serialize();
    assert!(
        !prunable_bytes.is_empty(),
        "Prunable section should not be empty for a full transaction"
    );

    let computed_hash: [u8; 32] = Keccak256::digest(&prunable_bytes).into();
    let computed_hex = hex::encode(computed_hash);

    assert_eq!(
        computed_hex, fixture.prunable_hash,
        "Keccak-256 of prunable section should match the fixture's prunable_hash"
    );
}

#[test]
fn test_pruned_is_prefix_of_full() {
    let fixture = load_fixture();
    let full_bytes = hex::decode(&fixture.as_hex_full).expect("decode full");
    let pruned_bytes = hex::decode(&fixture.pruned_as_hex).expect("decode pruned");

    // The pruned hex should be a prefix of the full hex (prefix + RctBase come first)
    assert!(
        full_bytes.starts_with(&pruned_bytes),
        "Pruned TX bytes should be a prefix of the full TX bytes"
    );
    assert!(
        pruned_bytes.len() < full_bytes.len(),
        "Pruned should be shorter than full"
    );
}
