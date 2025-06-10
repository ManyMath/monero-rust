use serde::{Deserialize, Serialize};

use crate::tx_builder::native::StoredOutputData;

/// Canonical wallet output type representing a received, owned output.
///
/// This is the single source of truth for output data across the library.
/// Scanner results, wallet state, and transaction building all use this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletOutput {
    pub tx_hash: String,
    pub output_index: u8,
    pub amount: u64,
    pub amount_xmr: String,
    pub key: String,
    pub key_offset: String,
    pub commitment_mask: String,
    pub subaddress_index: Option<(u32, u32)>,
    pub payment_id: Option<String>,
    pub received_output_bytes: String,
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
        }
    }
}

/// Unique key for identifying an output: "txHash:outputIndex"
impl WalletOutput {
    pub fn output_key(&self) -> String {
        format!("{}:{}", self.tx_hash, self.output_index)
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
            amount_xmr: "1.000000000000".into(),
            key: "key".into(),
            key_offset: "offset".into(),
            commitment_mask: "mask".into(),
            subaddress_index: Some((0, 1)),
            payment_id: None,
            received_output_bytes: "bytes".into(),
            block_height: 100,
            spent: false,
            spent_height: None,
            key_image: "ki_abc".into(),
            is_coinbase: false,
            frozen: false,
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
