//! Experimental adapter spike for the live `monero-oxide` core crate.
//!
//! This module is intentionally feature-gated. It proves the current toolchain
//! can compile and exercise read-only transaction/block parsing through the live
//! split `monero-oxide` workspace without replacing the production backend yet.

use std::io::{self, Cursor};

use monero_oxide::{
    block::Block,
    transaction::{Input, Timelock, Transaction},
};
#[cfg(feature = "oxide-wallet-adapter-spike")]
use monero_oxide::transaction::Pruned;

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

/// Build a `monero-wallet` scanner and scan a block with the supplied view keys.
#[cfg(feature = "oxide-wallet-adapter-spike")]
pub fn scan_block_with_wallet(
    public_spend_key: [u8; 32],
    private_view_key: [u8; 32],
    network: OxideNetwork,
    block_bytes: &[u8],
    pruned_transaction_bytes: &[&[u8]],
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

    let mut transactions = Vec::with_capacity(pruned_transaction_bytes.len());
    for bytes in pruned_transaction_bytes {
        let mut cursor = std::io::Cursor::new(*bytes);
        let transaction = Transaction::<Pruned>::read(&mut cursor)?;
        ensure_fully_consumed(&cursor, bytes.len())?;
        transactions.push(transaction);
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

    Ok(OxideWalletScanSummary {
        legacy_address,
        registered_subaddresses: subaddresses
            .iter()
            .filter(|(account, address)| SubaddressIndex::new(*account, *address).is_some())
            .count(),
        block_height,
        scanned_output_count: outputs.len(),
    })
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
