use monero_rust::tx_proof::{generate_out_proof_v2, verify_out_proof_v2};
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

/// OutProofV2 generate-then-verify roundtrip.
#[test]
fn test_out_proof_v2_generate_verify_roundtrip() {
    // Test parameters (stagenet)
    let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
    let tx_key = "0200000000000000000000000000000000000000000000000000000000000000";
    let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";
    let message = "cross-validation test";

    // Generate proof
    let proof = generate_out_proof_v2(tx_id, tx_key, address, message, "stagenet")
        .expect("generate_out_proof_v2 must succeed");

    // Verify proof structure
    assert!(
        proof.signature.starts_with("OutProofV2"),
        "Signature must have OutProofV2 prefix"
    );
    assert!(
        proof.formatted.contains("BEGIN OUTPROOF"),
        "Formatted output must contain BEGIN OUTPROOF"
    );
    assert!(
        proof.formatted.contains(message),
        "Formatted output must contain the message"
    );

    // Compute R = tx_key * G for verification
    let tx_key_bytes = hex::decode(tx_key).unwrap();
    let mut r_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&tx_key_bytes);
    let r = curve25519_dalek::scalar::Scalar::from_bytes_mod_order(r_bytes);
    let r_point = &r * curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    let r_pub_hex = hex::encode(r_point.compress().to_bytes());

    // Verify the generated proof
    let verified = verify_out_proof_v2(
        tx_id,
        address,
        message,
        &proof.signature,
        "stagenet",
        &r_pub_hex,
    )
    .expect("verify_out_proof_v2 must not error");
    assert!(verified, "Generated proof must verify successfully");
}

/// Wrong message fails verification.
#[test]
fn test_out_proof_v2_message_mismatch_fails() {
    let tx_id = "46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806";
    let tx_key = "0300000000000000000000000000000000000000000000000000000000000000";
    let address = "55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt";

    let proof = generate_out_proof_v2(tx_id, tx_key, address, "correct message", "stagenet")
        .expect("generate must succeed");

    let tx_key_bytes = hex::decode(tx_key).unwrap();
    let mut r_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&tx_key_bytes);
    let r = curve25519_dalek::scalar::Scalar::from_bytes_mod_order(r_bytes);
    let r_point = &r * curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
    let r_pub_hex = hex::encode(r_point.compress().to_bytes());

    // Verify with wrong message should fail
    let verified = verify_out_proof_v2(
        tx_id,
        address,
        "wrong message",
        &proof.signature,
        "stagenet",
        &r_pub_hex,
    )
    .expect("verify must not error");
    assert!(!verified, "Proof with wrong message must verify as false");
}

/// Verifies InProofV2 fixture matches the same `prefix + base58(96 bytes)` format.
#[test]
fn test_fixture_cross_validates_proof_format() {
    let fixture = load_fixture();

    // Fixture signature is InProofV2 (confirmed by research)
    assert!(
        fixture.signature.starts_with("InProofV2"),
        "Fixture should be InProofV2, got prefix: {}",
        &fixture.signature[..10.min(fixture.signature.len())]
    );

    // Structure matches OutProofV2: prefix + base58(96 bytes = D + c + s)
    let payload_str = fixture.signature.strip_prefix("InProofV2").unwrap();
    let decoded = base58_monero::decode(payload_str).expect("base58 decode must succeed");
    assert_eq!(
        decoded.len(),
        96,
        "V2 proof payload must be 96 bytes (D + c + s)"
    );

    // D is a valid compressed Edwards point (first 32 bytes)
    let d_bytes: [u8; 32] = decoded[0..32].try_into().unwrap();
    let d_point = curve25519_dalek::edwards::CompressedEdwardsY(d_bytes).decompress();
    assert!(d_point.is_some(), "D must be a valid curve point");

    // The fixture was verified by monero-wallet-cli (check.good == true)
    assert!(
        fixture.check.good,
        "monero-wallet-cli verified this proof as good"
    );
    assert!(
        fixture.check.received > 0,
        "Received amount must be non-zero"
    );
}
