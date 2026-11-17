//! The online wallet against an offline signer that signs something else.

mod cold_signing_common;

use cold_signing_common::*;
use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
use curve25519_dalek::scalar::Scalar;
use monero_oxide::transaction::{NotPruned, Transaction};
use monero_rust::monero_rpc::{Rpc, RpcConnection, RpcError};
use monero_rust::native::{
    broadcast_offline_signed_transaction_with_rpc,
    create_unsigned_transaction_from_wallet2_txset_keys, preview_unsigned_transaction,
    sign_unsigned_transaction_with_private_keys, OfflineSignResult, UnsignedTransactionResult,
};
use monero_rust::signed_tx_checks::check_offline_signed_transaction;
use monero_rust::unsigned_tx::UnsignedTransaction;
use monero_rust::WalletState;
use monero_wallet::address::{AddressType, MoneroAddress, Network, SubaddressIndex};
use monero_wallet::interface::FeeRate;
use monero_wallet::ViewPair;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

const FOREIGN_STANDARD: &str = "44AFFq5kSiGBoZ4NMDwYtN18obc8AemS33DBLWs3H7otXft3XjrpDtQGv7SqSsaBYBb98uNbr2VBBEt7f2wfn3RVGQBEP3A";

fn request() -> String {
    let cold = cold_keys();
    create_unsigned_transaction_from_wallet2_txset_keys(
        cold.spend_secret_key,
        cold.view_secret_key,
        &vector_unsigned_txset(),
        "mainnet",
    )
    .expect("the vector converts")
    .unsigned_tx_hex
}

fn edited(edit: impl FnOnce(&mut UnsignedTransaction)) -> String {
    let mut unsigned = UnsignedTransaction::from_bytes(&hex::decode(request()).unwrap()).unwrap();
    edit(&mut unsigned);
    hex::encode(unsigned.serialize())
}

fn sign(unsigned_hex: &str) -> OfflineSignResult {
    let cold = cold_keys();
    let reviewed = preview_unsigned_transaction(
        cold.spend_secret_key,
        cold.view_secret_key,
        unsigned_hex,
        "mainnet",
    )
    .expect("preview");
    let approval = reviewed.approve(u64::MAX, u64::MAX).expect("approve");
    sign_unsigned_transaction_with_private_keys(
        cold.spend_secret_key,
        cold.view_secret_key,
        unsigned_hex,
        "mainnet",
        &approval,
    )
    .expect("sign")
}

fn check(unsigned_hex: &str, signed: &OfflineSignResult) -> Result<(), String> {
    let cold = cold_keys();
    let spend_public = (&Scalar::from_bytes_mod_order(cold.spend_secret_key)
        * ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes();
    check_offline_signed_transaction(unsigned_hex, signed, &cold.view_secret_key, &spend_public)
}

fn relayable(signed: &OfflineSignResult) -> bool {
    let bytes = hex::decode(&signed.tx_blob).unwrap();
    Transaction::<NotPruned>::read(&mut bytes.as_slice()).is_ok()
}

#[test]
fn the_transaction_that_was_asked_for_passes() {
    let unsigned = request();
    let signed = sign(&unsigned);
    check(&unsigned, &signed).expect("the honest signer's transaction passes");
}

fn foreign(kind: &str) -> MoneroAddress {
    use monero_wallet::ed25519::{CompressedPoint, Scalar as OxideScalar};
    let spend = (&Scalar::from(1_234_567u64) * ED25519_BASEPOINT_TABLE)
        .compress()
        .to_bytes();
    let spend = CompressedPoint::read(&mut spend.as_slice())
        .unwrap()
        .decompress()
        .unwrap();
    let view = OxideScalar::read(&mut Scalar::from(7_654_321u64).to_bytes().as_slice()).unwrap();
    let pair = ViewPair::new(spend, Zeroizing::new(view)).unwrap();
    match kind {
        "standard" => pair.legacy_address(Network::Mainnet),
        "subaddress" => pair.subaddress(Network::Mainnet, SubaddressIndex::new(0, 1).unwrap()),
        "integrated" => pair.legacy_integrated_address(Network::Mainnet, [0x5a; 8]),
        _ => unreachable!(),
    }
}

#[test]
fn honest_payments_to_other_wallets_pass() {
    let cases: [Vec<(&str, u64)>; 4] = [
        vec![("standard", 300_000_000_000)],
        vec![("subaddress", 300_000_000_000)],
        vec![
            ("standard", 300_000_000_000),
            ("subaddress", 200_000_000_000),
        ],
        vec![("integrated", 300_000_000_000)],
    ];
    for payments in cases {
        let unsigned = edited(|tx| {
            tx.payments = payments
                .iter()
                .map(|(kind, amount)| (foreign(kind), *amount))
                .collect();
        });
        let signed = sign(&unsigned);
        check(&unsigned, &signed).unwrap_or_else(|e| panic!("{payments:?}: {e}"));
        if payments.len() > 1 {
            assert!(
                !signed.tx_key_additional.is_empty(),
                "additional keys in use"
            );
        }
    }
}

#[test]
fn another_payment_id_is_refused() {
    let unsigned = edited(|tx| tx.payments = vec![(foreign("integrated"), 300_000_000_000)]);
    let forged = sign(&edited(|tx| {
        let pair_address = foreign("standard");
        tx.payments = vec![(
            MoneroAddress::new(
                Network::Mainnet,
                AddressType::LegacyIntegrated([0xa5; 8]),
                pair_address.spend(),
                pair_address.view(),
            ),
            300_000_000_000,
        )];
    }));
    assert!(relayable(&forged));
    let err = check(&unsigned, &forged).unwrap_err();
    assert!(err.contains("payment ID"), "{err}");
}

#[test]
fn a_payment_sent_elsewhere_is_refused() {
    let unsigned = request();
    let forged = sign(&edited(|tx| {
        tx.payments[0].0 = MoneroAddress::from_str(Network::Mainnet, FOREIGN_STANDARD).unwrap();
    }));
    assert!(relayable(&forged));
    let err = check(&unsigned, &forged).unwrap_err();
    assert!(
        err.contains("not one the unsigned transaction requested"),
        "{err}"
    );
}

#[test]
fn change_moved_into_a_payment_is_refused() {
    let unsigned = request();
    let forged = sign(&edited(|tx| tx.payments[0].1 += 100_000_000));
    assert!(relayable(&forged));
    let err = check(&unsigned, &forged).unwrap_err();
    assert!(
        err.contains("not one the unsigned transaction requested"),
        "{err}"
    );
}

#[test]
fn another_fee_is_refused() {
    let unsigned = request();
    let forged = sign(&edited(|tx| {
        tx.fee_rate = FeeRate::new(tx.fee_rate.per_weight() * 4, 10_000).unwrap();
    }));
    assert!(relayable(&forged));
    let err = check(&unsigned, &forged).unwrap_err();
    assert!(err.contains("pays a fee of"), "{err}");
}

#[test]
fn other_inputs_are_refused() {
    let unsigned = request();
    let forged = sign(&edited(|tx| {
        tx.inputs.reverse();
        tx.inputs.truncate(1);
    }));
    assert!(relayable(&forged));
    let err = check(&unsigned, &forged).unwrap_err();
    assert!(err.contains("inputs"), "{err}");
}

#[test]
fn a_sign_result_that_misstates_its_transaction_is_refused() {
    let unsigned = request();
    let honest = sign(&unsigned);

    let mut wrong_key = honest.clone();
    wrong_key.tx_key = hex::encode(Scalar::from(7u64).to_bytes());
    wrong_key.transactions[0].tx_key = wrong_key.tx_key.clone();
    let err = check(&unsigned, &wrong_key).unwrap_err();
    assert!(err.contains("transaction keys"), "{err}");

    let mut wrong_images = honest.clone();
    wrong_images.spent_key_images[0] = "00".repeat(32);
    wrong_images.transactions[0].spent_key_images = wrong_images.spent_key_images.clone();
    let err = check(&unsigned, &wrong_images).unwrap_err();
    assert!(err.contains("key images"), "{err}");

    let mut inconsistent = honest;
    inconsistent.transactions[0].fee += 1;
    let err = check(&unsigned, &inconsistent).unwrap_err();
    assert!(err.contains("another transaction"), "{err}");
}

fn with_extra(signed: &OfflineSignResult, extra: impl FnOnce(&mut Vec<u8>)) -> OfflineSignResult {
    let bytes = hex::decode(&signed.tx_blob).unwrap();
    let mut tx = Transaction::<NotPruned>::read(&mut bytes.as_slice()).unwrap();
    let Transaction::V2 { prefix, .. } = &mut tx else {
        unreachable!()
    };
    extra(&mut prefix.extra);
    let mut forged = signed.clone();
    forged.tx_blob = hex::encode(tx.serialize());
    forged.tx_id = hex::encode(tx.hash());
    forged.transactions[0].tx_blob = forged.tx_blob.clone();
    forged.transactions[0].tx_id = forged.tx_id.clone();
    forged
}

#[test]
fn an_extra_with_more_than_keys_and_payment_id_is_refused() {
    use monero_wallet::extra::{ExtraField, PaymentId};
    let unsigned = request();
    let honest = sign(&unsigned);
    check(&unsigned, &honest).expect("the honest result passes");
    let nonce = |data: Vec<u8>| {
        let mut out = vec![];
        ExtraField::Nonce(data).write(&mut out).unwrap();
        out
    };
    let cases: Vec<(&str, OfflineSignResult)> = vec![
        (
            "a nonce of the signer's",
            with_extra(&honest, |extra| extra.extend(nonce(b"\x7ftagged".to_vec()))),
        ),
        (
            "an unencrypted payment ID",
            with_extra(&honest, |extra| {
                extra.extend(nonce(PaymentId::Unencrypted([7; 32]).serialize()))
            }),
        ),
        (
            "a second encrypted payment ID",
            with_extra(&honest, |extra| {
                extra.extend(nonce(PaymentId::Encrypted([7; 8]).serialize()))
            }),
        ),
        (
            "padding",
            with_extra(&honest, |extra| extra.extend([0, 0, 0])),
        ),
        (
            "the keys alone",
            with_extra(&honest, |extra| extra.truncate(33)),
        ),
    ];
    for (case, forged) in cases {
        assert!(relayable(&forged), "{case}");
        let err = check(&unsigned, &forged).expect_err(case);
        assert!(err.contains("extra"), "{case}: {err}");
    }

    let integrated = edited(|tx| tx.payments = vec![(foreign("integrated"), 300_000_000_000)]);
    let honest = sign(&integrated);
    check(&integrated, &honest).expect("the honest result passes");
    let doubled = with_extra(&honest, |extra| {
        extra.extend(nonce(PaymentId::Encrypted([0; 8]).serialize()))
    });
    assert!(check(&integrated, &doubled).unwrap_err().contains("extra"));
}

#[test]
fn change_outputs_the_transaction_does_not_have_are_refused() {
    let unsigned = request();
    let honest = sign(&unsigned);
    assert!(!honest.change_outputs.is_empty(), "the request has change");
    let edit = |edit: &dyn Fn(&mut Vec<monero_rust::native::ChangeOutputInfo>)| {
        let mut forged = honest.clone();
        edit(&mut forged.change_outputs);
        edit(&mut forged.transactions[0].change_outputs);
        forged
    };
    let cases: Vec<(&str, OfflineSignResult)> = vec![
        ("another amount", edit(&|outputs| outputs[0].amount += 1)),
        (
            "another one-time key",
            edit(&|outputs| outputs[0].key = outputs[0].key_offset.clone()),
        ),
        (
            "another commitment mask",
            edit(&|outputs| outputs[0].commitment_mask = "11".repeat(32)),
        ),
        (
            "another index",
            edit(&|outputs| outputs[0].output_index ^= 1),
        ),
        ("one left out", edit(&|outputs| outputs.clear())),
        (
            "one added",
            edit(&|outputs| {
                let copy = outputs[0].clone();
                outputs.push(copy)
            }),
        ),
    ];
    for (case, forged) in cases {
        let err = check(&unsigned, &forged).expect_err(case);
        assert!(err.contains("coming back to the wallet"), "{case}: {err}");
    }

    let mut inconsistent = honest.clone();
    inconsistent.change_outputs[0].amount += 1;
    let err = check(&unsigned, &inconsistent).unwrap_err();
    assert!(err.contains("another transaction"), "{err}");

    let other_image = edit(&|outputs| outputs[0].key_image = "22".repeat(32));
    check(&unsigned, &other_image).expect("a change key image is not checked");
}

#[derive(Clone, Debug, Default)]
struct Node(Arc<Mutex<Vec<String>>>);

#[async_trait::async_trait]
impl RpcConnection for Node {
    async fn post(&self, route: &str, body: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        assert_eq!(
            route, "sendrawtransaction",
            "only the relay reaches the node"
        );
        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
        self.0
            .lock()
            .unwrap()
            .push(request["tx_as_hex"].as_str().unwrap().to_string());
        Ok(br#"{"status":"OK","reason":""}"#.to_vec())
    }
}

#[tokio::test]
async fn the_cold_flow_relays_only_what_was_asked_for() {
    let cold = cold_keys();
    let view_key_hex = hex::encode(cold.view_secret_key);
    let spend_public_hex = hex::encode(
        (&Scalar::from_bytes_mod_order(cold.spend_secret_key) * ED25519_BASEPOINT_TABLE)
            .compress()
            .to_bytes(),
    );
    let node = Node::default();
    let rpc = Rpc::new_with_connection(node.clone());
    let mut wallet_state = WalletState::new();
    let relay = |request: &str, signed: &OfflineSignResult| {
        let request = UnsignedTransactionResult {
            unsigned_tx_hex: request.to_string(),
            fee: 0,
            recipients: vec![],
            input_rings: vec![],
        };
        (request, signed.clone())
    };

    let unsigned = request();
    let mut forged = vec![
        (
            "payment elsewhere",
            sign(&edited(|tx| {
                tx.payments[0].0 =
                    MoneroAddress::from_str(Network::Mainnet, FOREIGN_STANDARD).unwrap()
            })),
        ),
        (
            "change moved into the payment",
            sign(&edited(|tx| tx.payments[0].1 += 100_000_000)),
        ),
        (
            "another fee",
            sign(&edited(|tx| {
                tx.fee_rate = FeeRate::new(tx.fee_rate.per_weight() * 4, 10_000).unwrap();
            })),
        ),
        (
            "other inputs",
            sign(&edited(|tx| {
                tx.inputs.reverse();
                tx.inputs.truncate(1);
            })),
        ),
    ];
    let mut wrong_key = sign(&unsigned);
    wrong_key.tx_key = hex::encode(Scalar::from(7u64).to_bytes());
    wrong_key.transactions[0].tx_key = wrong_key.tx_key.clone();
    forged.push(("misstated transaction key", wrong_key));

    for (case, signed) in &forged {
        let (request, signed) = relay(&unsigned, signed);
        let err = broadcast_offline_signed_transaction_with_rpc(
            &rpc,
            &request,
            &signed,
            &view_key_hex,
            &spend_public_hex,
            &mut wallet_state,
            false,
        )
        .await
        .expect_err(case);
        assert!(err.starts_with("Refusing to relay"), "{case}: {err}");
    }
    assert!(node.0.lock().unwrap().is_empty(), "nothing forged was sent");

    let integrated = edited(|tx| tx.payments = vec![(foreign("integrated"), 300_000_000_000)]);
    let other_id = sign(&edited(|tx| {
        let address = foreign("standard");
        tx.payments = vec![(
            MoneroAddress::new(
                Network::Mainnet,
                AddressType::LegacyIntegrated([0xa5; 8]),
                address.spend(),
                address.view(),
            ),
            300_000_000_000,
        )];
    }));
    let (request, signed) = relay(&integrated, &other_id);
    assert!(broadcast_offline_signed_transaction_with_rpc(
        &rpc,
        &request,
        &signed,
        &view_key_hex,
        &spend_public_hex,
        &mut wallet_state,
        false,
    )
    .await
    .is_err());
    assert!(node.0.lock().unwrap().is_empty(), "nothing forged was sent");

    let honest = sign(&unsigned);
    let (request, signed) = relay(&unsigned, &honest);
    broadcast_offline_signed_transaction_with_rpc(
        &rpc,
        &request,
        &signed,
        &view_key_hex,
        &spend_public_hex,
        &mut wallet_state,
        false,
    )
    .await
    .expect("the honest result is relayed");
    assert_eq!(*node.0.lock().unwrap(), vec![honest.tx_blob.clone()]);
}
