use std::io::{self, Read, Write};

use zeroize::Zeroize;

fn read_bytes<R: Read, const N: usize>(r: &mut R) -> io::Result<[u8; N]> {
    let mut res = [0; N];
    r.read_exact(&mut res)?;
    Ok(res)
}

fn read_byte<R: Read>(r: &mut R) -> io::Result<u8> {
    Ok(read_bytes::<_, 1>(r)?[0])
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
                _ => Err(io::Error::new(
                    io::ErrorKind::Other,
                    "unrecognized protocol",
                ))?,
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
                _ => Err(io::Error::new(
                    io::ErrorKind::Other,
                    "unrecognized protocol",
                ))?,
            },
            _ => Err(io::Error::new(
                io::ErrorKind::Other,
                "unrecognized protocol",
            ))?,
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
        ((((self.per_weight * u64::try_from(weight).expect("weight exceeds u64")) - 1) / self.mask)
            + 1)
            * self.mask
    }
}

// ---------------------------------------------------------------------------
// Fee weight estimation (mirror arithmetic)
// ---------------------------------------------------------------------------

const BP_LOG_N: usize = 6;

fn varint_len(varint: usize) -> usize {
    ((usize::try_from(usize::BITS - varint.leading_zeros())
        .expect("bit count exceeds usize")
        .saturating_sub(1))
        / 7)
        + 1
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
    1 + 1
        + varint_len(inputs)
        + (inputs * input_fee_weight(ring_len))
        + 1
        + (outputs * output_fee_weight())
        + varint_len(extra)
        + extra
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
    len + if padded_outputs <= 2 {
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

pub(crate) fn extra_fee_weight(
    outputs: usize,
    additional: bool,
    payment_id: bool,
    data: &[Vec<u8>],
) -> usize {
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
    prefix_fee_weight(protocol.ring_len(), inputs, outputs, extra)
        + rct_base_fee_weight(outputs)
        + 1
        + bulletproofs_fee_weight(protocol.bp_plus(), outputs)
        + (inputs * (clsag_fee_weight(protocol.ring_len()) + 32))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            fee.calculate(1500),
            ((8000u64 * 1500 - 1) / 10000 + 1) * 10000
        );
    }
}
