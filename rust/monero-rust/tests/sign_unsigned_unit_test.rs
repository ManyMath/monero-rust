//! Unit tests for the offline signing pipeline without RPC or mock-rpc.

mod common;

use common::offline::{
    default_input, encode, owned_input, pinned_fee, ring_offsets, sign_reviewed_keys,
    sign_reviewed_seed, unsigned_for, Wallet, CHANGE_AMOUNT, FEE, INPUT_AMOUNT, PAYMENT_AMOUNT,
};
use curve25519_dalek::scalar::Scalar;
use monero_oxide::transaction::{NotPruned, Transaction};
use monero_rust::seed_compat::Seed;
use monero_rust::unsigned_tx::{UnsignedChange, UnsignedTransaction};
use monero_wallet::address::{AddressType, MoneroAddress, Network, SubaddressIndex};
use std::io::Cursor;
use zeroize::Zeroizing;

const TEST_SEED: &str = "honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime";
const NETWORK_STR: &str = "stagenet";

fn seed_wallet() -> Wallet {
    let seed = Seed::from_string(Zeroizing::new(TEST_SEED.to_string())).unwrap();
    Wallet::from_spend(Scalar::from_bytes_mod_order(*seed.entropy()))
}

fn other_wallet() -> Wallet {
    Wallet {
        spend: Scalar::from(777u64),
        view: Scalar::from(888u64),
    }
}

fn synthetic_unsigned_for(owner: &Wallet) -> UnsignedTransaction {
    unsigned_for(
        owner,
        Network::Stagenet,
        vec![(owner.address(Network::Stagenet), PAYMENT_AMOUNT)],
    )
}

#[test]
fn test_sign_unsigned_transaction_synthetic() {
    let wallet = seed_wallet();
    let unsigned_tx = synthetic_unsigned_for(&wallet);
    assert_eq!(CHANGE_AMOUNT, 199_900_000_000);

    let unsigned_bytes = unsigned_tx.serialize();
    assert!(
        !unsigned_bytes.is_empty(),
        "Serialized unsigned TX must not be empty"
    );

    let unsigned_hex = hex::encode(&unsigned_bytes);
    let result = sign_reviewed_seed(TEST_SEED, &unsigned_hex, NETWORK_STR, "")
        .expect("sign_unsigned_transaction should succeed with synthetic input");

    assert_eq!(result.tx_id.len(), 64, "tx_id must be 64 hex chars");
    assert!(
        hex::decode(&result.tx_id).is_ok(),
        "tx_id must be valid hex"
    );

    assert_eq!(
        result.fee, FEE,
        "fee must match the fee the unsigned transaction resolves to"
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

    let mut received: Vec<u64> = result.change_outputs.iter().map(|o| o.amount).collect();
    received.sort();
    assert_eq!(received, vec![CHANGE_AMOUNT, PAYMENT_AMOUNT]);

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

    assert_eq!(proofs.base.fee, FEE, "Fee in signed TX RCT base must match");

    let tx_hash = hex::encode(deserialized_tx.hash());
    assert_eq!(
        tx_hash, result.tx_id,
        "Hash of deserialized TX must match the returned tx_id"
    );

    assert_eq!(
        proofs.base.commitments.len(),
        2,
        "Must have 2 output commitments"
    );

    let mut expected_spent_key_images = Vec::new();
    for input in &deserialized_tx.prefix().inputs {
        match input {
            monero_oxide::transaction::Input::ToKey {
                key_offsets,
                key_image,
                ..
            } => {
                assert_eq!(key_offsets.len(), 16, "Ring size must be 16");
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

    let wallet = Wallet::from_spend(Scalar::from_bytes_mod_order(*offset_key));
    let unsigned_hex = encode(&synthetic_unsigned_for(&wallet));

    // Sign with the keys that own the outputs.
    let signed = sign_reviewed_seed(TEST_SEED, &unsigned_hex, NETWORK_STR, PASSPHRASE)
        .expect("the passphrase wallet must be able to sign its own transaction");
    assert_eq!(signed.fee, FEE, "fee must survive signing");
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
    let wallet = seed_wallet();

    let mut unsigned_tx = synthetic_unsigned_for(&wallet);
    unsigned_tx.change = Some(UnsignedChange::Own {
        address: other_wallet().address(Network::Stagenet),
        subaddress: None,
    });
    let unsigned_hex = encode(&unsigned_tx);

    // The preview must report that the declared change address is not ours
    // so the user can see the problem before signing.
    let preview = monero_rust::native::preview_unsigned_transaction(
        wallet.spend.to_bytes(),
        wallet.view.to_bytes(),
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
    assert_eq!(change.amount, CHANGE_AMOUNT);
    assert_eq!(preview.wallet_debit, CHANGE_AMOUNT + FEE);

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
    let wallet = Wallet::from_spend(Scalar::from(2345678u64));
    let preview_of = |encoded: &str| {
        monero_rust::native::preview_unsigned_transaction(
            wallet.spend.to_bytes(),
            wallet.view.to_bytes(),
            encoded,
            NETWORK_STR,
        )
        .unwrap()
    };

    for subaddress in [None, SubaddressIndex::new(0, 1)] {
        // A different network prefix does not change ownership.
        let owned = match subaddress {
            None => wallet.address(Network::Mainnet),
            Some(index) => wallet.subaddress(Network::Mainnet, index.account(), index.address()),
        };
        let wrong_type = if owned.is_subaddress() {
            AddressType::Legacy
        } else {
            AddressType::Subaddress
        };
        let hostile = MoneroAddress::new(Network::Mainnet, wrong_type, owned.spend(), owned.view());

        let mut unsigned = synthetic_unsigned_for(&wallet);
        unsigned.change = Some(UnsignedChange::Own {
            address: owned,
            subaddress,
        });
        let encoded = encode(&unsigned);
        assert!(preview_of(&encoded).transactions[0].destinations[1].is_ours);
        let signed = sign_reviewed_keys(&wallet, &encoded, NETWORK_STR).unwrap();
        assert_eq!(
            signed.change_outputs.iter().map(|o| o.amount).sum::<u64>(),
            999_900_000_000
        );

        unsigned.change = Some(UnsignedChange::Own {
            address: hostile,
            subaddress,
        });
        let encoded = encode(&unsigned);
        assert!(
            !preview_of(&encoded).transactions[0].destinations[1].is_ours,
            "relabeling owned keys must not imply ownership"
        );
        let error = sign_reviewed_keys(&wallet, &encoded, NETWORK_STR).unwrap_err();
        assert!(error.contains("Refusing to sign"));

        unsigned.change = Some(UnsignedChange::Own {
            address: owned,
            subaddress: SubaddressIndex::new(0, 2),
        });
        let encoded = encode(&unsigned);
        assert!(!preview_of(&encoded).transactions[0].destinations[1].is_ours);
        let error = sign_reviewed_keys(&wallet, &encoded, NETWORK_STR).unwrap_err();
        assert!(error.contains("Refusing to sign"));
    }
}

#[test]
fn ordinary_change_is_recovered_and_guaranteed_claim_is_rejected() {
    let wallet = Wallet::from_spend(Scalar::from(2345678u64));
    let mut unsigned = unsigned_for(
        &wallet,
        Network::Stagenet,
        vec![(other_wallet().address(Network::Stagenet), PAYMENT_AMOUNT)],
    );
    for guaranteed in [false, true] {
        if guaranteed {
            let ours = wallet.address(Network::Stagenet);
            unsigned.change = Some(UnsignedChange::Own {
                address: MoneroAddress::new(
                    Network::Stagenet,
                    AddressType::Featured {
                        subaddress: false,
                        payment_id: None,
                        guaranteed: true,
                    },
                    ours.spend(),
                    ours.view(),
                ),
                subaddress: None,
            });
        }
        let encoded = encode(&unsigned);
        let preview = monero_rust::native::preview_unsigned_transaction(
            wallet.spend.to_bytes(),
            wallet.view.to_bytes(),
            &encoded,
            NETWORK_STR,
        )
        .unwrap();
        assert_eq!(preview.transactions[0].destinations[1].is_ours, !guaranteed);
        let signed = sign_reviewed_keys(&wallet, &encoded, NETWORK_STR);
        if guaranteed {
            assert!(signed.unwrap_err().contains("Refusing to sign"));
        } else {
            let signed = signed.unwrap();
            assert_eq!(signed.change_outputs.len(), 1);
            assert_eq!(signed.change_outputs[0].amount, 199_900_000_000);
        }
    }
}

#[test]
fn refuses_an_unsigned_transaction_in_the_old_format() {
    let wallet = Wallet::from_spend(Scalar::from(2345678u64));
    let mut old = vec![0u8, 16];
    old.extend_from_slice(&[0x42; 300]);
    let error = monero_rust::native::preview_unsigned_transaction(
        wallet.spend.to_bytes(),
        wallet.view.to_bytes(),
        &hex::encode(old),
        NETWORK_STR,
    )
    .unwrap_err();
    assert!(error.contains("older version"), "{error}");
}

#[test]
fn dummy_output_is_reviewed_as_nobody_and_nonzero_value_is_refused() {
    use monero_rust::native::{
        preview_unsigned_transaction, sign_unsigned_transaction_with_private_keys,
    };

    let wallet = Wallet::from_spend(Scalar::from(123456789u64));
    let recipient = other_wallet();
    let (spend, view) = (wallet.spend.to_bytes(), wallet.view.to_bytes());
    let mut unsigned = unsigned_for(
        &wallet,
        Network::Stagenet,
        vec![(recipient.address(Network::Stagenet), INPUT_AMOUNT - FEE)],
    );
    unsigned.change = Some(UnsignedChange::Dummy);

    let encoded = encode(&unsigned);
    let preview = preview_unsigned_transaction(spend, view, &encoded, NETWORK_STR).unwrap();
    let transaction = &preview.transactions[0];
    assert_eq!(transaction.destinations.len(), 2);
    assert_eq!(transaction.destinations[0].amount, INPUT_AMOUNT - FEE);
    assert!(!transaction.destinations[0].is_dummy);
    assert_eq!(transaction.destinations[1].address, "");
    assert_eq!(transaction.destinations[1].amount, 0);
    assert!(!transaction.destinations[1].is_change);
    assert!(!transaction.destinations[1].is_ours);
    assert!(transaction.destinations[1].is_dummy);
    assert_eq!(preview.input_total, INPUT_AMOUNT);
    assert_eq!(preview.fee, FEE);
    assert_eq!(preview.wallet_debit, INPUT_AMOUNT);

    let approval = preview.approve(FEE, INPUT_AMOUNT).unwrap();
    sign_unsigned_transaction_with_private_keys(spend, view, &encoded, NETWORK_STR, &approval)
        .expect("a zero-amount dummy should sign");

    unsigned.payments[0].1 -= 1;
    let hostile_encoded = encode(&unsigned);
    let hostile = preview_unsigned_transaction(spend, view, &hostile_encoded, NETWORK_STR).unwrap();
    assert_eq!(hostile.transactions[0].destinations[1].amount, 1);
    assert!(hostile.transactions[0].destinations[1].is_dummy);
    let hostile_approval = hostile.approve(FEE, INPUT_AMOUNT).unwrap();
    let error = sign_unsigned_transaction_with_private_keys(
        spend,
        view,
        &hostile_encoded,
        NETWORK_STR,
        &hostile_approval,
    )
    .unwrap_err();
    assert!(error.contains("dummy output"), "{error}");
    assert!(error.contains("must carry nothing"), "{error}");
}

#[test]
fn approval_binds_full_review_and_independent_fee_and_debit_limits() {
    use monero_rust::native::{
        preview_unsigned_transaction, sign_unsigned_transaction_with_private_keys,
    };
    let wallet = Wallet::from_spend(Scalar::from(123456789u64));
    let (spend, view) = (wallet.spend.to_bytes(), wallet.view.to_bytes());
    let other = other_wallet();
    let original = unsigned_for(
        &wallet,
        Network::Stagenet,
        vec![(other.address(Network::Stagenet), PAYMENT_AMOUNT)],
    );
    let encoded = encode(&original);
    let preview = preview_unsigned_transaction(spend, view, &encoded, NETWORK_STR).unwrap();
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
    let signed =
        sign_unsigned_transaction_with_private_keys(spend, view, &encoded, NETWORK_STR, &approval)
            .unwrap();
    assert_eq!(signed.fee, preview.fee);

    let extra_input = || {
        owned_input(
            &wallet,
            Scalar::from(131313u64),
            Scalar::from(515151u64),
            10_000_000_000_000,
            ring_offsets(60_000),
        )
    };

    let mut high = original.clone();
    high.inputs.push(extra_input());
    let high_preview =
        preview_unsigned_transaction(spend, view, &encode(&high), NETWORK_STR).unwrap();
    assert_eq!(high_preview.fee, 100_000_000);
    assert_eq!(high_preview.input_total, 11_000_000_000_000);
    assert_eq!(high_preview.wallet_debit, 800_100_000_000);
    assert_eq!(
        high_preview.transactions[0].destinations[1].amount,
        CHANGE_AMOUNT + 10_000_000_000_000
    );

    let mut no_change = original.clone();
    no_change.change = None;
    no_change
        .payments
        .push((wallet.address(Network::Stagenet), CHANGE_AMOUNT));
    let no_change_preview =
        preview_unsigned_transaction(spend, view, &encode(&no_change), NETWORK_STR).unwrap();
    assert_eq!(no_change_preview.fee, 100_000_000);
    assert_eq!(no_change_preview.wallet_debit, 800_100_000_000);
    let mut burned = no_change.clone();
    burned.inputs.push(extra_input());
    let burned_preview =
        preview_unsigned_transaction(spend, view, &encode(&burned), NETWORK_STR).unwrap();
    assert_eq!(
        no_change_preview.transactions[0].destinations,
        burned_preview.transactions[0].destinations
    );
    assert_eq!(burned_preview.fee, 10_000_100_000_000);
    assert_eq!(burned_preview.input_total, 11_000_000_000_000);
    assert_eq!(burned_preview.wallet_debit, 10_800_100_000_000);
    assert!(burned_preview
        .approve(100_000_000, 800_100_000_000)
        .is_err());

    let mut recipient = original.clone();
    recipient.payments[0] = (other.subaddress(Network::Stagenet, 0, 1), PAYMENT_AMOUNT);
    let mut seed = original.clone();
    seed.seed[0] ^= 1;
    let mut ring = original.clone();
    let mut offsets = ring_offsets(50_000);
    offsets[1] += 1;
    ring.inputs[0] = owned_input(
        &wallet,
        Scalar::from_bytes_mod_order([0x01; 32]),
        Scalar::from_bytes_mod_order([0x05; 32]),
        INPUT_AMOUNT,
        offsets,
    );
    assert_ne!(ring.inputs[0], default_input(&wallet));
    let mut fee = original.clone();
    fee.fee_rate = pinned_fee(FEE + 1);
    let fee_preview =
        preview_unsigned_transaction(spend, view, &encode(&fee), NETWORK_STR).unwrap();
    assert_eq!(fee_preview.fee, FEE + 1);
    assert_eq!(
        fee_preview.transactions[0].destinations[1].amount,
        CHANGE_AMOUNT - 1
    );
    for modified in [high, no_change, burned, recipient, seed, ring, fee] {
        let error = sign_unsigned_transaction_with_private_keys(
            spend,
            view,
            &encode(&modified),
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
        spend, view, &encoded, "mainnet", &approval
    )
    .is_err());
    // A caller editing displayed totals cannot approve a misleading review.
    let mut misleading = preview.clone();
    misleading.fee = 0;
    let misleading = misleading.approve(0, 800_100_000_000).unwrap();
    assert!(sign_unsigned_transaction_with_private_keys(
        spend,
        view,
        &encoded,
        NETWORK_STR,
        &misleading
    )
    .is_err());
    // Trailing bytes are never silently signed as a shorter reviewed transaction.
    assert!(preview_unsigned_transaction(spend, view, &(encoded + "00"), NETWORK_STR).is_err());
}
