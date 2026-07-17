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
