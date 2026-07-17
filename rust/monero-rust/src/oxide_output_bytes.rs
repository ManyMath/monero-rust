//! Byte-level helpers for `monero-wallet` output serializations.
//!
//! `monero_wallet::WalletOutput` and `OutputWithDecoys` keep their fields
//! private, so the send path works through their stable serializations:
//! splicing a corrected `index_on_blockchain` into stored output bytes (the
//! scanner writes 0 when the daemon didn't provide output indices) and
//! assembling an `OutputWithDecoys` from a stored output plus locally
//! selected decoys.

// The send-path port consumes these helpers over the next commits.
#![allow(dead_code)]

use monero_oxide::ringct::clsag::Decoys;
use monero_wallet::{OutputWithDecoys, WalletOutput};

/// Byte range of `index_on_blockchain` in a serialized `WalletOutput`.
///
/// The serialization starts with the fixed-size AbsoluteId: transaction hash
/// (32 bytes) then `index_in_transaction` (8 bytes little-endian), followed by
/// the RelativeId, which is `index_on_blockchain` (8 bytes little-endian).
const INDEX_ON_BLOCKCHAIN_RANGE: core::ops::Range<usize> = 40..48;

/// Byte range of the OutputData section in a serialized `WalletOutput`:
/// output key (32) + key offset (32) + commitment mask (32) + amount (8).
const OUTPUT_DATA_RANGE: core::ops::Range<usize> = 48..152;

/// Parse stored `WalletOutput` bytes, overriding `index_on_blockchain`.
///
/// The scanner can't always learn the global RingCT index of an output, so
/// the send path re-resolves it from the daemon and splices it in here before
/// spending.
pub(crate) fn wallet_output_with_index_on_blockchain(
    output_bytes: &[u8],
    index_on_blockchain: u64,
) -> Result<WalletOutput, String> {
    if output_bytes.len() < OUTPUT_DATA_RANGE.end {
        return Err("stored output bytes are too short".to_string());
    }
    let mut bytes = output_bytes.to_vec();
    bytes[INDEX_ON_BLOCKCHAIN_RANGE].copy_from_slice(&index_on_blockchain.to_le_bytes());
    let output = WalletOutput::read(&mut bytes.as_slice())
        .map_err(|e| format!("Invalid stored output bytes: {}", e))?;
    if output.index_on_blockchain() != index_on_blockchain {
        return Err("stored output bytes did not accept the on-chain index".to_string());
    }
    Ok(output)
}

/// Assemble an `OutputWithDecoys` from stored `WalletOutput` bytes and a
/// locally selected decoy set.
pub(crate) fn output_with_decoys(
    output_bytes: &[u8],
    decoys: &Decoys,
) -> Result<OutputWithDecoys, String> {
    if output_bytes.len() < OUTPUT_DATA_RANGE.end {
        return Err("stored output bytes are too short".to_string());
    }
    let mut bytes = output_bytes[OUTPUT_DATA_RANGE].to_vec();
    decoys
        .write(&mut bytes)
        .map_err(|e| format!("Failed to serialize decoys: {}", e))?;
    OutputWithDecoys::read(&mut bytes.as_slice())
        .map_err(|e| format!("Failed to assemble output with decoys: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, Scalar};

    /// Hand-build a serialized `WalletOutput` and confirm the byte offsets
    /// used above against `WalletOutput::read`.
    fn sample_output_bytes() -> Vec<u8> {
        let key_offset = Scalar::from(5u64);
        let key = &Scalar::from(9u64) * ED25519_BASEPOINT_TABLE;
        let mask = Scalar::from(3u64);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x11; 32]); // AbsoluteId: transaction hash
        bytes.extend_from_slice(&1u64.to_le_bytes()); // index_in_transaction
        bytes.extend_from_slice(&7u64.to_le_bytes()); // index_on_blockchain
        bytes.extend_from_slice(key.compress().as_bytes()); // OutputData: key
        bytes.extend_from_slice(&key_offset.to_bytes()); // key offset
        bytes.extend_from_slice(&mask.to_bytes()); // commitment mask
        bytes.extend_from_slice(&1234u64.to_le_bytes()); // amount
        bytes.push(0); // Metadata: Timelock::None
        bytes.push(0); // no subaddress
        bytes.push(0); // no payment id
        bytes.push(0); // no arbitrary data
        bytes
    }

    #[test]
    fn offsets_match_wallet_output_read() {
        let bytes = sample_output_bytes();
        let output = WalletOutput::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(output.transaction(), [0x11; 32]);
        assert_eq!(output.index_in_transaction(), 1);
        assert_eq!(output.index_on_blockchain(), 7);
        assert_eq!(output.commitment().amount, 1234);
        assert_eq!(output.serialize(), bytes);
    }

    #[test]
    fn splices_index_on_blockchain() {
        let bytes = sample_output_bytes();
        let output = wallet_output_with_index_on_blockchain(&bytes, 424242).unwrap();
        assert_eq!(output.index_on_blockchain(), 424242);
        assert_eq!(output.index_in_transaction(), 1);
        assert_eq!(output.commitment().amount, 1234);
    }

    #[test]
    fn assembles_output_with_decoys() {
        let bytes = sample_output_bytes();
        let output = WalletOutput::read(&mut bytes.as_slice()).unwrap();

        let ring: Vec<_> = (0..16u64)
            .map(|i| {
                let point = monero_wallet::ed25519::CompressedPoint::from(
                    (&Scalar::from(100 + i) * ED25519_BASEPOINT_TABLE)
                        .compress()
                        .to_bytes(),
                )
                .decompress()
                .unwrap();
                [point, point]
            })
            .collect();
        let mut offsets = vec![3u64];
        offsets.extend(std::iter::repeat(1).take(15));
        let decoys = Decoys::new(offsets, 4, ring).unwrap();

        let with_decoys = output_with_decoys(&bytes, &decoys).unwrap();
        assert_eq!(with_decoys.key(), output.key());
        assert_eq!(
            with_decoys.commitment().commit(),
            output.commitment().commit()
        );
        let mut expected_decoys = Vec::new();
        decoys.write(&mut expected_decoys).unwrap();
        let mut actual_decoys = Vec::new();
        with_decoys.decoys().write(&mut actual_decoys).unwrap();
        assert_eq!(actual_decoys, expected_decoys);
    }

    #[test]
    fn rejects_truncated_bytes() {
        assert!(wallet_output_with_index_on_blockchain(&[0u8; 100], 1).is_err());
    }
}
