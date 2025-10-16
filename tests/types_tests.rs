use monero_rust::types::{Transaction, TransactionDirection, TxKey};

#[test]
fn test_incoming_transaction() {
    let txid = [0u8; 32];
    let tx = Transaction::new_incoming(txid, Some(100), 1234567890, 1000000000000);

    assert_eq!(tx.txid, txid);
    assert_eq!(tx.height, Some(100));
    assert_eq!(tx.amount, 1000000000000);
    assert_eq!(tx.direction, TransactionDirection::Incoming);
    assert!(!tx.is_pending);
}

#[test]
fn test_outgoing_transaction() {
    let txid = [1u8; 32];
    let destinations = vec!["address1".to_string(), "address2".to_string()];
    let tx = Transaction::new_outgoing(txid, None, 1234567890, 1000000000000, 50000000, destinations.clone());

    assert_eq!(tx.amount, -1000000000000);
    assert_eq!(tx.fee, Some(50000000));
    assert_eq!(tx.destinations, destinations);
    assert_eq!(tx.direction, TransactionDirection::Outgoing);
    assert!(tx.is_pending);
}

#[test]
fn test_confirmation_update() {
    let mut tx = Transaction::new_incoming([0u8; 32], Some(100), 1234567890, 1000000000000);
    tx.update_confirmations(105);
    assert_eq!(tx.confirmations, 6);
    assert!(tx.is_confirmed());
}

#[test]
fn test_tx_key() {
    let txid = [2u8; 32];
    let key = [3u8; 32];
    let mut tx_key = TxKey::new(txid, key);

    assert_eq!(tx_key.txid, txid);
    assert_eq!(*tx_key.tx_private_key, key);
    assert!(tx_key.additional_tx_keys.is_empty());

    tx_key.add_additional_key([4u8; 32]);
    assert_eq!(tx_key.additional_tx_keys.len(), 1);
    assert_eq!(*tx_key.additional_tx_keys[0], [4u8; 32]);
}
