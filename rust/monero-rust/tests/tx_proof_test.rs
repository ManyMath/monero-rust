use serde::Deserialize;

#[derive(Deserialize)]
struct CheckResult {
    confirmations: u64,
    good: bool,
    in_pool: bool,
    received: u64,
}

#[derive(Deserialize)]
struct ProofFixture {
    address: String,
    check: CheckResult,
    message: String,
    signature: String,
    txid: String,
}

fn load_fixture() -> ProofFixture {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/proof_pair_0.json");
    let data = std::fs::read_to_string(&path).expect("failed to read fixture");
    serde_json::from_str(&data).expect("failed to parse fixture JSON")
}

#[test]
fn test_proof_signature_has_valid_prefix() {
    let fixture = load_fixture();

    // The signature should start with a recognized proof prefix
    let valid_prefixes = ["OutProofV2", "InProofV2", "OutProofV1", "InProofV1"];
    let has_valid_prefix = valid_prefixes
        .iter()
        .any(|prefix| fixture.signature.starts_with(prefix));

    assert!(
        has_valid_prefix,
        "Proof signature should start with a recognized prefix, got: {}",
        &fixture.signature[..20.min(fixture.signature.len())]
    );
}

#[test]
fn test_proof_signature_base58_payload() {
    let fixture = load_fixture();

    // Strip the prefix to get the base58 payload
    let prefixes = ["OutProofV2", "InProofV2", "OutProofV1", "InProofV1"];
    let payload_str = prefixes
        .iter()
        .find_map(|prefix| fixture.signature.strip_prefix(prefix))
        .expect("should have a recognized prefix");

    // The base58 payload should decode successfully
    let decoded = base58_monero::decode(payload_str);
    assert!(
        decoded.is_ok(),
        "Proof base58 payload should decode successfully"
    );

    let data = decoded.unwrap();
    // V2 proofs contain D (32 bytes) + c (32 bytes) + s (32 bytes) = 96 bytes
    assert_eq!(
        data.len(),
        96,
        "V2 proof payload should be 96 bytes (D + c + s), got {}",
        data.len()
    );
}

#[test]
fn test_proof_txid_format() {
    let fixture = load_fixture();

    // Txid should be 64 hex chars (32 bytes)
    assert_eq!(fixture.txid.len(), 64, "Txid should be 64 hex chars");
    assert!(
        hex::decode(&fixture.txid).is_ok(),
        "Txid should be valid hex"
    );
}

#[test]
fn test_proof_address_format() {
    let fixture = load_fixture();

    // Address should be a valid Monero base58 address
    // Stagenet addresses typically start with '5'
    assert!(
        fixture.address.starts_with('5'),
        "Stagenet address should start with '5'"
    );
    assert!(
        fixture.address.len() >= 95 && fixture.address.len() <= 106,
        "Address length should be in valid Monero range (95-106), got {}",
        fixture.address.len()
    );
}

#[test]
fn test_proof_check_result() {
    let fixture = load_fixture();

    // The daemon verified this proof as good
    assert!(fixture.check.good, "check_tx_proof reported good: true");
    assert!(!fixture.check.in_pool, "TX should not be in the mempool");
    assert!(
        fixture.check.confirmations > 0,
        "TX should have confirmations"
    );
    assert!(
        fixture.check.received > 0,
        "Received amount should be non-zero"
    );
}

#[test]
fn test_proof_message_is_present() {
    let fixture = load_fixture();

    // The fixture was captured with a specific message
    assert!(
        !fixture.message.is_empty(),
        "Message should be present in the test fixture"
    );
}

#[test]
fn test_proof_d_point_is_valid_curve_point() {
    let fixture = load_fixture();

    // Extract D from the signature payload
    let prefixes = ["OutProofV2", "InProofV2", "OutProofV1", "InProofV1"];
    let payload_str = prefixes
        .iter()
        .find_map(|prefix| fixture.signature.strip_prefix(prefix))
        .expect("should have a recognized prefix");

    let data = base58_monero::decode(payload_str).expect("base58 decode");

    // First 32 bytes are D (an Edwards point)
    let mut d_bytes = [0u8; 32];
    d_bytes.copy_from_slice(&data[0..32]);

    let d_point = curve25519_dalek::edwards::CompressedEdwardsY(d_bytes).decompress();
    assert!(
        d_point.is_some(),
        "D in the proof should be a valid ed25519 curve point"
    );
}
