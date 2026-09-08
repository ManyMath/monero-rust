//! A send must look like one wallet2 would build.

mod common;

use common::offline::{encode, sign_reviewed_keys, unsigned_for, Wallet, PAYMENT_AMOUNT};
use curve25519_dalek::scalar::Scalar;
use monero_oxide::transaction::{NotPruned, Transaction};
use monero_rust::unsigned_tx::UnsignedTransaction;
use monero_wallet::address::Network;

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
