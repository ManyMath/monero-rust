//! The offline signer against hostile wallet2 unsigned txsets.

mod cold_signing_common;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use cold_signing_common::*;
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE,
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
};
use monero_oxide::transaction::{NotPruned, Transaction};
use monero_rust::epee_compat::{ParsedUnsignedTxSetSummary, TxDestinationEntrySummary};
use monero_rust::native::{
    preview_unsigned_transaction, sign_unsigned_transaction_with_private_keys, OfflineSignResult,
    TransactionPreview,
};
use monero_wallet::address::MoneroAddress;
use sha3::{Digest, Keccak256};

const FOREIGN_STANDARD: &str = "44AFFq5kSiGBoZ4NMDwYtN18obc8AemS33DBLWs3H7otXft3XjrpDtQGv7SqSsaBYBb98uNbr2VBBEt7f2wfn3RVGQBEP3A";
const OWN_STANDARD: &str = "42ey1afDFnn4886T7196doS9GPMzexD9gXpsZJDwVjeRVdFCSoHnv7KPbBeGpzJBzHRCAs9UxqeoyFQMYbqSWYTfJJQAWDm";

fn point_bytes(p: monero_wallet::ed25519::Point) -> [u8; 32] {
    let p: EdwardsPoint = p.into();
    p.compress().to_bytes()
}

fn destination(address: &str, amount: u64, with_original: bool) -> TxDestinationEntrySummary {
    let parsed = MoneroAddress::from_str_with_unchecked_network(address).expect("address");
    let original = if with_original {
        address.as_bytes().to_vec()
    } else {
        Vec::new()
    };
    TxDestinationEntrySummary {
        original_len: original.len(),
        original,
        amount,
        spend_public_key: point_bytes(parsed.spend()),
        view_public_key: point_bytes(parsed.view()),
        is_subaddress: parsed.is_subaddress(),
        is_integrated: parsed.payment_id().is_some(),
    }
}

fn sealed_hex(set: &ParsedUnsignedTxSetSummary) -> String {
    hex::encode(seal_unsigned_txset(set, &cold_keys().view_secret_key))
}

fn preview(set: &ParsedUnsignedTxSetSummary) -> Result<TransactionPreview, String> {
    let cold = cold_keys();
    preview_unsigned_transaction(
        cold.spend_secret_key,
        cold.view_secret_key,
        &sealed_hex(set),
        "mainnet",
    )
}

fn sign(set: &ParsedUnsignedTxSetSummary) -> Result<OfflineSignResult, String> {
    let cold = cold_keys();
    let encoded = sealed_hex(set);
    let reviewed = preview_unsigned_transaction(
        cold.spend_secret_key,
        cold.view_secret_key,
        &encoded,
        "mainnet",
    )?;
    let approval = reviewed.approve(u64::MAX, u64::MAX)?;
    sign_unsigned_transaction_with_private_keys(
        cold.spend_secret_key,
        cold.view_secret_key,
        &encoded,
        "mainnet",
        &approval,
    )
}

fn keccak(data: &[u8]) -> [u8; 32] {
    Keccak256::digest(data).into()
}

fn amounts_received_by(
    tx: &Transaction<NotPruned>,
    view_secret: &Scalar,
    spend_public: &EdwardsPoint,
) -> Vec<u64> {
    let extra =
        monero_wallet::extra::Extra::read(&mut tx.prefix().extra.as_slice()).expect("extra");
    let (keys, _additional) = extra.keys().expect("tx public key");
    let tx_public: EdwardsPoint = keys[0].into();
    let derivation = (view_secret * tx_public)
        .mul_by_cofactor()
        .compress()
        .to_bytes();
    let Transaction::V2 {
        proofs: Some(proofs),
        ..
    } = tx
    else {
        panic!("not RingCT")
    };
    let mut found = Vec::new();
    for (index, output) in tx.prefix().outputs.iter().enumerate() {
        let mut material = derivation.to_vec();
        varint(index as u64, &mut material);
        let shared = Scalar::from_bytes_mod_order(keccak(&material));
        let expected = (&shared * ED25519_BASEPOINT_TABLE) + spend_public;
        if expected.compress().to_bytes() != output.key.to_bytes() {
            continue;
        }
        let monero_oxide::ringct::EncryptedAmount::Compact { amount } =
            proofs.base.encrypted_amounts[index]
        else {
            panic!("expected compact amounts")
        };
        let mut mask_material = b"amount".to_vec();
        mask_material.extend_from_slice(&shared.to_bytes());
        let mask = keccak(&mask_material);
        let mut value = [0u8; 8];
        for i in 0..8 {
            value[i] = amount[i] ^ mask[i];
        }
        found.push(u64::from_le_bytes(value));
    }
    found
}

#[test]
fn reserialized_vector_is_byte_identical_to_wallet2s_archive() {
    let hot = hot_keys();
    let original = open(
        monero_rust::epee_compat::UNSIGNED_TX_MAGIC,
        &vector_unsigned_txset(),
        &hot.view_secret_key,
    );
    assert_eq!(unsigned_txset_archive(&parse_vector_unsigned()), original);
    let reviewed = preview(&parse_vector_unsigned()).expect("re-sealed vector previews");
    assert_eq!(reviewed.fee, 2_599_200_000);
}

#[test]
fn redirected_change_is_shown_and_refused() {
    let mut set = parse_vector_unsigned();
    let tx = &mut set.txes[0];
    let stolen = destination(FOREIGN_STANDARD, tx.change_amount, false);
    tx.change = stolen.clone();
    tx.split_destinations[0] = stolen;

    let reviewed = preview(&set).expect("preview shows the redirected change");
    let change = reviewed.transactions[0]
        .destinations
        .iter()
        .find(|d| d.is_change)
        .expect("change row");
    assert!(!change.is_ours);
    assert_eq!(change.address, FOREIGN_STANDARD);
    assert_eq!(
        reviewed.wallet_debit,
        reviewed.input_total - 1_000_000_000_000
    );
    let err = sign(&set).expect_err("signing must refuse redirected change");
    assert!(err.contains("change"), "{err}");
}

#[test]
fn inflated_fee_is_shown_and_bounded_by_the_approval() {
    let mut set = parse_vector_unsigned();
    let tx = &mut set.txes[0];
    let burned = 5_000_000_000_000;
    tx.change.amount -= burned;
    tx.change_amount -= burned;
    tx.split_destinations[0].amount -= burned;

    let reviewed = preview(&set).expect("preview");
    assert_eq!(reviewed.fee, 2_599_200_000 + burned);
    assert!(reviewed.approve(3_000_000_000, u64::MAX).is_err());
}

#[test]
fn real_output_index_lie_is_refused() {
    let mut set = parse_vector_unsigned();
    let source = &mut set.txes[0].sources[0];
    let real = source.real_output as usize;
    let foreign = (real + 1) % source.ring.len();
    source.ring[foreign].commitment = source.ring[real].commitment;
    source.real_output = foreign as u64;
    let err = preview(&set).expect_err("a foreign ring member must not be signed for");
    assert!(err.contains("does not belong"), "{err}");
}

#[test]
fn source_amount_lie_is_refused_before_a_signature_exists() {
    let mut set = parse_vector_unsigned();
    let tx = &mut set.txes[0];
    tx.sources[0].amount += 1_000_000_000_000;
    tx.change.amount += 1_000_000_000_000;
    tx.change_amount += 1_000_000_000_000;
    tx.split_destinations[0].amount += 1_000_000_000_000;
    let err = sign(&set).expect_err("an input amount lie must not sign");
    assert!(
        err.contains("Failed to sign") || err.contains("commitment"),
        "{err}"
    );
}

#[test]
fn own_keys_under_another_address_kind_are_not_ours() {
    let mut set = parse_vector_unsigned();
    let tx = &mut set.txes[0];
    let fee = 2_599_200_000u64;
    let total: u64 = tx.sources.iter().map(|source| source.amount).sum();
    let mut relabelled = destination(OWN_STANDARD, 1_000_000_000_000, false);
    relabelled.is_subaddress = true;
    let foreign = destination(FOREIGN_STANDARD, total - 1_000_000_000_000 - fee, true);
    tx.destinations = vec![relabelled.clone(), foreign.clone()];
    tx.destination_total_amount = total - fee;
    tx.split_destinations = vec![relabelled, foreign];
    tx.change.amount = 0;
    tx.change_amount = 0;

    let reviewed = preview(&set).expect("preview");
    let payment = &reviewed.transactions[0].destinations[0];
    assert!(!payment.is_ours, "{payment:?}");
    assert_eq!(reviewed.wallet_debit, reviewed.input_total);
    assert_eq!(reviewed.fee, fee);
}

#[test]
fn every_transaction_is_reviewed_and_signed_amounts_match_the_review() {
    let recipient_spend = Scalar::from_bytes_mod_order(keccak(b"test recipient spend"));
    let recipient_view = Scalar::from_bytes_mod_order(keccak(b"test recipient view"));
    let recipient_spend_public = &recipient_spend * ED25519_BASEPOINT_TABLE;
    let recipient = MoneroAddress::new(
        monero_wallet::address::Network::Mainnet,
        monero_wallet::address::AddressType::Legacy,
        monero_wallet::ed25519::Point::from(recipient_spend_public),
        monero_wallet::ed25519::Point::from(&recipient_view * ED25519_BASEPOINT_TABLE),
    )
    .to_string();

    let base = parse_vector_unsigned();
    let template = base.txes[0].clone();
    let own_change = template.change.clone();
    let fee = 1_000_000_000u64;
    let mut txes = Vec::new();
    for (index, pay) in [(0usize, 3_000_000_000_000u64), (1, 7_000_000_000_000)] {
        let mut tx = template.clone();
        tx.sources = vec![template.sources[index].clone()];
        tx.selected_transfer_indices = vec![template.selected_transfer_indices[index]];
        let change_amount = tx.sources[0].amount - pay - fee;
        let payment = destination(&recipient, pay, true);
        let mut change = own_change.clone();
        change.amount = change_amount;
        tx.change = change.clone();
        tx.change_amount = change_amount;
        tx.destinations = vec![payment.clone()];
        tx.destination_total_amount = pay;
        tx.split_destinations = vec![payment, change];
        txes.push(tx);
    }
    let set = ParsedUnsignedTxSetSummary { txes, ..base };

    let reviewed = preview(&set).expect("preview");
    assert_eq!(reviewed.transaction_count, 2);
    assert_eq!(reviewed.fee, 2 * fee);
    assert_eq!(
        reviewed.wallet_debit,
        3_000_000_000_000 + 7_000_000_000_000 + 2 * fee
    );
    for summary in &reviewed.transactions {
        assert_eq!(summary.destinations[0].address, recipient);
        assert!(!summary.destinations[0].is_ours);
    }

    let signed = sign(&set).expect("sign");
    assert_eq!(signed.transactions.len(), 2);
    for (signed_tx, pay) in signed
        .transactions
        .iter()
        .zip([3_000_000_000_000u64, 7_000_000_000_000])
    {
        let bytes = hex::decode(&signed_tx.tx_blob).unwrap();
        let tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(tx.prefix().outputs.len(), 2);
        assert_eq!(
            tx.prefix().additional_timelock,
            monero_oxide::transaction::Timelock::None
        );
        assert_eq!(
            amounts_received_by(&tx, &recipient_view, &recipient_spend_public),
            vec![pay],
            "the recipient decodes exactly the reviewed amount"
        );
        assert_eq!(signed_tx.fee, fee);
        let back: Vec<u64> = signed_tx.change_outputs.iter().map(|o| o.amount).collect();
        assert_eq!(back.len(), 1, "one output returns to the wallet");
    }
}

#[test]
#[ignore = "known defect: the signer ignores a nonzero unlock_time that wallet2 refuses"]
fn nonzero_unlock_time_is_refused_like_wallet2() {
    let mut set = parse_vector_unsigned();
    set.txes[0].unlock_time = 1_000_000;
    let outcome = preview(&set);
    assert!(
        outcome.is_err(),
        "wallet2 rejects unlock_time != 0; previewed as {:?}",
        outcome.map(|p| p.transactions)
    );
}

#[test]
#[ignore = "known defect: the signer pays a destination string without comparing it with the destination keys"]
fn destination_original_must_agree_with_destination_keys() {
    let mut set = parse_vector_unsigned();
    let tx = &mut set.txes[0];
    tx.destinations[0].original = FOREIGN_STANDARD.as_bytes().to_vec();
    tx.destinations[0].original_len = FOREIGN_STANDARD.len();
    tx.split_destinations[1] = tx.destinations[0].clone();
    let outcome = preview(&set);
    assert!(
        outcome.is_err(),
        "wallet2 rejects an original/keys mismatch; previewed as {:?}",
        outcome.map(|p| p.transactions)
    );
}

#[test]
#[ignore = "known defect: the signer never compares the split destinations with the destinations"]
fn split_destinations_must_agree_with_destinations() {
    let mut set = parse_vector_unsigned();
    let tx = &mut set.txes[0];
    tx.split_destinations[1] = destination(FOREIGN_STANDARD, 1_000_000_000_000, true);
    let outcome = preview(&set);
    assert!(
        outcome.is_err(),
        "wallet2 rejects dests != splitted_dsts; previewed as {:?}",
        outcome.map(|p| p.transactions)
    );
}

#[test]
#[ignore = "known defect: the signer signs a set that spends one output twice"]
fn one_output_spent_by_two_transactions_of_a_set_is_refused() {
    let mut set = parse_vector_unsigned();
    let duplicate = set.txes[0].clone();
    set.txes.push(duplicate);
    let outcome = preview(&set);
    assert!(
        outcome.is_err(),
        "wallet2 rejects duplicate inputs across a set; previewed {} transactions",
        outcome.map(|p| p.transaction_count).unwrap_or(0)
    );
}

#[test]
#[ignore = "known defect: the signer ignores the construction flags of a wallet2 txset"]
fn construction_without_view_tags_is_refused_like_wallet2() {
    let mut set = parse_vector_unsigned();
    set.txes[0].construction_flags = 0x01;
    assert!(
        preview(&set).is_err(),
        "wallet2 rejects use_view_tags == false"
    );
}

#[test]
#[ignore = "known defect: previewing a wallet2 txset does work quadratic in its size"]
fn preview_work_is_bounded_by_txset_size() {
    let mut set = parse_vector_unsigned();
    let tx = &mut set.txes[0];
    let decoy_key = CompressedEdwardsY(tx.sources[1].real_out_tx_key);
    assert!(decoy_key.decompress().is_some());
    let real_key = tx.sources[0].real_out_tx_key;
    let source = &mut tx.sources[0];
    source.real_out_tx_key = decoy_key.to_bytes();
    source.real_out_additional_tx_keys = vec![decoy_key.to_bytes(); 2_000];
    source.real_out_additional_tx_keys.push(real_key);
    tx.subaddr_indices = (1..=3_000u32).collect();

    let encoded = sealed_hex(&set);
    assert!(
        encoded.len() / 2 < 100 * 1024,
        "{} bytes",
        encoded.len() / 2
    );

    let cold = cold_keys();
    let (done, finished) = mpsc::channel();
    let started = Instant::now();
    std::thread::spawn(move || {
        let outcome = preview_unsigned_transaction(
            cold.spend_secret_key,
            cold.view_secret_key,
            &encoded,
            "mainnet",
        );
        let _ = done.send(outcome.map(|p| p.fee));
    });
    match finished.recv_timeout(Duration::from_secs(20)) {
        Ok(outcome) => {
            eprintln!("preview finished in {:?}: {outcome:?}", started.elapsed());
            let err = outcome.expect_err("3,000 subaddress indices for two inputs");
            assert!(
                err.contains("more subaddress indices (3000) than inputs (2)"),
                "{err}"
            );
        }
        Err(_) => panic!("previewing a 71 KiB txset did not finish within 20 s"),
    }
}
