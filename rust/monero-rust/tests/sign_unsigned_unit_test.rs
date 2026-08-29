use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, scalar::Scalar};
use monero_oxide::transaction::{NotPruned, Transaction};
/// Unit tests for the offline signing pipeline without RPC or mock-rpc.
use monero_rust::seed_compat::Seed;
use monero_rust::wallet_compat::{
    AddressSpec, Change, Commitment, Decoys, InternalPayment, Protocol, SpendableOutput,
    UnsignedInput, UnsignedTransaction, ViewPair,
};
use monero_wallet::address::Network;
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
    let result = sign_reviewed_seed(TEST_SEED, &unsigned_hex, NETWORK_STR, "")
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
    let deserialized_tx = Transaction::<NotPruned>::read(&mut Cursor::new(&signed_bytes[..]))
        .expect("tx_blob must deserialize to Transaction");

    assert_eq!(
        u64::from(deserialized_tx.version()),
        2,
        "TX version must be 2 (RingCT)"
    );

    assert_eq!(
        deserialized_tx.prefix().inputs.len(),
        1,
        "Must have 1 input"
    );

    assert_eq!(
        deserialized_tx.prefix().outputs.len(),
        2,
        "Must have 2 outputs (payment + change)"
    );

    let Transaction::V2 {
        proofs: Some(proofs),
        ..
    } = &deserialized_tx
    else {
        panic!("signed TX must be a v2 transaction with RCT proofs");
    };

    assert_eq!(proofs.base.fee, fee, "Fee in signed TX RCT base must match");

    // 11. Hash of deserialized transaction must match tx_id
    let tx_hash = hex::encode(deserialized_tx.hash());
    assert_eq!(
        tx_hash, result.tx_id,
        "Hash of deserialized TX must match the returned tx_id"
    );

    // 12. Verify CLSAG signature presence
    assert_eq!(
        proofs.base.commitments.len(),
        2,
        "Must have 2 output commitments"
    );

    // 13. Verify input is ToKey with ring size 16
    let mut expected_spent_key_images = Vec::new();
    for input in &deserialized_tx.prefix().inputs {
        match input {
            monero_oxide::transaction::Input::ToKey {
                key_offsets,
                key_image,
                ..
            } => {
                assert_eq!(
                    key_offsets.len(),
                    16,
                    "Ring size must be 16 for Protocol::v16"
                );
                let ki_bytes = key_image.to_bytes();
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
    for output in &deserialized_tx.prefix().outputs {
        assert_eq!(
            output.amount.unwrap_or(0),
            0,
            "RingCT outputs must have 0 prefix amount"
        );
        assert_ne!(
            output.key.to_bytes(),
            [0u8; 32],
            "Output key must not be zero"
        );
    }
}

/// Build an unsigned transaction owned by `spend_scalar`, including its ring
/// and change. Return the serialized hex and declared fee.
fn synthetic_unsigned_hex_for(spend_scalar: Scalar) -> (String, u64) {
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;
    let view_bytes: [u8; 32] = Keccak256::digest(spend_scalar.to_bytes()).into();
    let view_pair = ViewPair::new(
        spend_point,
        Zeroizing::new(Scalar::from_bytes_mod_order(view_bytes)),
    );

    let key_offset = Scalar::from_bytes_mod_order([0x01; 32]);
    let output_key = &(spend_scalar + key_offset) * ED25519_BASEPOINT_TABLE;
    let mask_scalar = Scalar::from_bytes_mod_order([0x05; 32]);
    let input_amount: u64 = 1_000_000_000_000;
    let commitment_point = Commitment::new(mask_scalar, input_amount).calculate();

    let global_index: u64 = 50000;
    let spendable = build_spendable_output(
        output_key,
        key_offset,
        mask_scalar,
        input_amount,
        [0xAA; 32],
        0,
        global_index,
    );

    let ring_len = 16usize;
    let mut ring: Vec<[curve25519_dalek::edwards::EdwardsPoint; 2]> = Vec::with_capacity(ring_len);
    ring.push([output_key, commitment_point]);
    for i in 1..ring_len {
        ring.push([
            &Scalar::from_bytes_mod_order([i as u8 + 0x10; 32]) * ED25519_BASEPOINT_TABLE,
            &Scalar::from_bytes_mod_order([i as u8 + 0x80; 32]) * ED25519_BASEPOINT_TABLE,
        ]);
    }
    let mut offsets: Vec<u64> = Vec::with_capacity(ring_len);
    offsets.push(global_index);
    for _ in 1..ring_len {
        offsets.push(100);
    }

    let fee: u64 = 100_000_000;
    let payment_amount: u64 = 800_000_000_000;
    let unsigned_tx = UnsignedTransaction {
        protocol: Protocol::v16,
        r_seed: Zeroizing::new([0x42; 32]),
        fee,
        payments: vec![
            InternalPayment::Payment((
                view_pair.address(Network::Stagenet, AddressSpec::Standard),
                payment_amount,
            )),
            InternalPayment::Change(
                Change::new(&view_pair, false),
                input_amount - payment_amount - fee,
            ),
        ],
        data: vec![],
        inputs: vec![UnsignedInput {
            output: spendable,
            decoys: Decoys {
                i: 0,
                offsets,
                ring,
            },
        }],
    };

    (hex::encode(unsigned_tx.serialize()), fee)
}

/// Sign with the same seed offset passphrase used to derive the wallet.
///
/// The passphrase changes the spend key by subtracting `cn_slow_hash(passphrase)`.
/// Omitting it selects a different wallet, which does not own these inputs.
/// Check both signing and preview with the passphrase applied.
#[test]
fn offline_signing_honours_the_seed_offset_passphrase() {
    const PASSPHRASE: &str = "hunter2";

    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string())).unwrap();
    let offset_key = seed.key_bytes_with_passphrase(PASSPHRASE);
    assert_ne!(
        offset_key,
        seed.key_bytes(),
        "premise: a passphrase must change which wallet the mnemonic opens"
    );

    let (unsigned_hex, fee) = synthetic_unsigned_hex_for(Scalar::from_bytes_mod_order(*offset_key));

    // Sign with the keys that own the outputs.
    let signed = sign_reviewed_seed(TEST_SEED, &unsigned_hex, NETWORK_STR, PASSPHRASE)
        .expect("the passphrase wallet must be able to sign its own transaction");
    assert_eq!(signed.fee, fee, "fee must survive signing");
    assert_eq!(signed.tx_id.len(), 64, "tx_id must be 64 hex chars");

    // Without the passphrase, the wallet does not own these outputs.
    let error = sign_reviewed_seed(TEST_SEED, &unsigned_hex, NETWORK_STR, "")
        .expect_err("the bare mnemonic's wallet does not own these outputs");
    assert!(
        error.contains("Refusing to sign") || error.contains("does not belong"),
        "expected a refusal naming the wrong wallet, got: {error}"
    );

    // Preview and signing must agree on the wallet and its change ownership.
    let previewed = monero_rust::native::preview_unsigned_transaction_from_seed(
        TEST_SEED,
        &unsigned_hex,
        NETWORK_STR,
        PASSPHRASE,
    )
    .expect("preview must parse the transaction the passphrase wallet signs");
    let change = previewed.transactions[0]
        .destinations
        .iter()
        .find(|d| d.is_change)
        .expect("preview must surface the change destination");
    assert!(
        change.is_ours,
        "preview disowned the passphrase wallet's own change: {change:?}"
    );

    let error = monero_rust::native::preview_unsigned_transaction_from_seed(
        TEST_SEED,
        &unsigned_hex,
        NETWORK_STR,
        "",
    )
    .unwrap_err();
    assert!(error.contains("does not belong"));
}

/// Reject unsigned transactions that redirect change to another wallet.
///
/// This fixture has owned inputs and balanced amounts, so those checks pass.
/// The signer must separately verify that it owns the declared change address.
#[test]
fn refuses_to_sign_a_transaction_whose_change_is_redirected() {
    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string())).unwrap();
    let entropy = seed.entropy();
    let mut spend_bytes = [0u8; 32];
    spend_bytes.copy_from_slice(&entropy[..]);
    let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
    let spend_point = &spend_scalar * ED25519_BASEPOINT_TABLE;

    let view_bytes: [u8; 32] = Keccak256::digest(spend_bytes).into();
    let view_scalar = Scalar::from_bytes_mod_order(view_bytes);
    let view_pair = ViewPair::new(spend_point, Zeroizing::new(view_scalar));

    // Use an owned input so `sign_offline()` passes its ownership check.
    let key_offset = Scalar::from_bytes_mod_order([0x01; 32]);
    let output_key = &(spend_scalar + key_offset) * ED25519_BASEPOINT_TABLE;
    let mask_scalar = Scalar::from_bytes_mod_order([0x05; 32]);
    let input_amount: u64 = 1_000_000_000_000;
    let commitment = Commitment::new(mask_scalar, input_amount);
    let commitment_point = commitment.calculate();

    let global_index: u64 = 50000;
    let spendable = build_spendable_output(
        output_key,
        key_offset,
        mask_scalar,
        input_amount,
        [0xAA; 32],
        0,
        global_index,
    );

    let ring_len = 16usize;
    let mut ring: Vec<[curve25519_dalek::edwards::EdwardsPoint; 2]> = Vec::with_capacity(ring_len);
    ring.push([output_key, commitment_point]);
    for i in 1..ring_len {
        let decoy_key =
            &Scalar::from_bytes_mod_order([i as u8 + 0x10; 32]) * ED25519_BASEPOINT_TABLE;
        let decoy_commitment =
            &Scalar::from_bytes_mod_order([i as u8 + 0x80; 32]) * ED25519_BASEPOINT_TABLE;
        ring.push([decoy_key, decoy_commitment]);
    }
    let mut offsets: Vec<u64> = Vec::with_capacity(ring_len);
    offsets.push(global_index);
    for _ in 1..ring_len {
        offsets.push(100);
    }
    let decoys = Decoys {
        i: 0,
        offsets,
        ring,
    };

    // Use an unrelated key pair for the attacker's wallet.
    let attacker_spend = Scalar::from_bytes_mod_order([0x77; 32]);
    let attacker_point = &attacker_spend * ED25519_BASEPOINT_TABLE;
    let attacker_view = Scalar::from_bytes_mod_order([0x88; 32]);
    let attacker_pair = ViewPair::new(attacker_point, Zeroizing::new(attacker_view));
    let attacker_address = attacker_pair.address(Network::Stagenet, AddressSpec::Standard);

    let dest_address = view_pair.address(Network::Stagenet, AddressSpec::Standard);
    let fee: u64 = 100_000_000;
    let payment_amount: u64 = 800_000_000_000;
    let change_amount: u64 = input_amount - payment_amount - fee;

    let unsigned_tx = UnsignedTransaction {
        protocol: Protocol::v16,
        r_seed: Zeroizing::new([0x42; 32]),
        fee,
        payments: vec![
            InternalPayment::Payment((dest_address, payment_amount)),
            // Label the attacker's destination as change.
            InternalPayment::Change(Change::from_raw(attacker_address, None), change_amount),
        ],
        data: vec![],
        inputs: vec![UnsignedInput {
            output: spendable,
            decoys,
        }],
    };

    let unsigned_hex = hex::encode(unsigned_tx.serialize());

    // The preview must report that the declared change address is not ours
    // so the user can see the problem before signing.
    let mut spend_bytes_preview = [0u8; 32];
    spend_bytes_preview.copy_from_slice(&entropy[..]);
    let view_bytes_preview: [u8; 32] = Keccak256::digest(spend_bytes_preview).into();
    let preview = monero_rust::native::preview_unsigned_transaction(
        spend_bytes_preview,
        view_bytes_preview,
        &unsigned_hex,
        NETWORK_STR,
    )
    .expect("preview should parse the same transaction the signer refuses");
    let change = preview.transactions[0]
        .destinations
        .iter()
        .find(|d| d.is_change)
        .expect("preview must surface the change destination");
    assert!(
        !change.is_ours,
        "preview claimed the redirected change belongs to this wallet: {change:?}"
    );

    let result = sign_reviewed_seed(TEST_SEED, &unsigned_hex, NETWORK_STR, "");

    let error = match result {
        Err(error) => error,
        Ok(_) => panic!(
            "signed a transaction whose change goes to an address this wallet cannot spend - \
             the offline signer would hand an attacker the wallet's balance"
        ),
    };
    assert!(
        error.contains("Refusing to sign"),
        "expected a refusal naming the redirected change, got: {error}"
    );
}

#[test]
fn change_ownership_requires_the_correct_address_derivation_mode() {
    use monero_wallet::address::{AddressType, MoneroAddress, SubaddressIndex};

    let spend = Scalar::from(2345678u64);
    let view = Scalar::from_bytes_mod_order(Keccak256::digest(spend.to_bytes()).into());
    let pair = ViewPair::new(&spend * ED25519_BASEPOINT_TABLE, Zeroizing::new(view));
    let (original, _) = synthetic_unsigned_hex_for(spend);
    let bytes = hex::decode(original).unwrap();
    for spec in [
        AddressSpec::Standard,
        AddressSpec::Subaddress(SubaddressIndex::new(0, 1).unwrap()),
    ] {
        // A different network prefix does not change ownership.
        let owned = pair.address(Network::Mainnet, spec);
        let wrong_type = if owned.is_subaddress() {
            AddressType::Legacy
        } else {
            AddressType::Subaddress
        };
        let hostile = MoneroAddress::new(Network::Mainnet, wrong_type, owned.spend(), owned.view());
        let mut unsigned = UnsignedTransaction::read(&mut bytes.as_slice()).unwrap();
        let amount = unsigned.payments[1].address_and_amount().1;
        unsigned.payments[1] =
            InternalPayment::Change(Change::from_raw(owned.clone(), None), amount);
        let encoded = hex::encode(unsigned.serialize());
        let preview = monero_rust::native::preview_unsigned_transaction(
            spend.to_bytes(),
            view.to_bytes(),
            &encoded,
            NETWORK_STR,
        )
        .unwrap();
        assert!(preview.transactions[0].destinations[1].is_ours);
        let signed =
            sign_reviewed_keys(spend.to_bytes(), view.to_bytes(), &encoded, NETWORK_STR).unwrap();
        assert_eq!(
            signed.change_outputs.iter().map(|o| o.amount).sum::<u64>(),
            999_900_000_000
        );

        unsigned.payments[1] =
            InternalPayment::Change(Change::from_raw(hostile.clone(), None), amount);
        let encoded = hex::encode(unsigned.serialize());
        let preview = monero_rust::native::preview_unsigned_transaction(
            spend.to_bytes(),
            view.to_bytes(),
            &encoded,
            NETWORK_STR,
        )
        .unwrap();
        assert!(
            !preview.transactions[0].destinations[1].is_ours,
            "relabeling owned keys must not imply ownership"
        );
        let error = sign_reviewed_keys(spend.to_bytes(), view.to_bytes(), &encoded, NETWORK_STR)
            .unwrap_err();
        assert!(error.contains("Refusing to sign"));

        // A valid first change entry must not hide a malformed second entry
        // carrying the same public keys. Keep the transaction balanced.
        unsigned.payments[1] = InternalPayment::Change(Change::from_raw(owned, None), amount / 2);
        unsigned.payments.push(InternalPayment::Change(
            Change::from_raw(hostile, None),
            amount - amount / 2,
        ));
        let error = sign_reviewed_keys(
            spend.to_bytes(),
            view.to_bytes(),
            &hex::encode(unsigned.serialize()),
            NETWORK_STR,
        )
        .unwrap_err();
        assert!(error.contains("Refusing to sign"));
    }
}

#[test]
fn ordinary_change_is_recovered_and_guaranteed_claim_is_rejected() {
    let spend = Scalar::from(2345678u64);
    let view = Scalar::from_bytes_mod_order(Keccak256::digest(spend.to_bytes()).into());
    let ours = ViewPair::new(&spend * ED25519_BASEPOINT_TABLE, Zeroizing::new(view));
    let other = ViewPair::new(
        &Scalar::from(777u64) * ED25519_BASEPOINT_TABLE,
        Zeroizing::new(Scalar::from(888u64)),
    );
    let (encoded, _) = synthetic_unsigned_hex_for(spend);
    let bytes = hex::decode(encoded).unwrap();
    let mut unsigned = UnsignedTransaction::read(&mut bytes.as_slice()).unwrap();
    unsigned.payments[0] = InternalPayment::Payment((
        other.address(Network::Stagenet, AddressSpec::Standard),
        800_000_000_000,
    ));
    for guaranteed in [false, true] {
        unsigned.payments[1] =
            InternalPayment::Change(Change::new(&ours, guaranteed), 199_900_000_000);
        let encoded = hex::encode(unsigned.serialize());
        let preview = monero_rust::native::preview_unsigned_transaction(
            spend.to_bytes(),
            view.to_bytes(),
            &encoded,
            NETWORK_STR,
        )
        .unwrap();
        assert_eq!(preview.transactions[0].destinations[1].is_ours, !guaranteed);
        let signed = sign_reviewed_keys(spend.to_bytes(), view.to_bytes(), &encoded, NETWORK_STR);
        if guaranteed {
            assert!(signed.unwrap_err().contains("Refusing to sign"));
        } else {
            let signed = signed.unwrap();
            assert_eq!(signed.change_outputs.len(), 1);
            assert_eq!(signed.change_outputs[0].amount, 199_900_000_000);
        }
    }
}

// Explicit test policy, independent of amounts supplied by the fixture.
fn sign_reviewed_seed(
    seed: &str,
    encoded: &str,
    network: &str,
    passphrase: &str,
) -> Result<monero_rust::native::OfflineSignResult, String> {
    let preview = monero_rust::native::preview_unsigned_transaction_from_seed(
        seed, encoded, network, passphrase,
    )?;
    let approval = preview.approve(10_000_000_000, 100_000_000_000_000)?;
    monero_rust::native::sign_unsigned_transaction(seed, encoded, network, passphrase, &approval)
}

fn sign_reviewed_keys(
    spend: [u8; 32],
    view: [u8; 32],
    encoded: &str,
    network: &str,
) -> Result<monero_rust::native::OfflineSignResult, String> {
    let preview = monero_rust::native::preview_unsigned_transaction(spend, view, encoded, network)?;
    let approval = preview.approve(10_000_000_000, 100_000_000_000_000)?;
    monero_rust::native::sign_unsigned_transaction_with_private_keys(
        spend, view, encoded, network, &approval,
    )
}

#[test]
fn approval_binds_full_review_and_independent_fee_and_debit_limits() {
    use monero_rust::native::{
        preview_unsigned_transaction, sign_unsigned_transaction_with_private_keys,
    };
    let spend = Scalar::from(123456789u64);
    let view = Scalar::from_bytes_mod_order(Keccak256::digest(spend.to_bytes()).into());
    let other = ViewPair::new(
        &Scalar::from(777u64) * ED25519_BASEPOINT_TABLE,
        Zeroizing::new(Scalar::from(888u64)),
    );
    let (encoded, _) = synthetic_unsigned_hex_for(spend);
    let bytes = hex::decode(encoded).unwrap();
    let mut original = UnsignedTransaction::read(&mut bytes.as_slice()).unwrap();
    original.payments[0] = InternalPayment::Payment((
        other.address(Network::Stagenet, AddressSpec::Standard),
        800_000_000_000,
    ));
    let encoded = hex::encode(original.serialize());
    let preview =
        preview_unsigned_transaction(spend.to_bytes(), view.to_bytes(), &encoded, NETWORK_STR)
            .unwrap();
    assert_eq!(preview.transaction_count, 1);
    assert_eq!(preview.transactions.len(), 1);
    assert_eq!(preview.input_total, 1_000_000_000_000);
    assert_eq!(preview.fee, 100_000_000);
    assert_eq!(preview.wallet_debit, 800_100_000_000);
    assert_eq!(preview.transactions[0].wallet_debit, preview.wallet_debit);
    assert!(preview
        .approve(99_999_999, 900_000_000_000)
        .unwrap_err()
        .contains("fee"));
    assert!(preview
        .approve(100_000_000, 800_099_999_999)
        .unwrap_err()
        .contains("debit"));
    let approval = preview.approve(100_000_000, 800_100_000_000).unwrap();
    let signed = sign_unsigned_transaction_with_private_keys(
        spend.to_bytes(),
        view.to_bytes(),
        &encoded,
        NETWORK_STR,
        &approval,
    )
    .unwrap();
    assert_eq!(signed.fee, preview.fee);

    // Identical destinations can conceal an extra owned input burned entirely as fee.
    let mut high = original.clone();
    let mut input = high.inputs[0].clone();
    let offset = Scalar::from(131313u64);
    input.output.output.data.key_offset = offset;
    input.output.output.data.key = &(spend + offset) * ED25519_BASEPOINT_TABLE;
    input.output.output.data.commitment =
        Commitment::new(Scalar::from(515151u64), 10_000_000_000_000);
    input.output.output.absolute.tx = [0xBB; 32];
    input.output.global_index = 60000;
    input.decoys.offsets[0] = 60000;
    input.decoys.ring[0] = [input.output.key(), input.output.commitment().calculate()];
    high.fee += input.output.commitment().amount;
    high.inputs.push(input);
    let high_encoded = hex::encode(high.serialize());
    let high_preview = preview_unsigned_transaction(
        spend.to_bytes(),
        view.to_bytes(),
        &high_encoded,
        NETWORK_STR,
    )
    .unwrap();
    assert_eq!(
        preview.transactions[0].destinations,
        high_preview.transactions[0].destinations
    );
    assert_eq!(high_preview.fee, 10_000_100_000_000);
    assert_eq!(high_preview.input_total, 11_000_000_000_000);
    assert_eq!(high_preview.wallet_debit, 10_800_100_000_000);
    assert!(high_preview.approve(100_000_000, 800_100_000_000).is_err());

    let mut recipient = original.clone();
    recipient.payments[0] = InternalPayment::Payment((
        other.address(
            Network::Stagenet,
            AddressSpec::Subaddress(monero_wallet::address::SubaddressIndex::new(0, 1).unwrap()),
        ),
        800_000_000_000,
    ));
    let mut seed = original.clone();
    seed.r_seed[0] ^= 1;
    let mut ring = original.clone();
    ring.inputs[0].decoys.offsets[1] += 1;
    let mut fee = original.clone();
    fee.fee += 1;
    let change = fee.payments[1].address_and_amount().1;
    if let InternalPayment::Change(_, amount) = &mut fee.payments[1] {
        *amount = change - 1;
    }
    for modified in [high, recipient, seed, ring, fee] {
        let error = sign_unsigned_transaction_with_private_keys(
            spend.to_bytes(),
            view.to_bytes(),
            &hex::encode(modified.serialize()),
            NETWORK_STR,
            &approval,
        )
        .unwrap_err();
        assert!(
            error.contains("differs from the approved review"),
            "{error}"
        );
    }
    assert!(sign_unsigned_transaction_with_private_keys(
        spend.to_bytes(),
        view.to_bytes(),
        &encoded,
        "mainnet",
        &approval
    )
    .is_err());
    // A caller editing displayed totals cannot approve a misleading review.
    let mut misleading = preview.clone();
    misleading.fee = 0;
    let misleading = misleading.approve(0, 800_100_000_000).unwrap();
    assert!(sign_unsigned_transaction_with_private_keys(
        spend.to_bytes(),
        view.to_bytes(),
        &encoded,
        NETWORK_STR,
        &misleading
    )
    .is_err());
    // Trailing bytes are never silently signed as a shorter reviewed transaction.
    assert!(preview_unsigned_transaction(
        spend.to_bytes(),
        view.to_bytes(),
        &(encoded + "00"),
        NETWORK_STR
    )
    .is_err());
}
