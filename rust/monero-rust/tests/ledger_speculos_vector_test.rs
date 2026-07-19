use std::path::Path;

use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE, edwards::CompressedEdwardsY, scalar::Scalar,
};
use monero_rust::monero_backend::{
    ringct::generate_key_image,
    wallet::address::{MoneroAddress, Network},
};
use serde::Deserialize;
use serde_json::Value;
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct LedgerSpeculosVector {
    schema: String,
    app: LedgerApp,
    vectors: Vec<ApduVector>,
    decoded: Decoded,
}

#[derive(Deserialize)]
struct LedgerApp {
    name: String,
    version: String,
    model: String,
}

#[derive(Deserialize)]
struct ApduVector {
    name: String,
    status: String,
    data: String,
    response_apdu: String,
}

#[derive(Deserialize)]
struct Decoded {
    public_view_key: String,
    public_spend_key: String,
    address: String,
    key_image: String,
    view_tag: String,
    key_image_private_key: String,
    key_image_public_key: String,
}

fn vector_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/ledger_app_speculos/monero_nanos2_crypto.json")
}

fn tx_flow_run_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/ledger_app_speculos/tx_flow_runs/20260614T175711Z-nanosp")
        .join(name)
}

fn decode_32(hex_value: &str) -> [u8; 32] {
    hex::decode(hex_value)
        .expect("valid hex")
        .try_into()
        .expect("32 byte value")
}

#[test]
fn ledger_app_speculos_crypto_vector_matches_local_primitives() {
    let fixture = std::fs::read_to_string(vector_path()).expect("fixture should be readable");
    let vector: LedgerSpeculosVector =
        serde_json::from_str(&fixture).expect("fixture should parse");

    assert_eq!(vector.schema, "monero-rust ledger app speculos vector v1");
    assert_eq!(vector.app.name, "Monero");
    assert_eq!(vector.app.version, "2.2.0");
    assert_eq!(vector.app.model, "nanosp");

    for apdu in &vector.vectors {
        assert_eq!(apdu.status, "9000", "{} should be approved", apdu.name);
        assert!(
            apdu.response_apdu.ends_with(&apdu.status),
            "{} response APDU should end with status",
            apdu.name
        );
        assert!(
            apdu.response_apdu.starts_with(&apdu.data),
            "{} response APDU should start with data",
            apdu.name
        );
    }

    let address = MoneroAddress::from_str(Network::Stagenet, &vector.decoded.address)
        .expect("Ledger address should decode as stagenet");
    assert_eq!(
        address.view().compress().to_bytes(),
        decode_32(&vector.decoded.public_view_key)
    );
    assert_eq!(
        address.spend().compress().to_bytes(),
        decode_32(&vector.decoded.public_spend_key)
    );

    let mut private_key = decode_32(&vector.decoded.key_image_private_key);
    for byte in &mut private_key {
        *byte ^= 0x55;
    }
    let scalar = Scalar::from_bytes_mod_order(private_key);
    let key_image = generate_key_image(&Zeroizing::new(scalar))
        .compress()
        .to_bytes();
    assert_eq!(key_image, decode_32(&vector.decoded.key_image));

    let supplied_public_key = CompressedEdwardsY(decode_32(&vector.decoded.key_image_public_key))
        .decompress()
        .expect("Ledger test public key should decompress");
    assert_eq!(
        (&scalar * ED25519_BASEPOINT_TABLE).compress().to_bytes(),
        supplied_public_key.compress().to_bytes()
    );
    assert_eq!(
        supplied_public_key.compress().to_bytes(),
        decode_32(&vector.decoded.key_image_public_key)
    );
    assert_eq!(vector.decoded.view_tag, "76");
}

#[test]
fn ledger_app_speculos_tx_flow_transcript_is_pinned() {
    let manifest_data =
        std::fs::read_to_string(tx_flow_run_path("manifest.json")).expect("manifest should exist");
    let manifest: Value = serde_json::from_str(&manifest_data).expect("manifest should parse");

    assert_eq!(
        manifest["schema"],
        "monero-rust ledger app speculos tx-flow run v1"
    );
    assert_eq!(
        manifest["app_monero_commit"],
        "8c5352aec6e2e16532c06c58a3d13413bec9bc6a"
    );
    assert_eq!(manifest["device"], "nanosp");
    assert_eq!(manifest["returncode"], 0);
    assert_eq!(
        manifest["app_elf_sha256"],
        "97f6e012d39a8edcd6c4d11baf0c4b30360b10c34281d17d9339ed4c851ce8a6"
    );

    let stdout = std::fs::read_to_string(tx_flow_run_path("pytest.stdout.log"))
        .expect("stdout should exist");
    assert!(stdout.contains("47 passed"), "missing pytest pass summary");
    assert!(
        stdout.contains("Firmware correctly rejected amount=0 with error 0x6913"),
        "missing amount-zero security assertion"
    );

    let stderr = std::fs::read_to_string(tx_flow_run_path("pytest.stderr.log"))
        .expect("stderr should exist");
    assert!(
        stderr.contains("ragger.apdu_logger"),
        "missing APDU logger output"
    );
    assert!(
        stderr.contains("037c0201"),
        "missing validation APDU transcript"
    );
    assert!(stderr.contains("<= 6913"), "missing rejection status word");
}
