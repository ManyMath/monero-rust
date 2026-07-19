//! Wallet data types ported from the vendored serai mirror.
//!
//! These preserve the mirror's wire formats bit-for-bit: the app's unsigned
//! transaction format, key-image export payloads, and wallet2 interop all
//! serialize through them, while the cryptography behind them moves to the
//! monero-oxide crates. Conversions into `monero-wallet` types live here too.

// Consumed by the send-path port over the next commits.
#![allow(dead_code)]

use std::io::{self, Read, Write};

use curve25519_dalek::{
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use monero_wallet::address::{MoneroAddress, SubaddressIndex};

// ---------------------------------------------------------------------------
// Serialization helpers (mirror serialize.rs subset)
// ---------------------------------------------------------------------------

pub(crate) fn write_byte<W: Write>(byte: &u8, w: &mut W) -> io::Result<()> {
    w.write_all(&[*byte])
}

pub(crate) fn write_varint<W: Write>(varint: &u64, w: &mut W) -> io::Result<()> {
    let mut varint = *varint;
    while {
        let mut b = u8::try_from(varint & u64::from(!(0u8) >> 1)).expect("7 bits fit a u8");
        varint >>= 7;
        if varint != 0 {
            b |= 1 << 7;
        }
        write_byte(&b, w)?;
        varint != 0
    } {}
    Ok(())
}

pub(crate) fn write_scalar<W: Write>(scalar: &Scalar, w: &mut W) -> io::Result<()> {
    w.write_all(&scalar.to_bytes())
}

pub(crate) fn write_point<W: Write>(point: &EdwardsPoint, w: &mut W) -> io::Result<()> {
    w.write_all(&point.compress().to_bytes())
}

pub(crate) fn write_vec<T, W: Write, F: Fn(&T, &mut W) -> io::Result<()>>(
    f: F,
    v: &[T],
    w: &mut W,
) -> io::Result<()> {
    write_varint(&(v.len() as u64), w)?;
    for item in v {
        f(item, w)?;
    }
    Ok(())
}

pub(crate) fn read_bytes<R: Read, const N: usize>(r: &mut R) -> io::Result<[u8; N]> {
    let mut res = [0; N];
    r.read_exact(&mut res)?;
    Ok(res)
}

pub(crate) fn read_byte<R: Read>(r: &mut R) -> io::Result<u8> {
    Ok(read_bytes::<_, 1>(r)?[0])
}

pub(crate) fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    read_bytes(r).map(u32::from_le_bytes)
}

pub(crate) fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> {
    read_bytes(r).map(u64::from_le_bytes)
}

pub(crate) fn read_varint<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut bits = 0;
    let mut res = 0;
    while {
        let b = read_byte(r)?;
        if (bits != 0) && (b == 0) {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "non-canonical varint",
            ))?;
        }
        if ((bits + 7) >= 64) && (b >= (1 << (64 - bits))) {
            Err(io::Error::new(io::ErrorKind::Other, "varint overflow"))?;
        }

        res += u64::from(b & (!(0u8) >> 1)) << bits;
        bits += 7;
        b & (1 << 7) != 0
    } {}
    Ok(res)
}

pub(crate) fn read_scalar<R: Read>(r: &mut R) -> io::Result<Scalar> {
    Option::from(Scalar::from_canonical_bytes(read_bytes(r)?))
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "unreduced scalar"))
}

pub(crate) fn read_point<R: Read>(r: &mut R) -> io::Result<EdwardsPoint> {
    let bytes = read_bytes(r)?;
    CompressedEdwardsY(bytes)
        .decompress()
        .filter(|point| point.compress().to_bytes() == bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "invalid point"))
}

pub(crate) fn read_raw_vec<R: Read, T, F: Fn(&mut R) -> io::Result<T>>(
    f: F,
    len: usize,
    r: &mut R,
) -> io::Result<Vec<T>> {
    let mut res = Vec::with_capacity(len.min(4096));
    for _ in 0..len {
        res.push(f(r)?);
    }
    Ok(res)
}

pub(crate) fn read_vec<R: Read, T, F: Fn(&mut R) -> io::Result<T>>(
    f: F,
    r: &mut R,
) -> io::Result<Vec<T>> {
    let len = read_varint(r)?
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "length overflow"))?;
    read_raw_vec(f, len, r)
}

// ---------------------------------------------------------------------------
// Protocol and fees
// ---------------------------------------------------------------------------

/// The Monero protocol version to build transactions for.
#[expect(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Zeroize)]
pub enum Protocol {
    v14,
    v16,
    Custom { ring_len: usize, bp_plus: bool },
}

impl Protocol {
    /// Amount of ring members under this protocol version.
    pub fn ring_len(&self) -> usize {
        match self {
            Protocol::v14 => 11,
            Protocol::v16 => 16,
            Protocol::Custom { ring_len, .. } => *ring_len,
        }
    }

    /// Whether Bulletproofs+ are in use.
    pub fn bp_plus(&self) -> bool {
        match self {
            Protocol::v14 => false,
            Protocol::v16 => true,
            Protocol::Custom { bp_plus, .. } => *bp_plus,
        }
    }

    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        match self {
            Protocol::v14 => w.write_all(&[0, 14]),
            Protocol::v16 => w.write_all(&[0, 16]),
            Protocol::Custom { ring_len, bp_plus } => {
                w.write_all(&[1, 0])?;
                w.write_all(
                    &u16::try_from(*ring_len)
                        .map_err(|_| {
                            io::Error::new(io::ErrorKind::Other, "ring length exceeds u16")
                        })?
                        .to_le_bytes(),
                )?;
                w.write_all(&[u8::from(*bp_plus)])
            }
        }
    }

    pub fn read<R: Read>(r: &mut R) -> io::Result<Protocol> {
        Ok(match read_byte(r)? {
            0 => match read_byte(r)? {
                14 => Protocol::v14,
                16 => Protocol::v16,
                _ => Err(io::Error::new(io::ErrorKind::Other, "unrecognized protocol"))?,
            },
            1 => match read_byte(r)? {
                0 => Protocol::Custom {
                    ring_len: read_bytes::<_, 2>(r).map(u16::from_le_bytes)?.into(),
                    bp_plus: match read_byte(r)? {
                        0 => false,
                        1 => true,
                        _ => Err(io::Error::new(io::ErrorKind::Other, "invalid bool"))?,
                    },
                },
                _ => Err(io::Error::new(io::ErrorKind::Other, "unrecognized protocol"))?,
            },
            _ => Err(io::Error::new(io::ErrorKind::Other, "unrecognized protocol"))?,
        })
    }

    /// The RingCT format transactions use under this protocol version.
    pub fn rct_type(&self) -> monero_oxide::ringct::RctType {
        if self.bp_plus() {
            monero_oxide::ringct::RctType::ClsagBulletproofPlus
        } else {
            monero_oxide::ringct::RctType::ClsagBulletproof
        }
    }
}

/// A fee rate: an amount per weight with a quantization mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fee {
    pub per_weight: u64,
    pub mask: u64,
}

impl Fee {
    pub fn calculate(&self, weight: usize) -> u64 {
        ((((self.per_weight * u64::try_from(weight).expect("weight exceeds u64")) - 1) /
            self.mask) +
            1) *
            self.mask
    }
}

// ---------------------------------------------------------------------------
// Received/spendable outputs (mirror wire format)
// ---------------------------------------------------------------------------

/// An absolute output ID: transaction hash and output index within it.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct AbsoluteId {
    pub tx: [u8; 32],
    pub o: u8,
}

impl AbsoluteId {
    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&self.tx)?;
        w.write_all(&[self.o])
    }

    pub fn read<R: Read>(r: &mut R) -> io::Result<AbsoluteId> {
        Ok(AbsoluteId {
            tx: read_bytes(r)?,
            o: read_byte(r)?,
        })
    }
}

/// A Pedersen commitment's opening.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct Commitment {
    pub mask: Scalar,
    pub amount: u64,
}

impl Commitment {
    pub fn new(mask: Scalar, amount: u64) -> Commitment {
        Commitment { mask, amount }
    }

    /// Compute the commitment `mask * G + amount * H`.
    pub fn calculate(&self) -> EdwardsPoint {
        let oxide = monero_oxide::ed25519::Commitment::new(
            monero_oxide::ed25519::Scalar::from(self.mask),
            self.amount,
        );
        oxide.commit().into()
    }

    pub(crate) fn to_oxide(&self) -> monero_oxide::ed25519::Commitment {
        monero_oxide::ed25519::Commitment::new(
            monero_oxide::ed25519::Scalar::from(self.mask),
            self.amount,
        )
    }
}

/// The data within an output, as necessary to spend the output.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct OutputData {
    pub key: EdwardsPoint,
    /// Absolute difference between the spend key and the key in this output.
    pub key_offset: Scalar,
    pub commitment: Commitment,
}

impl OutputData {
    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&self.key.compress().to_bytes())?;
        w.write_all(&self.key_offset.to_bytes())?;
        w.write_all(&self.commitment.mask.to_bytes())?;
        w.write_all(&self.commitment.amount.to_le_bytes())
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut serialized = Vec::with_capacity(32 + 32 + 32 + 8);
        self.write(&mut serialized)
            .expect("writing into a Vec never fails");
        serialized
    }

    pub fn read<R: Read>(r: &mut R) -> io::Result<OutputData> {
        Ok(OutputData {
            key: read_point(r)?,
            key_offset: read_scalar(r)?,
            commitment: Commitment::new(read_scalar(r)?, read_u64(r)?),
        })
    }
}

/// The metadata for an output.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct Metadata {
    /// The subaddress this output was sent to.
    pub subaddress: Option<SubaddressIndex>,
    /// The payment ID included with this output (zeroes when absent).
    pub payment_id: [u8; 8],
    /// Arbitrary data encoded in TX extra.
    pub arbitrary_data: Vec<Vec<u8>>,
}

impl Metadata {
    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        if let Some(subaddress) = self.subaddress {
            w.write_all(&[1])?;
            w.write_all(&subaddress.account().to_le_bytes())?;
            w.write_all(&subaddress.address().to_le_bytes())?;
        } else {
            w.write_all(&[0])?;
        }
        w.write_all(&self.payment_id)?;

        w.write_all(
            &u32::try_from(self.arbitrary_data.len())
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "too much arbitrary data"))?
                .to_le_bytes(),
        )?;
        for part in &self.arbitrary_data {
            w.write_all(&[u8::try_from(part.len()).map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "arbitrary data chunk too long")
            })?])?;
            w.write_all(part)?;
        }
        Ok(())
    }

    pub fn read<R: Read>(r: &mut R) -> io::Result<Metadata> {
        let subaddress = if read_byte(r)? == 1 {
            Some(
                SubaddressIndex::new(read_u32(r)?, read_u32(r)?).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Other, "invalid subaddress in metadata")
                })?,
            )
        } else {
            None
        };

        Ok(Metadata {
            subaddress,
            payment_id: read_bytes(r)?,
            arbitrary_data: {
                let mut data = vec![];
                for _ in 0..read_u32(r)? {
                    let len = read_byte(r)?;
                    data.push(read_raw_vec(read_byte, usize::from(len), r)?);
                }
                data
            },
        })
    }
}

/// A received output: its absolute ID, data, and metadata.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct ReceivedOutput {
    pub absolute: AbsoluteId,
    pub data: OutputData,
    pub metadata: Metadata,
}

impl ReceivedOutput {
    pub fn key(&self) -> EdwardsPoint {
        self.data.key
    }

    pub fn key_offset(&self) -> Scalar {
        self.data.key_offset
    }

    pub fn commitment(&self) -> Commitment {
        self.data.commitment.clone()
    }

    pub fn arbitrary_data(&self) -> &[Vec<u8>] {
        &self.metadata.arbitrary_data
    }

    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        self.absolute.write(w)?;
        self.data.write(w)?;
        self.metadata.write(w)
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut serialized = vec![];
        self.write(&mut serialized)
            .expect("writing into a Vec never fails");
        serialized
    }

    pub fn read<R: Read>(r: &mut R) -> io::Result<ReceivedOutput> {
        Ok(ReceivedOutput {
            absolute: AbsoluteId::read(r)?,
            data: OutputData::read(r)?,
            metadata: Metadata::read(r)?,
        })
    }
}

/// A spendable output: a received output and its global RingCT index.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct SpendableOutput {
    pub output: ReceivedOutput,
    pub global_index: u64,
}

impl SpendableOutput {
    /// Create a SpendableOutput directly from a ReceivedOutput and global
    /// index, without consulting a daemon.
    pub fn test_new(output: ReceivedOutput, global_index: u64) -> Self {
        SpendableOutput {
            output,
            global_index,
        }
    }

    pub fn key(&self) -> EdwardsPoint {
        self.output.key()
    }

    pub fn key_offset(&self) -> Scalar {
        self.output.key_offset()
    }

    pub fn commitment(&self) -> Commitment {
        self.output.commitment()
    }

    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        self.output.write(w)?;
        w.write_all(&self.global_index.to_le_bytes())
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut serialized = vec![];
        self.write(&mut serialized)
            .expect("writing into a Vec never fails");
        serialized
    }

    pub fn read<R: Read>(r: &mut R) -> io::Result<SpendableOutput> {
        Ok(SpendableOutput {
            output: ReceivedOutput::read(r)?,
            global_index: read_u64(r)?,
        })
    }
}

// ---------------------------------------------------------------------------
// Decoys (mirror wire format)
// ---------------------------------------------------------------------------

/// Decoy data, containing the actual ring member as well (at index `i`).
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct Decoys {
    pub i: u8,
    pub offsets: Vec<u64>,
    pub ring: Vec<[EdwardsPoint; 2]>,
}

impl Decoys {
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        write_byte(&self.i, w)?;
        write_varint(&(self.offsets.len() as u64), w)?;
        for offset in &self.offsets {
            write_varint(offset, w)?;
        }
        write_varint(&(self.ring.len() as u64), w)?;
        for pair in &self.ring {
            write_point(&pair[0], w)?;
            write_point(&pair[1], w)?;
        }
        Ok(())
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.write(&mut buf)
            .expect("writing into a Vec never fails");
        buf
    }

    pub fn read<R: Read>(r: &mut R) -> io::Result<Decoys> {
        let i = read_byte(r)?;
        let offsets_len: usize = read_varint(r)?
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "offsets length overflow"))?;
        if offsets_len > 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decoy offsets exceed limit",
            ));
        }
        let mut offsets = Vec::with_capacity(offsets_len);
        for _ in 0..offsets_len {
            offsets.push(read_varint(r)?);
        }
        let ring_len: usize = read_varint(r)?
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "ring length overflow"))?;
        if ring_len > 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decoy ring exceeds limit",
            ));
        }
        let mut ring = Vec::with_capacity(ring_len);
        for _ in 0..ring_len {
            ring.push([read_point(r)?, read_point(r)?]);
        }
        Ok(Decoys { i, offsets, ring })
    }

    /// Convert into the `monero-oxide` decoy representation.
    pub(crate) fn to_oxide(&self) -> Result<monero_oxide::ringct::clsag::Decoys, String> {
        let ring = self
            .ring
            .iter()
            .map(|pair| {
                let convert = |point: &EdwardsPoint| {
                    monero_wallet::ed25519::CompressedPoint::from(point.compress().to_bytes())
                        .decompress()
                        .ok_or_else(|| "ring member failed to decompress".to_string())
                };
                Ok([convert(&pair[0])?, convert(&pair[1])?])
            })
            .collect::<Result<Vec<_>, String>>()?;
        monero_oxide::ringct::clsag::Decoys::new(self.offsets.clone(), self.i, ring)
            .ok_or_else(|| "invalid decoy set".to_string())
    }

    /// Convert from the `monero-oxide` decoy representation.
    pub(crate) fn from_oxide(decoys: &monero_oxide::ringct::clsag::Decoys) -> Self {
        Decoys {
            i: decoys.signer_index(),
            offsets: decoys.offsets().to_vec(),
            ring: decoys
                .ring()
                .iter()
                .map(|pair| [pair[0].into(), pair[1].into()])
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Payments and change
// ---------------------------------------------------------------------------

/// Specification for a change output.
#[derive(Clone, PartialEq, Eq, Zeroize)]
pub struct Change {
    address: MoneroAddress,
    view: Option<Zeroizing<Scalar>>,
}

impl std::fmt::Debug for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Change")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl Change {
    pub fn from_raw(address: MoneroAddress, view: Option<Zeroizing<Scalar>>) -> Change {
        Change { address, view }
    }

    pub fn address(&self) -> &MoneroAddress {
        &self.address
    }

    pub fn view(&self) -> Option<&Scalar> {
        self.view.as_deref()
    }
}

/// A payment within a transaction: either to a recipient or back to the
/// wallet as change.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub enum InternalPayment {
    Payment((MoneroAddress, u64)),
    Change(Change, u64),
}

impl InternalPayment {
    /// The destination address and amount of this payment.
    pub fn address_and_amount(&self) -> (&MoneroAddress, u64) {
        match self {
            InternalPayment::Payment((address, amount)) => (address, *amount),
            InternalPayment::Change(change, amount) => (change.address(), *amount),
        }
    }
}

/// Convert a scanned `monero-wallet` output into the compat model.
pub fn received_from_wallet_output(
    output: &monero_wallet::WalletOutput,
) -> Result<ReceivedOutput, String> {
    let output_index = u8::try_from(output.index_in_transaction())
        .map_err(|_| "Output index exceeds the supported range".to_string())?;
    let key_offset = Option::from(Scalar::from_canonical_bytes(<[u8; 32]>::from(
        output.key_offset(),
    )))
    .ok_or_else(|| "Invalid stored output key offset".to_string())?;
    let mask = Option::from(Scalar::from_canonical_bytes(<[u8; 32]>::from(
        output.commitment().mask,
    )))
    .ok_or_else(|| "Invalid stored output commitment mask".to_string())?;
    let payment_id = match output.payment_id() {
        Some(monero_wallet::extra::PaymentId::Encrypted(id)) => id,
        _ => [0u8; 8],
    };
    Ok(ReceivedOutput {
        absolute: AbsoluteId {
            tx: output.transaction(),
            o: output_index,
        },
        data: OutputData {
            key: output.key().into(),
            key_offset,
            commitment: Commitment::new(mask, output.commitment().amount),
        },
        metadata: Metadata {
            subaddress: output.subaddress(),
            payment_id,
            arbitrary_data: output.arbitrary_data().to_vec(),
        },
    })
}

impl SpendableOutput {
    /// Update the spendable output's global index from the daemon. This is
    /// intended to be called if a re-organization occurred.
    pub async fn refresh_global_index<R: crate::monero_rpc::RpcConnection>(
        &mut self,
        rpc: &crate::monero_rpc::Rpc<R>,
    ) -> Result<(), crate::monero_rpc::RpcError> {
        let indexes = rpc.get_o_indexes(self.output.absolute.tx).await?;
        self.global_index = indexes
            .get(usize::from(self.output.absolute.o))
            .copied()
            .ok_or(crate::monero_rpc::RpcError::InvalidNode)?;
        Ok(())
    }

    /// Create a spendable output by resolving the received output's global
    /// RingCT index from the daemon.
    pub async fn from<R: crate::monero_rpc::RpcConnection>(
        rpc: &crate::monero_rpc::Rpc<R>,
        output: ReceivedOutput,
    ) -> Result<SpendableOutput, crate::monero_rpc::RpcError> {
        let mut output = SpendableOutput {
            output,
            global_index: 0,
        };
        output.refresh_global_index(rpc).await?;
        Ok(output)
    }
}

/// A collection of received outputs, gated behind their timelocks.
///
/// Matches the previous backend's scanner result shape.
pub struct Timelocked(pub(crate) Vec<ReceivedOutput>);

impl Timelocked {
    /// Ignore the timelocks and return all outputs within this container.
    pub fn ignore_timelock(self) -> Vec<ReceivedOutput> {
        self.0
    }
}

/// A per-transaction scanner matching the previous backend's API.
///
/// `monero-wallet` only scans whole blocks; this wraps its scanner with the
/// synthetic-block machinery the production paths use for mempool scanning.
pub struct Scanner {
    inner: monero_wallet::Scanner,
}

impl Scanner {
    /// Create a Scanner from a ViewPair.
    ///
    /// The burning-bug filter set of the previous API is accepted and ignored;
    /// callers are responsible for output deduplication.
    pub fn from_view(
        pair: ViewPair,
        _burning_bug: Option<std::collections::HashSet<curve25519_dalek::edwards::CompressedEdwardsY>>,
    ) -> Scanner {
        let inner = pair
            .to_oxide()
            .map(monero_wallet::Scanner::new)
            .expect("creating a scanner from an invalid view pair");
        Scanner { inner }
    }

    /// Register a subaddress to scan for.
    pub fn register_subaddress(&mut self, subaddress: SubaddressIndex) {
        self.inner.register_subaddress(subaddress);
    }

    /// Scan a single transaction for outputs owned by this scanner's keys.
    pub fn scan_transaction(
        &mut self,
        tx: &monero_oxide::transaction::Transaction,
    ) -> Timelocked {
        let tx_hash = tx.hash();
        let pruned = monero_oxide::transaction::Transaction::<
            monero_oxide::transaction::Pruned,
        >::from(tx.clone());
        let outputs =
            crate::scanner::scan_single_transaction(&mut self.inner, tx_hash, &pruned)
                .unwrap_or_default();
        Timelocked(
            outputs
                .iter()
                .filter_map(|output| received_from_wallet_output(output).ok())
                .collect(),
        )
    }
}

// ---------------------------------------------------------------------------
// View pairs and addresses
// ---------------------------------------------------------------------------

/// Errors when building a transaction.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum TransactionError {
    #[error("multiple payment ids")]
    MultiplePaymentIds,
    #[error("no inputs")]
    NoInputs,
    #[error("no outputs")]
    NoOutputs,
    #[error("only one output and no change address")]
    NoChange,
    #[error("too much data")]
    TooMuchData,
    #[error("too many outputs")]
    TooManyOutputs,
    #[error("too large transaction")]
    TooLargeTransaction,
    #[error("not enough funds (in {0}, out {1})")]
    NotEnoughFunds(u64, u64),
    #[error("wrong private key")]
    WrongPrivateKey,
    #[error("invalid transaction ({0})")]
    InvalidTransaction(String),
    #[error("rpc error ({0})")]
    RpcError(String),
}

/// An address specification, matching the previous backend's model.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Zeroize)]
pub enum AddressSpec {
    Standard,
    Integrated([u8; 8]),
    Subaddress(SubaddressIndex),
    Featured {
        subaddress: Option<SubaddressIndex>,
        payment_id: Option<[u8; 8]>,
        guaranteed: bool,
    },
}

/// The pair of keys necessary to scan and receive: the public spend key and
/// the private view key.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ViewPair {
    spend: EdwardsPoint,
    view: Zeroizing<Scalar>,
}

impl ViewPair {
    pub fn new(spend: EdwardsPoint, view: Zeroizing<Scalar>) -> ViewPair {
        ViewPair { spend, view }
    }

    pub fn spend(&self) -> EdwardsPoint {
        self.spend
    }

    pub fn view_scalar(&self) -> &Zeroizing<Scalar> {
        &self.view
    }

    fn to_oxide(&self) -> Result<monero_wallet::ViewPair, String> {
        let spend = monero_wallet::ed25519::CompressedPoint::from(
            self.spend.compress().to_bytes(),
        )
        .decompress()
        .ok_or_else(|| "invalid spend key".to_string())?;
        monero_wallet::ViewPair::new(
            spend,
            Zeroizing::new(monero_wallet::ed25519::Scalar::from(**(&self.view))),
        )
        .map_err(|e| format!("invalid view pair: {}", e))
    }

    /// Derive an address of the given specification.
    pub fn address(
        &self,
        network: monero_wallet::address::Network,
        spec: AddressSpec,
    ) -> MoneroAddress {
        let pair = self
            .to_oxide()
            .expect("deriving an address from an invalid view pair");
        match spec {
            AddressSpec::Standard => pair.legacy_address(network),
            AddressSpec::Integrated(payment_id) => {
                pair.legacy_integrated_address(network, payment_id)
            }
            AddressSpec::Subaddress(index) => pair.subaddress(network, index),
            AddressSpec::Featured {
                subaddress,
                payment_id,
                guaranteed,
            } => {
                // Resolve the spend/view keys for the (sub)address, then
                // re-wrap them in a featured address
                let base = match subaddress {
                    Some(index) => pair.subaddress(network, index),
                    None => pair.legacy_address(network),
                };
                MoneroAddress::new(
                    network,
                    monero_wallet::address::AddressType::Featured {
                        subaddress: subaddress.is_some(),
                        payment_id,
                        guaranteed,
                    },
                    base.spend(),
                    base.view(),
                )
            }
        }
    }
}

impl Change {
    /// Create a change output specification from a ViewPair, as needed to
    /// maintain privacy.
    pub fn new(view: &ViewPair, guaranteed: bool) -> Change {
        Change {
            address: view.address(
                monero_wallet::address::Network::Mainnet,
                if !guaranteed {
                    AddressSpec::Standard
                } else {
                    AddressSpec::Featured {
                        subaddress: None,
                        payment_id: None,
                        guaranteed: true,
                    }
                },
            ),
            view: Some(view.view.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// Fee weight estimation (mirror arithmetic)
// ---------------------------------------------------------------------------

const MAX_ARBITRARY_DATA_SIZE: usize = 255 - 1;
const MAX_OUTPUTS: usize = 16;
const BP_LOG_N: usize = 6;

fn varint_len(varint: usize) -> usize {
    ((usize::try_from(usize::BITS - varint.leading_zeros())
        .expect("bit count exceeds usize")
        .saturating_sub(1)) /
        7) +
        1
}

fn input_fee_weight(ring_len: usize) -> usize {
    // 1 byte VarInt amount (0), 1 byte input type, 1 byte ring length
    1 + 1 + 1 + (8 * ring_len) + 32
}

fn output_fee_weight() -> usize {
    1 + 1 + 32 + 1
}

fn prefix_fee_weight(ring_len: usize, inputs: usize, outputs: usize, extra: usize) -> usize {
    // Assumes Timelock::None
    1 + 1 +
        varint_len(inputs) +
        (inputs * input_fee_weight(ring_len)) +
        1 +
        (outputs * output_fee_weight()) +
        varint_len(extra) +
        extra
}

fn clsag_fee_weight(ring_len: usize) -> usize {
    (ring_len * 32) + 32 + 32
}

fn bulletproofs_fee_weight(plus: bool, outputs: usize) -> usize {
    let fields = if plus { 6 } else { 9 };

    #[allow(non_snake_case)]
    let mut LR_len = usize::try_from(usize::BITS - (outputs - 1).leading_zeros())
        .expect("bit count exceeds usize");
    let padded_outputs = 1 << LR_len;
    LR_len += BP_LOG_N;

    let len = (fields + (2 * LR_len)) * 32;
    len +
        if padded_outputs <= 2 {
            0
        } else {
            let base = ((fields + (2 * (BP_LOG_N + 1))) * 32) / 2;
            let size = (fields + (2 * LR_len)) * 32;
            ((base * padded_outputs) - size) * 4 / 5
        }
}

fn rct_base_fee_weight(outputs: usize) -> usize {
    1 + 8 + (outputs * (8 + 32))
}

fn extra_fee_weight(outputs: usize, additional: bool, payment_id: bool, data: &[Vec<u8>]) -> usize {
    // PublicKey, key
    (1 + 32) +
        // PublicKeys, length, additional keys
        (if additional { 1 + 1 + (outputs * 32) } else { 0 }) +
        // PaymentId (Nonce), length, encrypted, ID
        (if payment_id { 1 + 1 + 1 + 8 } else { 0 }) +
        // Nonce, length, ARBITRARY_DATA_MARKER, data
        data.iter().map(|v| 1 + varint_len(1 + v.len()) + 1 + v.len()).sum::<usize>()
}

/// Worst-case transaction weight for fee estimation, matching the previous
/// backend's arithmetic exactly.
pub fn transaction_fee_weight(
    protocol: Protocol,
    inputs: usize,
    outputs: usize,
    extra: usize,
) -> usize {
    prefix_fee_weight(protocol.ring_len(), inputs, outputs, extra) +
        rct_base_fee_weight(outputs) +
        1 +
        bulletproofs_fee_weight(protocol.bp_plus(), outputs) +
        (inputs * (clsag_fee_weight(protocol.ring_len()) + 32))
}

// ---------------------------------------------------------------------------
// Signable transactions (online side)
// ---------------------------------------------------------------------------

/// A transaction with all data necessary to prepare or sign it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SignableTransaction {
    protocol: Protocol,
    r_seed: Option<Zeroizing<[u8; 32]>>,
    inputs: Vec<SpendableOutput>,
    payments: Vec<InternalPayment>,
    data: Vec<Vec<u8>>,
    fee: u64,
}

impl SignableTransaction {
    /// Create a signable transaction, validating it and computing its fee.
    pub fn new(
        protocol: Protocol,
        r_seed: Option<Zeroizing<[u8; 32]>>,
        inputs: Vec<SpendableOutput>,
        mut payments: Vec<(MoneroAddress, u64)>,
        change_address: Option<Change>,
        data: Vec<Vec<u8>>,
        fee_rate: Fee,
    ) -> Result<SignableTransaction, TransactionError> {
        // Make sure there's only one payment ID
        let mut has_payment_id = {
            let mut payment_ids = 0;
            let mut count = |addr: &MoneroAddress| {
                if addr.payment_id().is_some() {
                    payment_ids += 1
                }
            };
            for payment in &payments {
                count(&payment.0);
            }
            if let Some(change) = change_address.as_ref() {
                count(change.address());
            }
            if payment_ids > 1 {
                Err(TransactionError::MultiplePaymentIds)?;
            }
            payment_ids == 1
        };

        if inputs.is_empty() {
            Err(TransactionError::NoInputs)?;
        }
        if payments.is_empty() {
            Err(TransactionError::NoOutputs)?;
        }

        for part in &data {
            if part.len() > MAX_ARBITRARY_DATA_SIZE {
                Err(TransactionError::TooMuchData)?;
            }
        }

        // If we don't have two outputs, as required by Monero, error
        if (payments.len() == 1) && change_address.is_none() {
            Err(TransactionError::NoChange)?;
        }
        let outputs = payments.len() + usize::from(change_address.is_some());
        // A dummy payment ID is added if there's only 2 outputs
        has_payment_id |= outputs == 2;

        // Calculate the extra length, assuming additional keys are needed for
        // a worst-case estimation
        let extra = extra_fee_weight(outputs, true, has_payment_id, data.as_ref());

        // https://github.com/monero-project/monero/pull/8733
        const MAX_EXTRA_SIZE: usize = 1060;
        if extra > MAX_EXTRA_SIZE {
            Err(TransactionError::TooMuchData)?;
        }

        let estimated_tx_size = transaction_fee_weight(protocol, inputs.len(), outputs, extra);

        // wallet2 will only create transactions up to 100k bytes
        const MAX_TX_SIZE: usize = 100_000;
        if estimated_tx_size >= MAX_TX_SIZE {
            Err(TransactionError::TooLargeTransaction)?;
        }

        // Calculate the minimum fee. Omitting change may increase the actual fee.
        let mut fee = fee_rate.calculate(estimated_tx_size);

        // Make sure we have enough funds
        let in_amount = inputs
            .iter()
            .map(|input| input.commitment().amount)
            .sum::<u64>();
        let out_amount = payments.iter().map(|payment| payment.1).sum::<u64>() + fee;
        if in_amount < out_amount {
            Err(TransactionError::NotEnoughFunds(in_amount, out_amount))?;
        }

        if outputs > MAX_OUTPUTS {
            Err(TransactionError::TooManyOutputs)?;
        }

        let mut payments = payments
            .drain(..)
            .map(InternalPayment::Payment)
            .collect::<Vec<_>>();
        if let Some(change) = change_address {
            payments.push(InternalPayment::Change(change, in_amount - out_amount));
        } else {
            // Intentionally omitted change becomes miner fee. Keep the recorded
            // fee balanced with the actual inputs and fixed recipient amounts.
            // This cannot overflow: out_amount includes fee and is <= in_amount.
            fee += in_amount - out_amount;
        }

        Ok(SignableTransaction {
            protocol,
            r_seed,
            inputs,
            payments,
            data,
            fee,
        })
    }

    pub fn fee(&self) -> u64 {
        self.fee
    }

    /// Select decoys for every input and package everything into an
    /// `UnsignedTransaction` for offline signing.
    pub async fn prepare_unsigned<
        R: rand_core::RngCore + rand_core::CryptoRng,
        C: crate::monero_backend::rpc::RpcConnection,
    >(
        self,
        rng: &mut R,
        rpc: &crate::monero_backend::rpc::Rpc<C>,
    ) -> Result<UnsignedTransaction, TransactionError> {
        let height = rpc
            .get_height()
            .await
            .map_err(|e| TransactionError::RpcError(format!("{:?}", e)))?;

        let ring_len = u8::try_from(self.protocol.ring_len())
            .map_err(|_| TransactionError::InvalidTransaction("ring too large".to_string()))?;

        let mut inputs = Vec::with_capacity(self.inputs.len());
        for output in &self.inputs {
            let spent = crate::decoy_select::SpentOutput {
                index_on_blockchain: output.global_index,
                key: output.key(),
                commitment: output.commitment().calculate(),
            };
            let decoys = crate::decoy_select::select_decoys(
                rng,
                rpc,
                ring_len,
                height.saturating_sub(1),
                &spent,
            )
            .await
            .map_err(TransactionError::RpcError)?;
            inputs.push(UnsignedInput {
                output: output.clone(),
                decoys: Decoys::from_oxide(&decoys),
            });
        }

        let r_seed = match &self.r_seed {
            Some(seed) => seed.clone(),
            None => {
                let mut seed = Zeroizing::new([0; 32]);
                rng.fill_bytes(seed.as_mut());
                seed
            }
        };

        Ok(UnsignedTransaction {
            protocol: self.protocol,
            r_seed,
            fee: self.fee,
            payments: self.payments.clone(),
            data: self.data.clone(),
            inputs,
        })
    }

    /// Select decoys and sign this transaction with the private spend key.
    pub async fn sign<
        R: rand_core::RngCore + rand_core::CryptoRng,
        C: crate::monero_backend::rpc::RpcConnection,
    >(
        self,
        rng: &mut R,
        rpc: &crate::monero_backend::rpc::Rpc<C>,
        spend: &Zeroizing<Scalar>,
    ) -> Result<monero_oxide::transaction::Transaction, TransactionError> {
        let unsigned = self.prepare_unsigned(rng, rpc).await?;
        let (tx, _, _) = sign_offline(rng, spend, unsigned)
            .map_err(TransactionError::InvalidTransaction)?;
        Ok(tx)
    }
}

/// A builder for signable transactions, matching the previous backend's API.
#[derive(Clone, Debug)]
pub struct SignableTransactionBuilder {
    protocol: Protocol,
    fee: Fee,
    r_seed: Option<Zeroizing<[u8; 32]>>,
    inputs: Vec<SpendableOutput>,
    payments: Vec<(MoneroAddress, u64)>,
    change_address: Option<Change>,
    data: Vec<Vec<u8>>,
}

impl SignableTransactionBuilder {
    pub fn new(
        protocol: Protocol,
        fee: Fee,
        change_address: Option<Change>,
    ) -> SignableTransactionBuilder {
        SignableTransactionBuilder {
            protocol,
            fee,
            r_seed: None,
            inputs: vec![],
            payments: vec![],
            change_address,
            data: vec![],
        }
    }

    pub fn set_r_seed(&mut self, r_seed: Zeroizing<[u8; 32]>) -> &mut Self {
        self.r_seed = Some(r_seed);
        self
    }

    pub fn add_input(&mut self, input: SpendableOutput) -> &mut Self {
        self.inputs.push(input);
        self
    }

    pub fn add_inputs(&mut self, inputs: &[SpendableOutput]) -> &mut Self {
        self.inputs.extend(inputs.iter().cloned());
        self
    }

    pub fn add_payment(&mut self, dest: MoneroAddress, amount: u64) -> &mut Self {
        self.payments.push((dest, amount));
        self
    }

    pub fn add_payments(&mut self, payments: &[(MoneroAddress, u64)]) -> &mut Self {
        self.payments.extend(payments.iter().cloned());
        self
    }

    pub fn add_data(&mut self, data: Vec<u8>) -> &mut Self {
        self.data.push(data);
        self
    }

    pub fn build(self) -> Result<SignableTransaction, TransactionError> {
        SignableTransaction::new(
            self.protocol,
            self.r_seed,
            self.inputs,
            self.payments,
            self.change_address,
            self.data,
            self.fee,
        )
    }
}

// ---------------------------------------------------------------------------
// Unsigned transactions and offline signing
// ---------------------------------------------------------------------------

/// An input of an unsigned transaction: the output being spent plus the
/// decoys selected for its ring.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct UnsignedInput {
    pub output: SpendableOutput,
    pub decoys: Decoys,
}

/// A fully prepared transaction awaiting an offline signature.
///
/// The wire format matches the previous backend byte-for-byte: it's produced
/// by the online (view-only) side and consumed by cold-signing devices.
#[derive(Clone, PartialEq, Eq, Debug, Zeroize, ZeroizeOnDrop)]
pub struct UnsignedTransaction {
    pub protocol: Protocol,
    pub r_seed: Zeroizing<[u8; 32]>,
    pub fee: u64,
    pub payments: Vec<InternalPayment>,
    pub data: Vec<Vec<u8>>,
    pub inputs: Vec<UnsignedInput>,
}

impl UnsignedTransaction {
    pub fn write<W: Write>(&self, w: &mut W) -> io::Result<()> {
        self.protocol.write(w)?;
        w.write_all(self.r_seed.as_ref())?;
        w.write_all(&self.fee.to_le_bytes())?;

        fn write_payment<W: Write>(payment: &InternalPayment, w: &mut W) -> io::Result<()> {
            match payment {
                InternalPayment::Payment(payment) => {
                    w.write_all(&[0])?;
                    write_vec(write_byte, payment.0.to_string().as_bytes(), w)?;
                    w.write_all(&payment.1.to_le_bytes())
                }
                InternalPayment::Change(change, amount) => {
                    w.write_all(&[1])?;
                    write_vec(write_byte, change.address().to_string().as_bytes(), w)?;
                    if let Some(view) = change.view() {
                        w.write_all(&[1])?;
                        write_scalar(view, w)?;
                    } else {
                        w.write_all(&[0])?;
                    }
                    w.write_all(&amount.to_le_bytes())
                }
            }
        }
        write_vec(write_payment, &self.payments, w)?;

        write_varint(&(self.data.len() as u64), w)?;
        for chunk in &self.data {
            write_vec(write_byte, chunk, w)?;
        }

        write_varint(&(self.inputs.len() as u64), w)?;
        for input in &self.inputs {
            input.output.write(w)?;
            input.decoys.write(w)?;
        }

        Ok(())
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.write(&mut buf)
            .expect("writing into a Vec never fails");
        buf
    }

    pub fn read<R: Read>(r: &mut R) -> io::Result<UnsignedTransaction> {
        let protocol = Protocol::read(r)?;
        let r_seed = Zeroizing::new(read_bytes::<_, 32>(r)?);
        let fee = read_u64(r)?;

        fn read_payment<R: Read>(r: &mut R) -> io::Result<InternalPayment> {
            fn read_address<R: Read>(r: &mut R) -> io::Result<MoneroAddress> {
                String::from_utf8(read_vec(read_byte, r)?)
                    .ok()
                    .and_then(|str| MoneroAddress::from_str_with_unchecked_network(&str).ok())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "invalid address"))
            }

            Ok(match read_byte(r)? {
                0 => InternalPayment::Payment((read_address(r)?, read_u64(r)?)),
                1 => InternalPayment::Change(
                    Change::from_raw(
                        read_address(r)?,
                        match read_byte(r)? {
                            0 => None,
                            1 => Some(Zeroizing::new(read_scalar(r)?)),
                            _ => Err(io::Error::new(
                                io::ErrorKind::Other,
                                "invalid change payment",
                            ))?,
                        },
                    ),
                    read_u64(r)?,
                ),
                _ => Err(io::Error::new(io::ErrorKind::Other, "invalid payment"))?,
            })
        }

        let payments = read_vec(read_payment, r)?;

        let data_len: usize = read_varint(r)?
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "data length overflow"))?;
        if data_len > 1_000_000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "data length exceeds limit",
            ));
        }
        let mut data = Vec::with_capacity(data_len);
        for _ in 0..data_len {
            data.push(read_vec(read_byte, r)?);
        }

        let inputs_len: usize = read_varint(r)?
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "inputs length overflow"))?;
        if inputs_len > 1000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "inputs count exceeds limit",
            ));
        }
        let mut inputs = Vec::with_capacity(inputs_len);
        for _ in 0..inputs_len {
            inputs.push(UnsignedInput {
                output: SpendableOutput::read(r)?,
                decoys: Decoys::read(r)?,
            });
        }

        Ok(UnsignedTransaction {
            protocol,
            r_seed,
            fee,
            payments,
            data,
            inputs,
        })
    }
}

/// Monero's canonical input ordering: descending by key image bytes.
fn key_image_sort(x: &[u8; 32], y: &[u8; 32]) -> std::cmp::Ordering {
    x.cmp(y).reverse()
}

/// Sign a prepared `UnsignedTransaction` with the private spend key,
/// entirely offline.
///
/// Returns the signed transaction plus the transaction key (and additional
/// keys, when payments to subaddresses require them) for payment proofs.
pub fn sign_offline<R: rand_core::RngCore + rand_core::CryptoRng>(
    rng: &mut R,
    spend: &Zeroizing<Scalar>,
    unsigned: UnsignedTransaction,
) -> Result<
    (
        monero_oxide::transaction::Transaction,
        Zeroizing<Scalar>,
        Vec<Zeroizing<Scalar>>,
    ),
    String,
> {
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;

    if unsigned.inputs.is_empty() {
        return Err("unsigned transaction has no inputs".to_string());
    }

    // Verify the spend key owns every input before doing anything else
    for input in &unsigned.inputs {
        let offset = Zeroizing::new(**spend + input.output.key_offset());
        if (&*offset * ED25519_BASEPOINT_TABLE) != input.output.key() {
            return Err("wrong private key for unsigned transaction".to_string());
        }
    }

    // The unsigned transaction carries a fixed fee; monero-wallet derives the
    // fee as inputs minus outputs when no change output is specified, so the
    // amounts must be consistent for the fee to be honored exactly.
    let input_sum: u64 = unsigned
        .inputs
        .iter()
        .map(|input| input.output.commitment().amount)
        .sum();
    let payment_sum: u64 = unsigned
        .payments
        .iter()
        .map(|payment| payment.address_and_amount().1)
        .sum();
    if input_sum != payment_sum.checked_add(unsigned.fee).ok_or_else(|| {
        "unsigned transaction amounts overflow".to_string()
    })? {
        return Err("unsigned transaction fee doesn't match its amounts".to_string());
    }

    // Package each input with its pre-selected decoys
    let mut inputs = Vec::with_capacity(unsigned.inputs.len());
    for input in &unsigned.inputs {
        let mut bytes = input.output.output.data.serialize();
        input
            .decoys
            .to_oxide()?
            .write(&mut bytes)
            .map_err(|e| format!("Failed to serialize decoys: {}", e))?;
        inputs.push(
            monero_wallet::OutputWithDecoys::read(&mut bytes.as_slice())
                .map_err(|e| format!("Invalid unsigned transaction input: {}", e))?,
        );
    }

    // Express every output, change included, as a fixed payment so the
    // stored fee is used exactly (all unspent input value becomes the fee)
    let payments: Vec<(MoneroAddress, u64)> = unsigned
        .payments
        .iter()
        .map(|payment| {
            let (address, amount) = payment.address_and_amount();
            (address.clone(), amount)
        })
        .collect();

    // A floor fee rate: the fee was already decided by the online side; this
    // only has to keep the necessary fee at or below it
    let fee_rate = monero_wallet::interface::FeeRate::new(1, 1)
        .ok_or_else(|| "failed to construct fee rate".to_string())?;

    let signable = monero_wallet::send::SignableTransaction::new(
        unsigned.protocol.rct_type(),
        unsigned.r_seed.clone(),
        inputs,
        payments,
        monero_wallet::send::Change::fingerprintable(None),
        unsigned.data.clone(),
        fee_rate,
    )
    .map_err(|e| format!("Failed to build signable transaction: {}", e))?;

    // Re-derive the transaction keys the signer will embed, for payment
    // proofs. They're seeded from the outgoing view key and the inputs in
    // their final (key image sorted) order.
    let (tx_key, additional_keys) = {
        let mut keyed_inputs = unsigned
            .inputs
            .iter()
            .map(|input| {
                let offset = Zeroizing::new(**spend + input.output.key_offset());
                let key_image_point: EdwardsPoint = monero_wallet::ed25519::Point::biased_hash(
                    input.output.key().compress().to_bytes(),
                )
                .into();
                let key_image = (key_image_point * **(&offset)).compress().to_bytes();
                (key_image, input)
            })
            .collect::<Vec<_>>();
        keyed_inputs.sort_by(|(x, _), (y, _)| key_image_sort(x, y));

        let input_keys_and_commitments = keyed_inputs
            .iter()
            .map(|(_, input)| {
                let convert = |point: EdwardsPoint| {
                    monero_wallet::ed25519::CompressedPoint::from(point.compress().to_bytes())
                        .decompress()
                        .ok_or_else(|| "input key failed to decompress".to_string())
                };
                Ok((
                    convert(input.output.key())?,
                    convert(input.output.commitment().calculate())?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;

        let mut tx_keys = monero_wallet::send::TransactionKeys::new(
            &unsigned.r_seed,
            input_keys_and_commitments,
        );
        let tx_key = tx_keys
            .next()
            .expect("TransactionKeys (never-ending) was exhausted");

        // Additional keys are used iff any payment is to a subaddress (the
        // change carries no view key in this construction)
        let uses_additional_keys = unsigned
            .payments
            .iter()
            .any(|payment| payment.address_and_amount().0.is_subaddress());
        let mut additional_keys = vec![];
        if uses_additional_keys {
            for _ in 0..unsigned.payments.len() {
                additional_keys.push(
                    tx_keys
                        .next()
                        .expect("TransactionKeys (never-ending) was exhausted"),
                );
            }
        }
        (
            Zeroizing::new(Scalar::from_bytes_mod_order(<[u8; 32]>::from(*tx_key))),
            additional_keys
                .into_iter()
                .map(|key| Zeroizing::new(Scalar::from_bytes_mod_order(<[u8; 32]>::from(*key))))
                .collect::<Vec<_>>(),
        )
    };

    let spend_oxide = Zeroizing::new(monero_wallet::ed25519::Scalar::from(**spend));
    let tx = signable
        .sign(rng, &spend_oxide)
        .map_err(|e| format!("Failed to sign transaction: {}", e))?;

    Ok((tx, tx_key, additional_keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;

    fn sample_received_output() -> ReceivedOutput {
        ReceivedOutput {
            absolute: AbsoluteId { tx: [7; 32], o: 3 },
            data: OutputData {
                key: &Scalar::from(11u64) * ED25519_BASEPOINT_TABLE,
                key_offset: Scalar::from(13u64),
                commitment: Commitment::new(Scalar::from(17u64), 1_000_000),
            },
            metadata: Metadata {
                subaddress: SubaddressIndex::new(1, 2),
                payment_id: [9; 8],
                arbitrary_data: vec![vec![1, 2, 3]],
            },
        }
    }

    #[test]
    fn received_output_roundtrip() {
        let output = sample_received_output();
        let bytes = output.serialize();
        let read = ReceivedOutput::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(read, output);
    }

    #[test]
    fn spendable_output_roundtrip() {
        let spendable = SpendableOutput::test_new(sample_received_output(), 42424242);
        let bytes = spendable.serialize();
        let read = SpendableOutput::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(read, spendable);
    }

    #[test]
    fn output_data_matches_oxide_layout() {
        // The compat OutputData layout must equal monero-wallet's, so stored
        // outputs can feed `oxide_output_bytes::output_with_decoys`.
        let data = sample_received_output().data.clone();
        let bytes = data.serialize();
        assert_eq!(bytes.len(), 104);

        let mut decoy_ring = Vec::new();
        for i in 0..16u64 {
            let point = monero_wallet::ed25519::CompressedPoint::from(
                (&Scalar::from(500 + i) * ED25519_BASEPOINT_TABLE)
                    .compress()
                    .to_bytes(),
            )
            .decompress()
            .unwrap();
            decoy_ring.push([point, point]);
        }
        let mut offsets = vec![10u64];
        offsets.extend(std::iter::repeat(1).take(15));
        let oxide_decoys =
            monero_oxide::ringct::clsag::Decoys::new(offsets, 5, decoy_ring).unwrap();

        let mut with_decoys = bytes.clone();
        oxide_decoys.write(&mut with_decoys).unwrap();
        let assembled =
            monero_wallet::OutputWithDecoys::read(&mut with_decoys.as_slice()).unwrap();
        assert_eq!(
            assembled.key().compress().to_bytes(),
            data.key.compress().to_bytes()
        );
        assert_eq!(assembled.commitment().amount, 1_000_000);
    }

    #[test]
    fn decoys_roundtrip_and_oxide_conversion() {
        let ring: Vec<[EdwardsPoint; 2]> = (0..16u64)
            .map(|i| {
                let point = &Scalar::from(300 + i) * ED25519_BASEPOINT_TABLE;
                [point, point]
            })
            .collect();
        let mut offsets = vec![100u64];
        offsets.extend(std::iter::repeat(2).take(15));
        let decoys = Decoys {
            i: 7,
            offsets,
            ring,
        };

        let bytes = decoys.serialize();
        let read = Decoys::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(read, decoys);

        let oxide = decoys.to_oxide().unwrap();
        assert_eq!(oxide.signer_index(), 7);
        assert_eq!(oxide.offsets(), &decoys.offsets[..]);
        let back = Decoys::from_oxide(&oxide);
        assert_eq!(back, decoys);
    }

    #[test]
    fn builder_fee_matches_mirror_formula() {
        let spend = Zeroizing::new(Scalar::from(31u64));
        let key_offset = Scalar::from(7u64);
        let output = SpendableOutput::test_new(
            ReceivedOutput {
                absolute: AbsoluteId { tx: [1; 32], o: 0 },
                data: OutputData {
                    key: &(*spend + key_offset) * ED25519_BASEPOINT_TABLE,
                    key_offset,
                    commitment: Commitment::new(Scalar::from(3u64), 2_000_000_000),
                },
                metadata: Metadata {
                    subaddress: None,
                    payment_id: [0; 8],
                    arbitrary_data: vec![],
                },
            },
            5,
        );

        let convert = |point: EdwardsPoint| {
            monero_wallet::ed25519::CompressedPoint::from(point.compress().to_bytes())
                .decompress()
                .unwrap()
        };
        let dest = MoneroAddress::new(
            monero_wallet::address::Network::Mainnet,
            monero_wallet::address::AddressType::Legacy,
            convert(&Scalar::from(41u64) * ED25519_BASEPOINT_TABLE),
            convert(&Scalar::from(43u64) * ED25519_BASEPOINT_TABLE),
        );
        let view_pair = ViewPair::new(
            &Scalar::from(51u64) * ED25519_BASEPOINT_TABLE,
            Zeroizing::new(Scalar::from(53u64)),
        );

        let fee_rate = Fee {
            per_weight: 8000,
            mask: 10000,
        };
        let mut builder = SignableTransactionBuilder::new(
            Protocol::v16,
            fee_rate,
            Some(Change::new(&view_pair, false)),
        );
        builder.add_input(output);
        builder.add_payment(dest, 1_000_000_000);
        let signable = builder.build().unwrap();

        // Two outputs (payment + change); a dummy payment ID applies
        let extra = extra_fee_weight(2, true, true, &[]);
        let expected_fee =
            fee_rate.calculate(transaction_fee_weight(Protocol::v16, 1, 2, extra));
        assert_eq!(signable.fee(), expected_fee);
    }

    #[test]
    fn builder_rejects_single_payment_without_change() {
        let output = SpendableOutput::test_new(sample_received_output(), 5);
        let convert = |point: EdwardsPoint| {
            monero_wallet::ed25519::CompressedPoint::from(point.compress().to_bytes())
                .decompress()
                .unwrap()
        };
        let dest = MoneroAddress::new(
            monero_wallet::address::Network::Mainnet,
            monero_wallet::address::AddressType::Legacy,
            convert(&Scalar::from(41u64) * ED25519_BASEPOINT_TABLE),
            convert(&Scalar::from(43u64) * ED25519_BASEPOINT_TABLE),
        );
        let mut builder = SignableTransactionBuilder::new(
            Protocol::v16,
            Fee {
                per_weight: 8000,
                mask: 10000,
            },
            None,
        );
        builder.add_input(output);
        builder.add_payment(dest, 1);
        assert_eq!(builder.build().unwrap_err(), TransactionError::NoChange);
    }

    #[test]
    fn sign_offline_produces_valid_transaction() {
        use rand::SeedableRng;

        let mut rng = rand::rngs::StdRng::seed_from_u64(12345);
        let spend = Zeroizing::new(Scalar::from(987654321u64));
        let key_offset = Scalar::from(1122334455u64);
        let one_time_key = &(*spend + key_offset) * ED25519_BASEPOINT_TABLE;
        let mask = Scalar::from(31337u64);
        let amount_in = 2_000_000_000u64;
        let commitment = Commitment::new(mask, amount_in);
        let commitment_point = commitment.calculate();

        let signer_index = 5u8;
        let ring: Vec<[EdwardsPoint; 2]> = (0..16u64)
            .map(|i| {
                if i == u64::from(signer_index) {
                    [one_time_key, commitment_point]
                } else {
                    let point = &Scalar::from(9000 + i) * ED25519_BASEPOINT_TABLE;
                    [point, point]
                }
            })
            .collect();
        let mut offsets = vec![1000u64];
        offsets.extend(std::iter::repeat(3).take(15));
        let decoys = Decoys {
            i: signer_index,
            offsets: offsets.clone(),
            ring,
        };

        let output = SpendableOutput::test_new(
            ReceivedOutput {
                absolute: AbsoluteId { tx: [5; 32], o: 0 },
                data: OutputData {
                    key: one_time_key,
                    key_offset,
                    commitment,
                },
                metadata: Metadata {
                    subaddress: None,
                    payment_id: [0; 8],
                    arbitrary_data: vec![],
                },
            },
            1000 + 3 * 5,
        );

        let recipient_spend = &Scalar::from(777u64) * ED25519_BASEPOINT_TABLE;
        let recipient_view = &Scalar::from(888u64) * ED25519_BASEPOINT_TABLE;
        let convert = |point: EdwardsPoint| {
            monero_wallet::ed25519::CompressedPoint::from(point.compress().to_bytes())
                .decompress()
                .unwrap()
        };
        let recipient = MoneroAddress::new(
            monero_wallet::address::Network::Mainnet,
            monero_wallet::address::AddressType::Legacy,
            convert(recipient_spend),
            convert(recipient_view),
        );
        let change_addr = MoneroAddress::new(
            monero_wallet::address::Network::Mainnet,
            monero_wallet::address::AddressType::Legacy,
            convert(&Scalar::from(111u64) * ED25519_BASEPOINT_TABLE),
            convert(&Scalar::from(222u64) * ED25519_BASEPOINT_TABLE),
        );

        let pay = 1_000_000_000u64;
        let change = 900_000_000u64;
        let fee = amount_in - pay - change;
        let unsigned = UnsignedTransaction {
            protocol: Protocol::v16,
            r_seed: Zeroizing::new([42; 32]),
            fee,
            payments: vec![
                InternalPayment::Payment((recipient, pay)),
                InternalPayment::Change(Change::from_raw(change_addr, None), change),
            ],
            data: vec![],
            inputs: vec![UnsignedInput {
                output,
                decoys,
            }],
        };

        // The wire format roundtrips
        let bytes = unsigned.serialize();
        assert_eq!(
            UnsignedTransaction::read(&mut bytes.as_slice()).unwrap(),
            unsigned
        );

        let (tx, tx_key, additional_keys) =
            sign_offline(&mut rng, &spend, unsigned.clone()).unwrap();
        assert!(additional_keys.is_empty());

        // Input structure: our ring offsets, our key image
        assert_eq!(tx.version(), 2);
        let inputs = &tx.prefix().inputs;
        assert_eq!(inputs.len(), 1);
        let monero_oxide::transaction::Input::ToKey {
            key_offsets,
            key_image,
            ..
        } = &inputs[0]
        else {
            panic!("input wasn't ToKey");
        };
        assert_eq!(key_offsets, &offsets);
        let expected_key_image: EdwardsPoint = monero_wallet::ed25519::Point::biased_hash(
            one_time_key.compress().to_bytes(),
        )
        .into();
        let expected_key_image = expected_key_image * (*spend + key_offset);
        assert_eq!(
            key_image.to_bytes(),
            expected_key_image.compress().to_bytes()
        );

        // Two outputs (payment + change-as-payment), exact fee honored
        assert_eq!(tx.prefix().outputs.len(), 2);
        let monero_oxide::transaction::Transaction::V2 {
            proofs: Some(proofs),
            ..
        } = &tx
        else {
            panic!("transaction wasn't RingCT");
        };
        assert_eq!(proofs.base.fee, fee);

        // The returned tx key matches the pubkey embedded in extra
        let extra =
            monero_wallet::extra::Extra::read(&mut tx.prefix().extra.as_slice()).unwrap();
        let (tx_pubkeys, _additional) = extra.keys().unwrap();
        let expected_pubkey = (&*tx_key * ED25519_BASEPOINT_TABLE).compress().to_bytes();
        assert_eq!(tx_pubkeys[0].compress().to_bytes(), expected_pubkey);

        // Deterministic given the same seed material
        let mut rng2 = rand::rngs::StdRng::seed_from_u64(999);
        let (tx2, tx_key2, _) = sign_offline(&mut rng2, &spend, unsigned).unwrap();
        assert_eq!(tx_key2, tx_key);
        assert_eq!(tx2.prefix().extra, tx.prefix().extra);
    }

    #[test]
    fn no_change_builder_records_residual_as_the_signed_fee() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(987);
        let spend = Zeroizing::new(Scalar::from(31u64));
        let view = ViewPair::new(
            &*spend * ED25519_BASEPOINT_TABLE,
            Zeroizing::new(Scalar::from(53u64)),
        );
        let mut received = sample_received_output();
        received.data.key = &(*spend + received.data.key_offset) * ED25519_BASEPOINT_TABLE;
        received.data.commitment.amount = 2_000_000_000;
        let output = SpendableOutput::test_new(received, 1000);
        let ring = (0..16u64)
            .map(|i| {
                if i == 0 {
                    [output.key(), output.commitment().calculate()]
                } else {
                    let point = &Scalar::from(9000 + i) * ED25519_BASEPOINT_TABLE;
                    [point, point]
                }
            })
            .collect();
        let mut offsets = vec![1000];
        offsets.extend(std::iter::repeat(3).take(15));
        let recipients = vec![
            (
                view.address(
                    monero_wallet::address::Network::Mainnet,
                    AddressSpec::Standard,
                ),
                1_000_000_000,
            ),
            (
                view.address(
                    monero_wallet::address::Network::Mainnet,
                    AddressSpec::Subaddress(SubaddressIndex::new(0, 1).unwrap()),
                ),
                999_000_000,
            ),
        ];
        let signable = SignableTransaction::new(
            Protocol::v16,
            Some(Zeroizing::new([42; 32])),
            vec![output],
            recipients,
            None,
            vec![],
            Fee {
                per_weight: 1,
                mask: 1,
            },
        )
        .unwrap();
        assert_eq!(signable.fee(), 1_000_000);
        assert!(signable
            .payments
            .iter()
            .all(|p| matches!(p, InternalPayment::Payment(_))));
        let unsigned = UnsignedTransaction {
            protocol: signable.protocol,
            r_seed: signable.r_seed.unwrap(),
            fee: signable.fee,
            payments: signable.payments,
            data: signable.data,
            inputs: vec![UnsignedInput {
                output: signable.inputs[0].clone(),
                decoys: Decoys {
                    i: 0,
                    offsets,
                    ring,
                },
            }],
        };
        let bytes = unsigned.serialize();
        let unsigned = UnsignedTransaction::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(unsigned.fee, 1_000_000);
        let (tx, _, _) = sign_offline(&mut rng, &spend, unsigned).unwrap();
        let monero_oxide::transaction::Transaction::V2 {
            proofs: Some(proofs),
            ..
        } = tx
        else {
            panic!("expected RingCT transaction")
        };
        assert_eq!(proofs.base.fee, 1_000_000);
    }

    #[test]
    fn sign_offline_rejects_wrong_key_and_fee() {
        use rand::SeedableRng;

        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let spend = Zeroizing::new(Scalar::from(4u64));
        let output = SpendableOutput::test_new(sample_received_output(), 9);
        let decoys = Decoys {
            i: 0,
            offsets: vec![9],
            ring: vec![[output.key(), output.commitment().calculate()]],
        };
        let unsigned = UnsignedTransaction {
            protocol: Protocol::v16,
            r_seed: Zeroizing::new([1; 32]),
            fee: 1,
            payments: vec![],
            data: vec![],
            inputs: vec![UnsignedInput {
                output,
                decoys,
            }],
        };
        // sample_received_output isn't owned by `spend`
        assert!(sign_offline(&mut rng, &spend, unsigned)
            .unwrap_err()
            .contains("wrong private key"));
    }

    #[test]
    fn protocol_roundtrip() {
        for protocol in [
            Protocol::v14,
            Protocol::v16,
            Protocol::Custom {
                ring_len: 16,
                bp_plus: true,
            },
        ] {
            let mut bytes = Vec::new();
            protocol.write(&mut bytes).unwrap();
            assert_eq!(Protocol::read(&mut bytes.as_slice()).unwrap(), protocol);
        }
        assert_eq!(Protocol::v16.ring_len(), 16);
        assert!(Protocol::v16.bp_plus());
    }

    #[test]
    fn fee_calculation_matches_mirror() {
        let fee = Fee {
            per_weight: 8000,
            mask: 10000,
        };
        // Matches serai's quantized fee formula.
        assert_eq!(fee.calculate(1500), ((8000u64 * 1500 - 1) / 10000 + 1) * 10000);
    }

    #[test]
    fn commitment_calculate_matches_oxide() {
        let commitment = Commitment::new(Scalar::from(23u64), 777);
        let expected: EdwardsPoint = monero_oxide::ed25519::Commitment::new(
            monero_oxide::ed25519::Scalar::from(Scalar::from(23u64)),
            777,
        )
        .commit()
        .into();
        assert_eq!(commitment.calculate(), expected);
    }
}
