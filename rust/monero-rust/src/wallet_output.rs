use serde::{Deserialize, Serialize};

use crate::tx_builder::native::{DecoySelection, StoredOutputData};

/// Canonical wallet output type representing a received, owned output.
///
/// This is the single source of truth for output data across the library.
/// Scanner results, wallet state, and transaction building all use this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletOutput {
    pub tx_hash: String,
    /// Position of the output in its transaction. Coinbase transactions can
    /// have far more than 255 outputs.
    pub output_index: u64,
    pub amount: u64,
    pub key: String,
    pub key_offset: String,
    pub commitment_mask: String,
    pub subaddress_index: Option<(u32, u32)>,
    pub payment_id: Option<String>,
    pub received_output_bytes: String,
    /// The output's global index; `None` for mempool and legacy records, which must
    /// be refreshed before spending: a lookup by tx hash reveals the real input.
    #[serde(default)]
    pub global_index: Option<u64>,
    /// A copy of the wallet's ring database entry for `key_image`; it is the
    /// database (`WalletState::ring_database`) that must be stored.
    #[serde(default)]
    pub ring: Option<DecoySelection>,
    pub block_height: u64,
    pub spent: bool,
    /// Height at which this output was spent. None if unspent or unknown.
    #[serde(default)]
    pub spent_height: Option<u64>,
    pub key_image: String,
    pub is_coinbase: bool,
    /// Whether this output is frozen (excluded from coin selection).
    #[serde(default)]
    pub frozen: bool,
    /// Output of a version 1 transaction (per-amount global index). This library
    /// cannot spend it; it counts in `Balance::pre_ringct` and is never selected.
    #[serde(default)]
    pub pre_ringct: bool,
    /// For a record that replaced a smaller output with the same one-time key: the
    /// height of the first output credited for the key, which keeps its position in
    /// key image exports as wallet2 overwrites in place (wallet2.cpp:2653).
    #[serde(default)]
    pub export_height: Option<u64>,
}

impl From<&WalletOutput> for StoredOutputData {
    fn from(o: &WalletOutput) -> Self {
        StoredOutputData {
            tx_hash: o.tx_hash.clone(),
            output_index: o.output_index,
            amount: o.amount,
            key: o.key.clone(),
            key_offset: o.key_offset.clone(),
            commitment_mask: o.commitment_mask.clone(),
            subaddress: o.subaddress_index,
            payment_id: o.payment_id.clone(),
            received_output_bytes: o.received_output_bytes.clone(),
            global_index: o.global_index,
            key_image: o.key_image.clone(),
            ring: o.ring.clone(),
            pre_ringct: o.pre_ringct,
        }
    }
}

impl From<WalletOutput> for StoredOutputData {
    fn from(o: WalletOutput) -> Self {
        StoredOutputData {
            tx_hash: o.tx_hash,
            output_index: o.output_index,
            amount: o.amount,
            key: o.key,
            key_offset: o.key_offset,
            commitment_mask: o.commitment_mask,
            subaddress: o.subaddress_index,
            payment_id: o.payment_id,
            received_output_bytes: o.received_output_bytes,
            global_index: o.global_index,
            key_image: o.key_image,
            ring: o.ring,
            pre_ringct: o.pre_ringct,
        }
    }
}

/// Unique key for identifying an output: "txHash:outputIndex"
impl WalletOutput {
    pub fn output_key(&self) -> String {
        format!("{}:{}", self.tx_hash, self.output_index)
    }

    /// Read the transaction's additional timelock from the serialized output.
    ///
    /// This lock applies in addition to the normal spendable age. The scanner
    /// stores the full `monero-wallet` output in `received_output_bytes`, including
    /// the timelock in its metadata.
    /// Read that stored value rather than maintain a second copy.
    ///
    /// Return `Timelock::None` if there is no additional lock or no serialized
    /// output data. Synthetic fixtures may leave the data empty.
    pub fn additional_timelock(&self) -> monero_wallet::transaction::Timelock {
        use monero_wallet::transaction::Timelock;

        if self.received_output_bytes.is_empty() {
            return Timelock::None;
        }
        let Ok(bytes) = hex::decode(&self.received_output_bytes) else {
            return Timelock::None;
        };
        match monero_wallet::WalletOutput::read(&mut bytes.as_slice()) {
            Ok(output) => output.additional_timelock(),
            Err(_) => Timelock::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_output() -> WalletOutput {
        WalletOutput {
            tx_hash: "abc123".into(),
            output_index: 0,
            amount: 1_000_000_000_000,
            key: "key".into(),
            key_offset: "offset".into(),
            commitment_mask: "mask".into(),
            subaddress_index: Some((0, 1)),
            payment_id: None,
            received_output_bytes: "bytes".into(),
            global_index: Some(42),
            ring: None,
            block_height: 100,
            spent: false,
            spent_height: None,
            key_image: "ki_abc".into(),
            is_coinbase: false,
            frozen: false,
            pre_ringct: false,
            export_height: None,
        }
    }

    #[test]
    fn test_output_key() {
        let o = sample_output();
        assert_eq!(o.output_key(), "abc123:0");
    }

    #[test]
    fn test_into_stored_output_data() {
        let o = sample_output();
        let stored: StoredOutputData = (&o).into();
        assert_eq!(stored.tx_hash, o.tx_hash);
        assert_eq!(stored.output_index, o.output_index);
        assert_eq!(stored.amount, o.amount);
        assert_eq!(stored.subaddress, o.subaddress_index);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let o = sample_output();
        let json = serde_json::to_string(&o).unwrap();
        let deserialized: WalletOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tx_hash, o.tx_hash);
        assert_eq!(deserialized.amount, o.amount);
        assert_eq!(deserialized.subaddress_index, o.subaddress_index);
        assert_eq!(deserialized.key_image, o.key_image);
        assert_eq!(deserialized.is_coinbase, o.is_coinbase);
        assert_eq!(deserialized.spent_height, None);
    }

    #[test]
    fn test_spent_height_serialization_roundtrip() {
        let mut o = sample_output();
        o.spent_height = Some(500);
        let json = serde_json::to_string(&o).unwrap();
        let deserialized: WalletOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.spent_height, Some(500));
    }

    #[test]
    fn test_frozen_serialization_roundtrip() {
        let mut o = sample_output();
        o.frozen = true;
        let json = serde_json::to_string(&o).unwrap();
        let deserialized: WalletOutput = serde_json::from_str(&json).unwrap();
        assert!(deserialized.frozen);
    }

    #[test]
    fn test_frozen_backward_compat() {
        // JSON without frozen field should deserialize to false
        let json = r#"{
            "tx_hash":"abc123","output_index":0,"amount":1000000000000,
            "amount_xmr":"1.000000000000","key":"key","key_offset":"offset",
            "commitment_mask":"mask","subaddress_index":[0,1],"payment_id":null,
            "received_output_bytes":"bytes","block_height":100,"spent":false,
            "key_image":"ki_abc","is_coinbase":false
        }"#;
        let deserialized: WalletOutput = serde_json::from_str(json).unwrap();
        assert!(!deserialized.frozen);
    }

    #[test]
    fn pre_ringct_flag_defaults_to_false_and_reaches_the_builder() {
        let json = r#"{
            "tx_hash":"abc123","output_index":0,"amount":1000000000000,
            "key":"key","key_offset":"offset",
            "commitment_mask":"mask","subaddress_index":[0,1],"payment_id":null,
            "received_output_bytes":"bytes","block_height":100,"spent":false,
            "key_image":"ki_abc","is_coinbase":false
        }"#;
        let deserialized: WalletOutput = serde_json::from_str(json).unwrap();
        assert!(!deserialized.pre_ringct);

        let mut o = sample_output();
        o.pre_ringct = true;
        let json = serde_json::to_string(&o).unwrap();
        assert!(
            serde_json::from_str::<WalletOutput>(&json)
                .unwrap()
                .pre_ringct
        );
        assert!(StoredOutputData::from(&o).pre_ringct);
        assert!(StoredOutputData::from(o).pre_ringct);
    }

    #[test]
    fn test_spent_height_backward_compat() {
        // JSON without spent_height should deserialize to None
        let json = r#"{
            "tx_hash":"abc123","output_index":0,"amount":1000000000000,
            "amount_xmr":"1.000000000000","key":"key","key_offset":"offset",
            "commitment_mask":"mask","subaddress_index":[0,1],"payment_id":null,
            "received_output_bytes":"bytes","block_height":100,"spent":false,
            "key_image":"ki_abc","is_coinbase":false
        }"#;
        let deserialized: WalletOutput = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.spent_height, None);
    }
}
