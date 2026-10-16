//! A send must look like one wallet2 would build.

mod common;

use std::path::Path;

use common::offline::{
    encode, sign_reviewed_keys, unsigned_for, Wallet, FEE, INPUT_AMOUNT, PAYMENT_AMOUNT,
};
use curve25519_dalek::{edwards::EdwardsPoint, scalar::Scalar};
use monero_oxide::transaction::{NotPruned, Transaction};
use monero_rust::unsigned_tx::{sign_offline, UnsignedChange, UnsignedTransaction};
use monero_wallet::{
    address::{MoneroAddress, Network},
    extra::Extra,
};
use rand::SeedableRng;
use zeroize::Zeroizing;

const NETWORK_STR: &str = "stagenet";

fn sender() -> Wallet {
    Wallet::from_spend(Scalar::from(2_345_678u64))
}

fn subaddress_send(sender: &Wallet) -> UnsignedTransaction {
    let recipient = Wallet::from_spend(Scalar::from(777u64)).subaddress(Network::Stagenet, 0, 1);
    unsigned_for(sender, Network::Stagenet, vec![(recipient, PAYMENT_AMOUNT)])
}

fn sign(wallet: &Wallet, unsigned: &UnsignedTransaction) -> Transaction<NotPruned> {
    let signed = sign_reviewed_keys(wallet, &encode(unsigned), NETWORK_STR).expect("sign");
    let blob = hex::decode(signed.tx_blob).expect("hex");
    Transaction::<NotPruned>::read(&mut blob.as_slice()).expect("signed transaction")
}

fn extra_counts(tx: &Transaction<NotPruned>) -> (usize, usize) {
    let extra = Extra::read(&mut tx.prefix().extra.as_slice()).expect("transaction extra");
    let (keys, additional) = extra.keys().expect("transaction public key");
    (keys.len(), additional.map_or(0, |keys| keys.len()))
}

#[test]
fn two_output_send_to_a_subaddress_uses_no_additional_tx_keys() {
    let sender = sender();
    let tx = sign(&sender, &subaddress_send(&sender));
    assert_eq!(tx.prefix().outputs.len(), 2);

    let extra = monero_wallet::extra::Extra::read(&mut tx.prefix().extra.as_slice())
        .expect("transaction extra");
    let (tx_keys, additional) = extra.keys().expect("transaction public key");
    assert_eq!(tx_keys.len(), 1, "one transaction public key");
    assert_eq!(
        additional.map_or(0, |keys| keys.len()),
        0,
        "wallet2 uses no additional transaction keys for a subaddress payment with change"
    );
    // Transaction public key (1 + 32) and the encrypted dummy payment ID nonce
    // (1 + 1 + 1 + 8), as wallet2 writes them.
    assert_eq!(tx.prefix().extra.len(), 44);
}

#[test]
fn unsigned_bytes_do_not_contain_the_view_key() {
    let sender = sender();
    let unsigned = subaddress_send(&sender).serialize();
    for secret in [sender.view.to_bytes(), sender.spend.to_bytes()] {
        assert!(
            !unsigned.windows(32).any(|window| window == secret),
            "the unsigned transaction carries a private key of the wallet"
        );
    }
}

#[test]
fn destination_shapes_follow_wallet2_key_rules() {
    struct Shape {
        name: &'static str,
        destinations: Vec<MoneroAddress>,
        wallet2_additional_keys: usize,
        single_subaddress: Option<MoneroAddress>,
    }

    let sender = sender();
    let recipient = Wallet::from_spend(Scalar::from(777u64));
    let standard = recipient.address(Network::Stagenet);
    let subaddress = recipient.subaddress(Network::Stagenet, 0, 1);
    let shapes = vec![
        Shape {
            name: "one standard address",
            destinations: vec![standard],
            wallet2_additional_keys: 0,
            single_subaddress: None,
        },
        Shape {
            name: "one subaddress",
            destinations: vec![subaddress],
            wallet2_additional_keys: 0,
            single_subaddress: Some(subaddress),
        },
        Shape {
            name: "two standard addresses",
            destinations: vec![
                standard,
                Wallet::from_spend(Scalar::from(778u64)).address(Network::Stagenet),
            ],
            wallet2_additional_keys: 0,
            single_subaddress: None,
        },
        Shape {
            name: "one standard address and one subaddress",
            destinations: vec![standard, subaddress],
            wallet2_additional_keys: usize::MAX,
            single_subaddress: None,
        },
        Shape {
            name: "the same subaddress twice",
            destinations: vec![subaddress, subaddress],
            wallet2_additional_keys: 0,
            single_subaddress: Some(subaddress),
        },
    ];

    for shape in shapes {
        for with_change in [true, false] {
            let output_count = if with_change || shape.destinations.len() == 1 {
                shape.destinations.len() + 1
            } else {
                shape.destinations.len()
            };
            let payment_total = if with_change {
                PAYMENT_AMOUNT
            } else {
                INPUT_AMOUNT - FEE
            };
            let each = payment_total / shape.destinations.len() as u64;
            let mut left = payment_total;
            let payments = shape
                .destinations
                .iter()
                .enumerate()
                .map(|(index, address)| {
                    let amount = if index + 1 == shape.destinations.len() {
                        left
                    } else {
                        left -= each;
                        each
                    };
                    (*address, amount)
                })
                .collect();
            let mut unsigned = unsigned_for(&sender, Network::Stagenet, payments);
            if !with_change {
                unsigned.change = if shape.destinations.len() == 1 {
                    Some(UnsignedChange::Dummy)
                } else {
                    None
                };
            }

            let mut rng = rand::rngs::StdRng::seed_from_u64(12345);
            let signed = sign_offline(
                &mut rng,
                &Zeroizing::new(sender.spend),
                &Zeroizing::new(sender.view),
                &unsigned,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{} with_change={with_change} should sign: {error}",
                    shape.name
                )
            });
            let (tx_keys, additional_keys) = extra_counts(&signed.tx);
            assert_eq!(
                signed.tx.prefix().outputs.len(),
                output_count,
                "{} with_change={with_change}",
                shape.name
            );
            assert_eq!(tx_keys, 1, "{} with_change={with_change}", shape.name);

            let wallet2_additional = if shape.wallet2_additional_keys == usize::MAX {
                output_count
            } else {
                shape.wallet2_additional_keys
            };
            assert_eq!(
                additional_keys, wallet2_additional,
                "{} with_change={with_change}",
                shape.name
            );

            if let Some(subaddress) = shape.single_subaddress {
                let extra = Extra::read(&mut signed.tx.prefix().extra.as_slice()).unwrap();
                let (keys, _) = extra.keys().unwrap();
                let spend: EdwardsPoint = subaddress.spend().into();
                let expected = &*signed.tx_key * spend;
                let actual: EdwardsPoint = keys[0].into();
                assert_eq!(
                    actual, expected,
                    "{} with_change={with_change} must use R = r*D",
                    shape.name
                );
            }
        }
    }
}

#[test]
fn wallet2_zero_change_txsets_convert_and_sign_with_a_dummy_output() {
    let vector = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/cold_signing_regtest_zero_change");
    let cold = monero_rust::wallet_keys_file::read_keys_file(&vector.join("cold_full.keys"), "")
        .expect("cold wallet keys");
    let wallet = Wallet {
        spend: Scalar::from_bytes_mod_order(cold.spend_secret_key),
        view: Scalar::from_bytes_mod_order(cold.view_secret_key),
    };

    for name in ["standard", "subaddress"] {
        let txset = std::fs::read(vector.join(format!("zero_change_{name}_unsigned_monero_tx")))
            .expect("wallet2 unsigned txset");
        let converted = monero_rust::native::create_unsigned_transaction_from_wallet2_txset_keys(
            cold.spend_secret_key,
            cold.view_secret_key,
            &txset,
            "mainnet",
        )
        .unwrap_or_else(|error| panic!("{name} txset should convert: {error}"));
        let bytes = hex::decode(&converted.unsigned_tx_hex).unwrap();
        let unsigned = UnsignedTransaction::from_bytes(&bytes).unwrap();
        assert_eq!(unsigned.change, Some(UnsignedChange::Dummy));
        let resolved = unsigned.resolve(&wallet.view_pair()).unwrap();
        assert_eq!(resolved.dummy_amount, Some(0));
        assert!(resolved.change.is_none());

        let preview = monero_rust::native::preview_unsigned_transaction(
            cold.spend_secret_key,
            cold.view_secret_key,
            &hex::encode(&txset),
            "mainnet",
        )
        .unwrap();
        assert_eq!(preview.transactions[0].destinations.len(), 2);
        let dummy = &preview.transactions[0].destinations[1];
        assert_eq!(dummy.address, "");
        assert_eq!(dummy.amount, 0);
        assert!(dummy.is_dummy);
        assert!(!dummy.is_change);
        assert!(!dummy.is_ours);

        let app_signed = sign_reviewed_keys(&wallet, &hex::encode(&txset), "mainnet")
            .unwrap_or_else(|error| panic!("{name} txset should sign: {error}"));
        let app_blob = hex::decode(app_signed.tx_blob).unwrap();
        let app_tx = Transaction::<NotPruned>::read(&mut app_blob.as_slice()).unwrap();

        let wallet2_signed =
            std::fs::read(vector.join(format!("zero_change_{name}_signed_monero_tx")))
                .expect("wallet2 signed txset");
        let wallet2_txs = monero_rust::epee_compat::extract_signed_monero_txset_transactions(
            &wallet2_signed,
            &cold.view_secret_key,
        )
        .unwrap();
        let wallet2_tx =
            Transaction::<NotPruned>::read(&mut wallet2_txs[0].tx_blob.as_slice()).unwrap();

        assert_eq!(app_tx.prefix().outputs.len(), 2);
        assert_eq!(extra_counts(&app_tx), (1, 0));
        assert_eq!(app_tx.prefix().extra.len(), 44);
        assert_eq!(
            app_tx.prefix().outputs.len(),
            wallet2_tx.prefix().outputs.len()
        );
        assert_eq!(extra_counts(&app_tx), extra_counts(&wallet2_tx));
        assert_eq!(app_tx.prefix().extra.len(), wallet2_tx.prefix().extra.len());
    }
}
