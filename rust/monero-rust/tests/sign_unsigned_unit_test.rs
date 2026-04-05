use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_rust::monero_backend::transaction::Transaction;
/// Unit tests for the offline signing pipeline without RPC or mock-rpc.
use monero_rust::monero_backend::wallet::{
    address::{AddressSpec, Network},
    seed::Seed,
    Change, Decoys, InternalPayment, SpendableOutput, UnsignedInput, UnsignedTransaction, ViewPair,
};
use monero_rust::monero_backend::{Commitment, Protocol};
use sha3::{Digest, Keccak256};
use std::io::Cursor;
use zeroize::Zeroizing;

const TEST_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const NETWORK_STR: &str = "stagenet";

/// Constructs a `SpendableOutput` by serializing raw components into the wire format.
fn build_spendable_output(
    output_key: curve25519_dalek::edwards::EdwardsPoint,
    key_offset: Scalar,
    mask: Scalar,
    amount: u64,
    tx_hash: [u8; 32],
    output_index: u8,
    global_index: u64,
) -> SpendableOutput {
    let mut buf: Vec<u8> = Vec::new();

    // AbsoluteId: tx (32 bytes) + o (1 byte)
    buf.extend_from_slice(&tx_hash);
    buf.push(output_index);

    // OutputData: key (compressed 32 bytes) + key_offset (32 bytes) + mask (32 bytes) + amount (u64 LE)
    buf.extend_from_slice(&output_key.compress().to_bytes());
    buf.extend_from_slice(&key_offset.to_bytes());
    buf.extend_from_slice(&mask.to_bytes());
    buf.extend_from_slice(&amount.to_le_bytes());

    // Metadata: subaddress flag (0 = None) + payment_id (8 bytes) + arbitrary_data count (u32 LE = 0)
    buf.push(0u8); // no subaddress
    buf.extend_from_slice(&[0u8; 8]); // zero payment_id
    buf.extend_from_slice(&0u32.to_le_bytes()); // no arbitrary data

    // SpendableOutput: global_index (u64 LE)
    buf.extend_from_slice(&global_index.to_le_bytes());

    SpendableOutput::read(&mut Cursor::new(&buf))
        .expect("Failed to deserialize synthetic SpendableOutput")
}

#[test]
fn test_sign_unsigned_transaction_synthetic() {
    // 1. Derive spend key and view pair from the test seed, same as tx_builder.rs
    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string())).unwrap();
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;

    let view_bytes: [u8; 32] = Keccak256::digest(spend_bytes).into();
    let view_scalar = Scalar::from_bytes_mod_order(view_bytes);
    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));

    // 2. Build OutputData with mathematically consistent key
    // key = (spend_scalar + key_offset) * G  -- this is checked by sign_offline
    let key_offset = Scalar::from_bytes_mod_order([0x01; 32]);
    let output_key = &(spend_scalar + key_offset) * ED25519_BASEPOINT_TABLE;

    let mask_scalar = Scalar::from_bytes_mod_order([0x05; 32]);
    let input_amount: u64 = 1_000_000_000_000; // 1 XMR
    let commitment = Commitment::new(mask_scalar, input_amount);
    let commitment_point = commitment.calculate();

    // 3. Build SpendableOutput via serialization roundtrip
    let global_index: u64 = 50000;
    let spendable = build_spendable_output(
        output_key,
        key_offset,
        mask_scalar,
        input_amount,
        [0xAA; 32], // tx hash
        0,          // output index
        global_index,
    );

    // 4. Build Decoys with 16 ring members (Protocol::v16 ring size)
    // Real spend is at index 0
    let ring_len = 16usize;
    let real_index: u8 = 0;

    let mut ring: Vec<[curve25519_dalek::edwards::EdwardsPoint; 2]> = Vec::with_capacity(ring_len);

    // Position 0 = real spend: [output_key, commitment_point]
    ring.push([output_key, commitment_point]);

    // Positions 1..16 = deterministic decoys
    for i in 1..ring_len {
        let decoy_key =
            &Scalar::from_bytes_mod_order([i as u8 + 0x10; 32]) * ED25519_BASEPOINT_TABLE;
        let decoy_commitment =
            &Scalar::from_bytes_mod_order([i as u8 + 0x80; 32]) * ED25519_BASEPOINT_TABLE;
        ring.push([decoy_key, decoy_commitment]);
    }

    // Build offsets: first entry is absolute global index of first ring member,
    // subsequent entries are relative differences.
    let mut absolute_indices: Vec<u64> = Vec::with_capacity(ring_len);
    absolute_indices.push(global_index); // real output at position 0
    for i in 1..ring_len {
        absolute_indices.push(global_index + (i as u64) * 100);
    }
    // Convert to offset format
    let mut offsets: Vec<u64> = Vec::with_capacity(ring_len);
    offsets.push(absolute_indices[0]);
    for i in 1..ring_len {
        offsets.push(absolute_indices[i] - absolute_indices[i - 1]);
    }

    let decoys = Decoys {
        i: real_index,
        offsets,
        ring,
    };

    // 5. Build destination address (send to self on stagenet)
    let dest_address = view_pair.address(Network::Stagenet, AddressSpec::Standard);

    // 6. Build Change
    let change = Change::new(&view_pair, false);

    // 7. Build UnsignedTransaction
    let fee: u64 = 100_000_000; // 0.0001 XMR
    let payment_amount: u64 = 800_000_000_000; // 0.8 XMR
    let change_amount: u64 = input_amount - payment_amount - fee; // 199_900_000_000

    let unsigned_tx = UnsignedTransaction {
        protocol: Protocol::v16,
        r_seed: Zeroizing::new([0x42; 32]),
        fee,
        payments: vec![
            InternalPayment::Payment((dest_address, payment_amount)),
            InternalPayment::Change(change, change_amount),
        ],
        data: vec![],
        inputs: vec![UnsignedInput {
            output: spendable,
            decoys,
        }],
    };

    // 8. Serialize and sign
    let unsigned_bytes = unsigned_tx.serialize();
    assert!(
        !unsigned_bytes.is_empty(),
        "Serialized unsigned TX must not be empty"
    );

    let unsigned_hex = hex::encode(&unsigned_bytes);
    let result =
        monero_rust::native::sign_unsigned_transaction(TEST_SEED, &unsigned_hex, NETWORK_STR)
            .expect("sign_unsigned_transaction should succeed with synthetic input");

    // 9. Assert results on OfflineSignResult
    assert_eq!(result.tx_id.len(), 64, "tx_id must be 64 hex chars");
    assert!(
        hex::decode(&result.tx_id).is_ok(),
        "tx_id must be valid hex"
    );

    assert_eq!(
        result.fee, fee,
        "fee must match the unsigned transaction fee"
    );

    assert!(
        result.tx_blob.len() > 100,
        "Signed blob hex must be substantial (got {} chars)",
        result.tx_blob.len()
    );
    assert!(
        hex::decode(&result.tx_blob).is_ok(),
        "tx_blob must be valid hex"
    );

    assert_eq!(result.tx_key.len(), 64, "tx_key must be 64 hex chars");
    assert!(
        hex::decode(&result.tx_key).is_ok(),
        "tx_key must be valid hex"
    );

    // 10. Deserialize the signed blob and verify structure
    let signed_bytes = hex::decode(&result.tx_blob).unwrap();
    let deserialized_tx = Transaction::read(&mut Cursor::new(&signed_bytes[..]))
        .expect("tx_blob must deserialize to Transaction");

    assert_eq!(
        deserialized_tx.prefix.version, 2,
        "TX version must be 2 (RingCT)"
    );

    assert_eq!(deserialized_tx.prefix.inputs.len(), 1, "Must have 1 input");

    assert_eq!(
        deserialized_tx.prefix.outputs.len(),
        2,
        "Must have 2 outputs (payment + change)"
    );

    assert_eq!(
        deserialized_tx.rct_signatures.base.fee, fee,
        "Fee in signed TX RCT base must match"
    );

    // 11. Hash of deserialized transaction must match tx_id
    let tx_hash = hex::encode(deserialized_tx.hash());
    assert_eq!(
        tx_hash, result.tx_id,
        "Hash of deserialized TX must match the returned tx_id"
    );

    // 12. Verify CLSAG signature presence
    assert_eq!(
        deserialized_tx.rct_signatures.base.commitments.len(),
        2,
        "Must have 2 output commitments"
    );

    // 13. Verify input is ToKey with ring size 16
    let mut expected_spent_key_images = Vec::new();
    for input in &deserialized_tx.prefix.inputs {
        match input {
            monero_rust::monero_backend::transaction::Input::ToKey {
                key_offsets,
                key_image,
                ..
            } => {
                assert_eq!(
                    key_offsets.len(),
                    16,
                    "Ring size must be 16 for Protocol::v16"
                );
                let ki_bytes = key_image.compress().to_bytes();
                assert_ne!(ki_bytes, [0u8; 32], "Key image must not be zero");
                expected_spent_key_images.push(hex::encode(ki_bytes));
            }
            _ => panic!("Expected ToKey input"),
        }
    }
    assert_eq!(
        result.spent_key_images, expected_spent_key_images,
        "Offline signing result must expose spent input key images"
    );

    // 14. Verify outputs are confidential (amount == 0 in prefix for RingCT)
    for output in &deserialized_tx.prefix.outputs {
        assert_eq!(output.amount, 0, "RingCT outputs must have 0 prefix amount");
        assert_ne!(
            output.key.to_bytes(),
            [0u8; 32],
            "Output key must not be zero"
        );
    }
}
