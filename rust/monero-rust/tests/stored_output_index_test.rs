use monero_rust::{native::StoredOutputData, WalletOutput};

const AUTHORITATIVE_INDEX: u64 = 424_242;

fn scanned_output(global_index: Option<u64>) -> WalletOutput {
    WalletOutput {
        tx_hash: "11".repeat(32),
        output_index: 1,
        amount: 1_234,
        key: "22".repeat(32),
        key_offset: "33".repeat(32),
        commitment_mask: "44".repeat(32),
        subaddress_index: Some((0, 0)),
        payment_id: None,
        received_output_bytes: "55".repeat(32),
        global_index,
        block_height: 100,
        spent: false,
        spent_height: None,
        key_image: "66".repeat(32),
        is_coinbase: false,
        frozen: false,
    }
}

#[test]
fn refresh_index_survives_the_spend_metadata_roundtrip() {
    let stored: StoredOutputData = (&scanned_output(Some(AUTHORITATIVE_INDEX))).into();
    let json = serde_json::to_string(&stored).expect("serialize stored output");
    let restored: StoredOutputData = serde_json::from_str(&json).expect("restore stored output");

    assert_eq!(restored.global_index, Some(AUTHORITATIVE_INDEX));
}

#[test]
fn a_legacy_placeholder_remains_explicitly_missing() {
    let stored: StoredOutputData = (&scanned_output(None)).into();
    assert_eq!(stored.global_index, None);
}
