//! Experimental adapter spike for the live `monero-oxide` core crate.
//!
//! This module is intentionally feature-gated. It proves the current toolchain
//! can compile and exercise read-only transaction/block parsing through the live
//! split `monero-oxide` workspace without replacing the production backend yet.

use std::io::{self, Cursor};

#[cfg(feature = "oxide-wallet-adapter-spike")]
use crate::monero_backend::rpc::{BlockCompleteEntry, BlockOutputIndices};
#[cfg(feature = "oxide-wallet-adapter-spike")]
use monero_oxide::transaction::{NotPruned, Pruned};
use monero_oxide::{
    block::Block,
    transaction::{Input, Timelock, Transaction},
};

#[cfg(feature = "oxide-wallet-adapter-spike")]
use monero_wallet::{
    address::{Network, SubaddressIndex},
    ed25519::{CompressedPoint, Scalar},
    interface::ScannableBlock,
    Scanner, ViewPair,
};
#[cfg(feature = "oxide-wallet-adapter-spike")]
use zeroize::Zeroizing;

/// Error returned by the experimental `monero-oxide` parsing adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OxideAdapterError {
    /// The upstream parser rejected the provided blob.
    Parse(String),
    /// The parser accepted a prefix but did not consume the full blob.
    TrailingBytes {
        /// Number of bytes consumed by the parser.
        consumed: usize,
        /// Total input length.
        total: usize,
    },
}

impl From<io::Error> for OxideAdapterError {
    fn from(error: io::Error) -> Self {
        Self::Parse(error.to_string())
    }
}

/// Monero transaction timelock summary from the `monero-oxide` parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OxideTimelockSummary {
    /// No additional timelock.
    None,
    /// Locked until a block height.
    Block(usize),
    /// Locked until a Unix timestamp.
    Time(u64),
}

/// Read-only summary of a transaction parsed by `monero-oxide`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OxideTransactionSummary {
    /// Transaction version.
    pub version: u8,
    /// Transaction hash reported by `monero-oxide`.
    pub hash: [u8; 32],
    /// Original serialized byte length.
    pub serialized_len: usize,
    /// Number of inputs in the transaction prefix.
    pub input_count: usize,
    /// Number of outputs in the transaction prefix.
    pub output_count: usize,
    /// Length of the transaction extra field.
    pub extra_len: usize,
    /// Sum of ring members across non-miner inputs.
    pub ring_member_count: usize,
    /// Miner input height when this is a miner transaction.
    pub miner_input_height: Option<usize>,
    /// Additional transaction timelock.
    pub timelock: OxideTimelockSummary,
}

/// Read-only summary of a block parsed by `monero-oxide`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OxideBlockSummary {
    /// Block height from the miner transaction.
    pub height: usize,
    /// Block hash reported by `monero-oxide`.
    pub hash: [u8; 32],
    /// Miner transaction hash reported by `monero-oxide`.
    pub miner_tx_hash: [u8; 32],
    /// Number of non-miner transaction hashes in the block.
    pub transaction_count: usize,
    /// Original serialized byte length.
    pub serialized_len: usize,
}

/// Network selector for the experimental `monero-wallet` adapter.
#[cfg(feature = "oxide-wallet-adapter-spike")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OxideNetwork {
    /// Monero mainnet.
    Mainnet,
    /// Monero testnet.
    Testnet,
    /// Monero stagenet.
    Stagenet,
}

/// Read-only scanner result from the experimental `monero-wallet` adapter.
#[cfg(feature = "oxide-wallet-adapter-spike")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OxideWalletScanSummary {
    /// Legacy address derived from the provided view pair.
    pub legacy_address: String,
    /// Number of explicitly registered non-primary subaddresses.
    pub registered_subaddresses: usize,
    /// Block height from the parsed block.
    pub block_height: usize,
    /// Number of outputs returned by `monero-wallet` after timelock filtering.
    pub scanned_output_count: usize,
    /// Stable summaries of outputs returned by `monero-wallet`.
    pub outputs: Vec<OxideWalletOutputSummary>,
}

/// Stable read-only summary of a wallet output returned by `monero-wallet`.
#[cfg(feature = "oxide-wallet-adapter-spike")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OxideWalletOutputSummary {
    /// Hash of the transaction that created the output.
    pub transaction: [u8; 32],
    /// Output index within the transaction.
    pub index_in_transaction: u64,
    /// RingCT output index on the blockchain.
    pub index_on_blockchain: u64,
    /// Decrypted output amount.
    pub amount: u64,
    /// Subaddress account/address pair, if this output was sent to a registered subaddress.
    pub subaddress: Option<(u32, u32)>,
}

/// Parse a full transaction blob with `monero-oxide` and return a stable summary.
pub fn summarize_transaction(bytes: &[u8]) -> Result<OxideTransactionSummary, OxideAdapterError> {
    let mut cursor = Cursor::new(bytes);
    let transaction = Transaction::read(&mut cursor)?;
    ensure_fully_consumed(&cursor, bytes.len())?;

    let prefix = transaction.prefix();
    let (miner_input_height, ring_member_count) = prefix.inputs.iter().fold(
        (None, 0usize),
        |(miner_height, ring_members), input| match input {
            Input::Gen(height) => (Some(*height), ring_members),
            Input::ToKey { key_offsets, .. } => (miner_height, ring_members + key_offsets.len()),
        },
    );

    Ok(OxideTransactionSummary {
        version: transaction.version(),
        hash: transaction.hash(),
        serialized_len: bytes.len(),
        input_count: prefix.inputs.len(),
        output_count: prefix.outputs.len(),
        extra_len: prefix.extra.len(),
        ring_member_count,
        miner_input_height,
        timelock: summarize_timelock(prefix.additional_timelock),
    })
}

/// Parse a block blob with `monero-oxide` and return a stable summary.
pub fn summarize_block(bytes: &[u8]) -> Result<OxideBlockSummary, OxideAdapterError> {
    let mut cursor = Cursor::new(bytes);
    let block = Block::read(&mut cursor)?;
    ensure_fully_consumed(&cursor, bytes.len())?;

    Ok(OxideBlockSummary {
        height: block.number(),
        hash: block.hash(),
        miner_tx_hash: block.miner_transaction().hash(),
        transaction_count: block.transactions.len(),
        serialized_len: bytes.len(),
    })
}

/// Infer the first RingCT output index in a block from daemon transaction output indexes.
///
/// The `transaction_output_indices` slices must be in the same order as the
/// non-miner transaction hashes embedded in the block.
#[cfg(feature = "oxide-wallet-adapter-spike")]
pub fn infer_first_ringct_output_index(
    block_bytes: &[u8],
    transaction_output_indices: &[&[u64]],
) -> Result<Option<u64>, OxideAdapterError> {
    let mut block_cursor = Cursor::new(block_bytes);
    let block = Block::read(&mut block_cursor)?;
    ensure_fully_consumed(&block_cursor, block_bytes.len())?;

    if block.transactions.len() != transaction_output_indices.len() {
        return Err(OxideAdapterError::Parse(format!(
            "expected {} transaction output-index lists, got {}",
            block.transactions.len(),
            transaction_output_indices.len()
        )));
    }

    let mut seen_outputs_before_tx = if block.miner_transaction().version() == 2 {
        u64::try_from(block.miner_transaction().prefix().outputs.len())
            .expect("miner output count should fit in u64")
    } else {
        0
    };
    let mut first_ringct_output_index = None;

    for output_indices in transaction_output_indices {
        if let Some(first_output_index_in_tx) = output_indices.first().copied() {
            let inferred_first = match first_ringct_output_index {
                Some(first_ringct_output_index) => first_ringct_output_index,
                None => {
                    let inferred = first_output_index_in_tx
                        .checked_sub(seen_outputs_before_tx)
                        .ok_or_else(|| {
                            OxideAdapterError::Parse(
                                "transaction output index precedes prior block outputs".to_string(),
                            )
                        })?;
                    first_ringct_output_index = Some(inferred);
                    inferred
                }
            };

            let expected_first_output_index = inferred_first
                .checked_add(seen_outputs_before_tx)
                .ok_or_else(|| {
                    OxideAdapterError::Parse("RingCT output index overflow".to_string())
                })?;
            if first_output_index_in_tx != expected_first_output_index {
                return Err(OxideAdapterError::Parse(format!(
                    "expected first transaction output index {}, got {}",
                    expected_first_output_index, first_output_index_in_tx
                )));
            }

            for (offset, output_index) in output_indices.iter().copied().enumerate() {
                let expected_output_index = expected_first_output_index
                    .checked_add(u64::try_from(offset).expect("output offset should fit in u64"))
                    .ok_or_else(|| {
                        OxideAdapterError::Parse("RingCT output index overflow".to_string())
                    })?;
                if output_index != expected_output_index {
                    return Err(OxideAdapterError::Parse(format!(
                        "expected transaction output index {}, got {}",
                        expected_output_index, output_index
                    )));
                }
            }
        }

        seen_outputs_before_tx = seen_outputs_before_tx
            .checked_add(
                u64::try_from(output_indices.len()).expect("output count should fit in u64"),
            )
            .ok_or_else(|| OxideAdapterError::Parse("RingCT output index overflow".to_string()))?;
    }

    Ok(first_ringct_output_index)
}

/// Build a `monero-wallet` scanner and scan a block with the supplied view keys.
#[cfg(feature = "oxide-wallet-adapter-spike")]
pub fn scan_block_with_wallet(
    public_spend_key: [u8; 32],
    private_view_key: [u8; 32],
    network: OxideNetwork,
    block_bytes: &[u8],
    transaction_bytes: &[&[u8]],
    output_index_for_first_ringct_output: Option<u64>,
    subaddresses: &[(u32, u32)],
) -> Result<OxideWalletScanSummary, OxideAdapterError> {
    let spend = CompressedPoint::from(public_spend_key)
        .decompress()
        .ok_or_else(|| OxideAdapterError::Parse("invalid public spend key".to_string()))?;
    let view = Scalar::read(&mut private_view_key.as_slice())?;
    let view_pair = ViewPair::new(spend, Zeroizing::new(view))
        .map_err(|error| OxideAdapterError::Parse(error.to_string()))?;
    let legacy_address = view_pair
        .legacy_address(convert_network(network))
        .to_string();

    let mut scanner = Scanner::new(view_pair);
    for (account, address) in subaddresses {
        let Some(subaddress) = SubaddressIndex::new(*account, *address) else {
            continue;
        };
        scanner.register_subaddress(subaddress);
    }

    let mut block_cursor = std::io::Cursor::new(block_bytes);
    let block = Block::read(&mut block_cursor)?;
    ensure_fully_consumed(&block_cursor, block_bytes.len())?;
    let block_height = block.number();

    let mut transactions = Vec::with_capacity(transaction_bytes.len());
    for bytes in transaction_bytes {
        transactions.push(parse_scannable_transaction(bytes)?);
    }

    let scannable_block = ScannableBlock {
        block,
        transactions,
        output_index_for_first_ringct_output,
    };
    let outputs = scanner
        .scan(scannable_block)
        .map_err(|error| OxideAdapterError::Parse(error.to_string()))?
        .not_additionally_locked();
    let output_summaries = outputs
        .iter()
        .map(|output| OxideWalletOutputSummary {
            transaction: output.transaction(),
            index_in_transaction: output.index_in_transaction(),
            index_on_blockchain: output.index_on_blockchain(),
            amount: output.commitment().amount,
            subaddress: output
                .subaddress()
                .map(|subaddress| (subaddress.account(), subaddress.address())),
        })
        .collect::<Vec<_>>();

    Ok(OxideWalletScanSummary {
        legacy_address,
        registered_subaddresses: subaddresses
            .iter()
            .filter(|(account, address)| SubaddressIndex::new(*account, *address).is_some())
            .count(),
        block_height,
        scanned_output_count: outputs.len(),
        outputs: output_summaries,
    })
}

/// Expand one `/getblocks.bin` block entry into the `monero-wallet` scanner shape.
#[cfg(feature = "oxide-wallet-adapter-spike")]
pub fn scan_rpc_block_with_wallet(
    public_spend_key: [u8; 32],
    private_view_key: [u8; 32],
    network: OxideNetwork,
    block_entry: &BlockCompleteEntry,
    output_indices: Option<&BlockOutputIndices>,
    subaddresses: &[(u32, u32)],
) -> Result<OxideWalletScanSummary, OxideAdapterError> {
    let mut block_cursor = Cursor::new(block_entry.block.as_slice());
    let block = Block::read(&mut block_cursor)?;
    ensure_fully_consumed(&block_cursor, block_entry.block.len())?;
    if block.transactions.len() != block_entry.txs.len() {
        return Err(OxideAdapterError::Parse(format!(
            "expected {} expanded transaction blobs, got {}",
            block.transactions.len(),
            block_entry.txs.len()
        )));
    }

    let transaction_bytes = block_entry
        .txs
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    let output_index_for_first_ringct_output = if let Some(output_indices) = output_indices {
        let transaction_output_indices = output_indices
            .indices
            .iter()
            .map(|tx| tx.indices.as_slice())
            .collect::<Vec<_>>();
        infer_first_ringct_output_index(&block_entry.block, &transaction_output_indices)?
    } else {
        None
    };

    scan_block_with_wallet(
        public_spend_key,
        private_view_key,
        network,
        &block_entry.block,
        &transaction_bytes,
        output_index_for_first_ringct_output,
        subaddresses,
    )
}

/// Expand and scan a sequence of `/getblocks.bin` block entries.
///
/// When daemon output indices are supplied, they must be parallel to
/// `block_entries`. An empty output-index slice means no index metadata is
/// available for any block.
#[cfg(feature = "oxide-wallet-adapter-spike")]
pub fn scan_rpc_blocks_with_wallet(
    public_spend_key: [u8; 32],
    private_view_key: [u8; 32],
    network: OxideNetwork,
    block_entries: &[BlockCompleteEntry],
    output_indices: &[BlockOutputIndices],
    subaddresses: &[(u32, u32)],
) -> Result<Vec<OxideWalletScanSummary>, OxideAdapterError> {
    if !output_indices.is_empty() && output_indices.len() != block_entries.len() {
        return Err(OxideAdapterError::Parse(format!(
            "expected {} block output-index entries, got {}",
            block_entries.len(),
            output_indices.len()
        )));
    }

    block_entries
        .iter()
        .enumerate()
        .map(|(index, block_entry)| {
            scan_rpc_block_with_wallet(
                public_spend_key,
                private_view_key,
                network,
                block_entry,
                output_indices.get(index),
                subaddresses,
            )
        })
        .collect()
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn parse_scannable_transaction(bytes: &[u8]) -> Result<Transaction<Pruned>, OxideAdapterError> {
    let mut pruned_cursor = Cursor::new(bytes);
    match Transaction::<Pruned>::read(&mut pruned_cursor) {
        Ok(transaction) if pruned_cursor.position() as usize == bytes.len() => {
            return Ok(transaction);
        }
        Ok(_) | Err(_) => {}
    }

    let mut full_cursor = Cursor::new(bytes);
    let transaction = Transaction::<NotPruned>::read(&mut full_cursor)?;
    ensure_fully_consumed(&full_cursor, bytes.len())?;
    Ok(Transaction::<Pruned>::from(transaction))
}

fn summarize_timelock(timelock: Timelock) -> OxideTimelockSummary {
    match timelock {
        Timelock::None => OxideTimelockSummary::None,
        Timelock::Block(height) => OxideTimelockSummary::Block(height),
        Timelock::Time(timestamp) => OxideTimelockSummary::Time(timestamp),
    }
}

fn ensure_fully_consumed(cursor: &Cursor<&[u8]>, total: usize) -> Result<(), OxideAdapterError> {
    let consumed = cursor.position() as usize;
    if consumed == total {
        Ok(())
    } else {
        Err(OxideAdapterError::TrailingBytes { consumed, total })
    }
}

#[cfg(feature = "oxide-wallet-adapter-spike")]
fn convert_network(network: OxideNetwork) -> Network {
    match network {
        OxideNetwork::Mainnet => Network::Mainnet,
        OxideNetwork::Testnet => Network::Testnet,
        OxideNetwork::Stagenet => Network::Stagenet,
    }
}
