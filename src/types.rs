//! Wallet types.

use serde::{Deserialize, Serialize};

pub use monero_wallet::WalletOutput;

pub type KeyImage = [u8; 32];

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub txid: [u8; 32],
    pub height: Option<u64>,
    pub timestamp: u64,
    /// Positive for incoming, negative for outgoing (piconeros)
    pub amount: i64,
    pub fee: Option<u64>,
    pub destinations: Vec<String>,
    pub payment_id: Option<Vec<u8>>,
    pub direction: TransactionDirection,
    pub confirmations: u64,
    pub is_pending: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionDirection {
    Incoming,
    Outgoing,
}

/// For payment proofs
#[derive(Debug, Serialize, Deserialize)]
pub struct TxKey {
    pub txid: [u8; 32],
    #[serde(
        serialize_with = "serialize_zeroizing_bytes",
        deserialize_with = "deserialize_zeroizing_bytes"
    )]
    pub tx_private_key: zeroize::Zeroizing<[u8; 32]>,
    #[serde(
        serialize_with = "serialize_zeroizing_vec",
        deserialize_with = "deserialize_zeroizing_vec"
    )]
    pub additional_tx_keys: Vec<zeroize::Zeroizing<[u8; 32]>>,
}

fn serialize_zeroizing_bytes<S>(
    data: &zeroize::Zeroizing<[u8; 32]>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_bytes(&**data)
}

fn deserialize_zeroizing_bytes<'de, D>(
    deserializer: D,
) -> Result<zeroize::Zeroizing<[u8; 32]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
    if bytes.len() != 32 {
        return Err(serde::de::Error::custom(format!(
            "Invalid key length: expected 32, got {}",
            bytes.len()
        )));
    }
    let mut array = zeroize::Zeroizing::new([0u8; 32]);
    array.copy_from_slice(&bytes);
    Ok(array)
}

fn serialize_zeroizing_vec<S>(
    data: &Vec<zeroize::Zeroizing<[u8; 32]>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(data.len()))?;
    for item in data {
        seq.serialize_element(&**item)?;
    }
    seq.end()
}

fn deserialize_zeroizing_vec<'de, D>(
    deserializer: D,
) -> Result<Vec<zeroize::Zeroizing<[u8; 32]>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let vec_of_vecs: Vec<Vec<u8>> = serde::Deserialize::deserialize(deserializer)?;
    let mut result = Vec::with_capacity(vec_of_vecs.len());

    for bytes in vec_of_vecs {
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom(format!(
                "Invalid key length: expected 32, got {}",
                bytes.len()
            )));
        }
        let mut array = zeroize::Zeroizing::new([0u8; 32]);
        array.copy_from_slice(&bytes);
        result.push(array);
    }

    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableOutput {
    pub tx_hash: [u8; 32],
    /// u64 to support large transactions
    pub output_index: u64,
    pub amount: u64,
    pub key_image: KeyImage,
    pub subaddress_indices: (u32, u32),
    pub height: u64,
    pub unlocked: bool,
    pub spent: bool,
    pub frozen: bool,
}

impl Transaction {
    pub fn new_incoming(
        txid: [u8; 32],
        height: Option<u64>,
        timestamp: u64,
        amount: u64,
    ) -> Self {
        Self {
            txid,
            height,
            timestamp,
            amount: amount as i64,
            fee: None,
            destinations: Vec::new(),
            payment_id: None,
            direction: TransactionDirection::Incoming,
            confirmations: 0,
            is_pending: height.is_none(),
        }
    }

    pub fn new_outgoing(
        txid: [u8; 32],
        height: Option<u64>,
        timestamp: u64,
        amount: u64,
        fee: u64,
        destinations: Vec<String>,
    ) -> Self {
        Self {
            txid,
            height,
            timestamp,
            amount: -(amount as i64),
            fee: Some(fee),
            destinations,
            payment_id: None,
            direction: TransactionDirection::Outgoing,
            confirmations: 0,
            is_pending: height.is_none(),
        }
    }

    pub fn update_confirmations(&mut self, daemon_height: u64) {
        if let Some(tx_height) = self.height {
            self.confirmations = daemon_height.saturating_sub(tx_height) + 1;
        }
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmations > 0
    }
}

impl TxKey {
    pub fn new(txid: [u8; 32], tx_private_key: [u8; 32]) -> Self {
        Self {
            txid,
            tx_private_key: zeroize::Zeroizing::new(tx_private_key),
            additional_tx_keys: Vec::new(),
        }
    }

    pub fn add_additional_key(&mut self, key: [u8; 32]) {
        self.additional_tx_keys.push(zeroize::Zeroizing::new(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_creation() {
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
        let tx = Transaction::new_outgoing(
            txid,
            None,
            1234567890,
            1000000000000,
            50000000,
            destinations.clone(),
        );

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
    fn test_tx_key_creation() {
        let txid = [2u8; 32];
        let key = [3u8; 32];
        let mut tx_key = TxKey::new(txid, key);

        assert_eq!(tx_key.txid, txid);
        assert_eq!(*tx_key.tx_private_key, key);
        assert_eq!(tx_key.additional_tx_keys.len(), 0);

        tx_key.add_additional_key([4u8; 32]);
        assert_eq!(tx_key.additional_tx_keys.len(), 1);
        assert_eq!(*tx_key.additional_tx_keys[0], [4u8; 32]);
    }
}
