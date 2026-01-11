//! Supporting types for wallet state management.
//!
//! This module defines the core data structures used throughout the wallet
//! implementation, including transaction tracking, output management, and
//! key image handling.

use serde::{Deserialize, Serialize};

// Re-export WalletOutput from monero-oxide for output tracking.
// Note: Will need to implement custom serialization for this type.
pub use monero_wallet::WalletOutput;

/// Type alias for key images used to identify outputs.
/// Key images are unique identifiers derived from output public keys and
/// the wallet's private view key, used to detect spent outputs.
pub type KeyImage = [u8; 32];

/// Represents a Monero transaction with all relevant metadata.
///
/// This structure tracks both incoming and outgoing transactions,
/// storing essential information needed for transaction history,
/// balance calculation, and UI display.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    /// Transaction ID (hash)
    pub txid: [u8; 32],

    /// Block height where transaction was confirmed (None if pending)
    pub height: Option<u64>,

    /// Unix timestamp when transaction was created/detected
    pub timestamp: u64,

    /// Net amount for this wallet (positive for incoming, negative for outgoing)
    /// Stored in atomic units (piconeros)
    pub amount: i64,

    /// Transaction fee in atomic units (only for outgoing transactions)
    pub fee: Option<u64>,

    /// Destination addresses for outgoing transactions
    pub destinations: Vec<String>,

    /// Payment ID (deprecated but still supported for compatibility)
    pub payment_id: Option<Vec<u8>>,

    /// Whether this is an incoming or outgoing transaction
    pub direction: TransactionDirection,

    /// Number of confirmations (daemon height - tx height + 1)
    pub confirmations: u64,

    /// Whether the transaction is in the mempool (unconfirmed)
    pub is_pending: bool,
}

/// Direction of a transaction relative to the wallet.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionDirection {
    /// Incoming transaction (receiving funds)
    Incoming,
    /// Outgoing transaction (sending funds)
    Outgoing,
}

/// Transaction key for proving ownership/sending of a transaction.
///
/// Transaction keys are used to generate payment proofs and verify
/// that a transaction was sent to a specific address.
///
/// SECURITY: Transaction private keys are sensitive and wrapped in Zeroizing
/// to ensure they are cleared from memory when dropped.
#[derive(Debug, Serialize, Deserialize)]
pub struct TxKey {
    /// The transaction ID this key is associated with
    pub txid: [u8; 32],

    /// The transaction private key (r) - SENSITIVE
    /// This is a scalar value used in the transaction's one-time key derivation
    /// Wrapped in Zeroizing to clear from memory on drop
    #[serde(
        serialize_with = "serialize_zeroizing_bytes",
        deserialize_with = "deserialize_zeroizing_bytes"
    )]
    pub tx_private_key: zeroize::Zeroizing<[u8; 32]>,

    /// Additional transaction keys for multi-output transactions
    /// One per output (for subaddress support)
    /// Each key is wrapped in Zeroizing for security
    #[serde(
        serialize_with = "serialize_zeroizing_vec",
        deserialize_with = "deserialize_zeroizing_vec"
    )]
    pub additional_tx_keys: Vec<zeroize::Zeroizing<[u8; 32]>>,
}

// Custom serialization for Zeroizing<[u8; 32]>
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

// Custom serialization for Vec<Zeroizing<[u8; 32]>>
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

/// Wrapper for WalletOutput with serialization support.
///
/// Since monero-oxide's WalletOutput may not implement Serialize/Deserialize,
/// we'll need to create a serializable version that can be converted to/from
/// the original type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableOutput {
    /// Transaction hash
    pub tx_hash: [u8; 32],

    /// Output index in the transaction (u64 to support large transactions)
    pub output_index: u64,

    /// Amount in atomic units (piconeros)
    pub amount: u64,

    /// Key image for this output (used to detect if spent)
    pub key_image: KeyImage,

    /// Subaddress indices (account, index)
    pub subaddress_indices: (u32, u32),

    /// Block height where output appeared
    pub height: u64,

    /// Whether this output is currently spendable (unlocked)
    pub unlocked: bool,

    /// Whether this output has been spent
    /// NOTE: This is redundant with WalletState.spent_outputs HashSet
    /// The HashSet is the canonical source of truth
    pub spent: bool,

    /// Whether this output is frozen (manually locked by user)
    /// NOTE: This is redundant with WalletState.frozen_outputs HashSet
    /// The HashSet is the canonical source of truth
    pub frozen: bool,
}

impl Transaction {
    /// Creates a new incoming transaction.
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

    /// Creates a new outgoing transaction.
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

    /// Updates the confirmation count based on current daemon height.
    pub fn update_confirmations(&mut self, daemon_height: u64) {
        if let Some(tx_height) = self.height {
            self.confirmations = daemon_height.saturating_sub(tx_height) + 1;
        }
    }

    /// Checks if the transaction is confirmed (at least 1 confirmation).
    pub fn is_confirmed(&self) -> bool {
        self.confirmations > 0
    }
}

impl TxKey {
    /// Creates a new transaction key.
    pub fn new(txid: [u8; 32], tx_private_key: [u8; 32]) -> Self {
        Self {
            txid,
            tx_private_key: zeroize::Zeroizing::new(tx_private_key),
            additional_tx_keys: Vec::new(),
        }
    }

    /// Adds an additional transaction key (for subaddress outputs).
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
