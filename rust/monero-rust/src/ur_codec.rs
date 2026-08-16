//! UR (Uniform Resource) animated QR code encoding/decoding.

use qrcode::types::EcLevel;

/// UR type identifiers matching Feather/XmrSigner conventions.
pub const UR_TYPE_UNSIGNED_TX: &str = "xmr-txunsigned";
pub const UR_TYPE_SIGNED_TX: &str = "xmr-txsigned";
pub const UR_TYPE_KEY_IMAGE: &str = "xmr-keyimage";
pub const UR_TYPE_OUTPUT: &str = "xmr-output";

/// A single QR code frame: the boolean module matrix plus metadata.
#[derive(Clone, Debug)]
pub struct QrFrame {
    /// Row-major boolean module matrix (true = dark).
    pub modules: Vec<bool>,
    /// Width (and height) of the QR code in modules.
    pub size: usize,
    /// The UR URI string for this frame (for debugging / text display).
    pub uri: String,
    /// Current sequence number (1-based).
    pub seq_num: usize,
    /// Total number of unique fragments.
    pub seq_len: usize,
}

/// Fountain-coded UR encoder that produces QR frames.
pub struct UrEncoder {
    inner: ur::Encoder<'static>,
    fragment_count: usize,
}

impl UrEncoder {
    /// Create a new encoder for the given data and UR type.
    pub fn new(
        data: &[u8],
        ur_type: &'static str,
        max_fragment_len: usize,
    ) -> Result<Self, String> {
        let inner = ur::Encoder::new(data, max_fragment_len, ur_type)
            .map_err(|e| format!("UR encoder error: {:?}", e))?;
        let fragment_count = inner.fragment_count();
        Ok(UrEncoder {
            inner,
            fragment_count,
        })
    }

    /// Generate the next QR frame.
    pub fn next_frame(&mut self) -> Result<QrFrame, String> {
        let raw_index = self.inner.current_index();
        let seq_num = (raw_index % self.fragment_count) + 1;
        let uri = self
            .inner
            .next_part()
            .map_err(|e| format!("UR next_part error: {:?}", e))?;

        let qr = qrcode::QrCode::with_error_correction_level(uri.as_bytes(), EcLevel::L)
            .map_err(|e| format!("QR encode error: {:?}", e))?;

        let size = qr.width() as usize;
        let modules: Vec<bool> = qr
            .into_colors()
            .into_iter()
            .map(|c| c == qrcode::types::Color::Dark)
            .collect();

        Ok(QrFrame {
            modules,
            size,
            uri,
            seq_num,
            seq_len: self.fragment_count,
        })
    }

    pub fn fragment_count(&self) -> usize {
        self.fragment_count
    }

    pub fn current_index(&self) -> usize {
        self.inner.current_index()
    }
}

/// Maximum fragment count accepted from a UR frame.
///
/// 8192 leaves room beyond the few hundred fragments typically needed for an
/// unsigned transaction while bounding the fountain decoder's allocations.
const MAX_SEQUENCE_COUNT: u32 = 8192;

/// Validate the fountain header before passing a frame to the UR decoder.
///
/// The decoder allocates arrays proportional to the CBOR `sequence_count`.
/// The count in the URI text is parsed separately and discarded, so checking
/// that text cannot bound the allocation.
///
/// Also reject sequence zero: `choose_fragments` subtracts one from the sequence
/// without checking for underflow.
fn check_declared_fragment_count(uri: &str) -> Result<(), String> {
    let Some(after_scheme) = uri.strip_prefix("ur:") else {
        // Let the UR decoder report unsupported URI syntax.
        return Ok(());
    };
    let Some((_ur_type, rest)) = after_scheme.split_once('/') else {
        return Ok(());
    };
    // Single-part URs have no sequence numbers or fountain header.
    let Some((_indices, payload)) = rest.rsplit_once('/') else {
        return Ok(());
    };

    let cbor = ur::bytewords::decode(payload, ur::bytewords::Style::Minimal)
        .map_err(|e| format!("UR decode error: {:?}", e))?;

    let (sequence, sequence_count) = read_fountain_header(&cbor)
        .ok_or_else(|| "UR frame header is not a fountain part".to_string())?;

    if sequence == 0 {
        return Err("UR frame declares sequence 0, which no encoder emits".to_string());
    }
    if sequence_count == 0 || sequence_count > MAX_SEQUENCE_COUNT {
        return Err(format!(
            "UR frame declares {sequence_count} fragments; refusing more than \
             {MAX_SEQUENCE_COUNT}"
        ));
    }
    Ok(())
}

/// Read `(sequence, sequence_count)` from a fountain part's CBOR header.
///
/// The header is `[sequence, sequence_count, message_length, checksum, data]`.
/// Only the first two unsigned integers are needed here. Return `None` for
/// unsupported encodings and let the UR decoder report the error.
fn read_fountain_header(cbor: &[u8]) -> Option<(u32, u32)> {
    let mut rest = cbor;
    // Expect a definite-length CBOR array with five elements.
    if *rest.first()? != 0x85 {
        return None;
    }
    rest = &rest[1..];
    let sequence = read_cbor_u32(&mut rest)?;
    let sequence_count = read_cbor_u32(&mut rest)?;
    Some((sequence, sequence_count))
}

/// Read a CBOR unsigned integer into `u32` and advance `rest`.
fn read_cbor_u32(rest: &mut &[u8]) -> Option<u32> {
    let initial = *rest.first()?;
    // Require CBOR major type 0: an unsigned integer.
    if initial >> 5 != 0 {
        return None;
    }
    let (value, consumed) = match initial & 0x1f {
        immediate @ 0..=23 => (u32::from(immediate), 1),
        24 => (u32::from(*rest.get(1)?), 2),
        25 => (
            u32::from(u16::from_be_bytes([*rest.get(1)?, *rest.get(2)?])),
            3,
        ),
        26 => (
            u32::from_be_bytes([*rest.get(1)?, *rest.get(2)?, *rest.get(3)?, *rest.get(4)?]),
            5,
        ),
        // The UR decoder also rejects 64-bit encodings for these u32 fields.
        _ => return None,
    };
    *rest = &rest[consumed..];
    Some(value)
}

/// Progress of UR decoding.
#[derive(Clone, Debug)]
pub struct DecodeProgress {
    pub progress: f64,
    pub is_complete: bool,
}

/// UR decoder that accumulates scanned QR frames.
pub struct UrDecoder {
    inner: ur::Decoder,
    parts_received: usize,
    expected_parts: Option<usize>,
}

impl UrDecoder {
    pub fn new() -> Self {
        UrDecoder {
            inner: ur::Decoder::default(),
            parts_received: 0,
            expected_parts: None,
        }
    }

    /// Feed a scanned UR URI string into the decoder.
    pub fn receive(&mut self, uri: &str) -> Result<DecodeProgress, String> {
        // Before the crate sees it: the declared fragment count drives allocations inside
        // the fountain decoder and nothing in the crate bounds it.
        check_declared_fragment_count(uri)?;

        match self.inner.receive(uri) {
            Ok(()) => {}
            Err(e) => {
                // Single-part URIs can't go through the multi-part decoder
                // (ur::Decoder::receive returns NotMultiPart).
                // Signal completion; caller should use decode_single().
                let err_msg = format!("{:?}", e);
                if err_msg.contains("NotMultiPart") {
                    return Ok(DecodeProgress {
                        progress: 1.0,
                        is_complete: true,
                    });
                }
                return Err(format!("UR decode error: {}", err_msg));
            }
        }
        self.parts_received += 1;

        if self.expected_parts.is_none() {
            if let Some(total) = parse_sequence_total(uri) {
                self.expected_parts = Some(total);
            }
        }

        let progress = match self.expected_parts {
            Some(total) if total > 0 => (self.parts_received as f64 / total as f64).min(1.0),
            _ => 0.0,
        };

        let is_complete = self.inner.complete();

        Ok(DecodeProgress {
            progress: if is_complete { 1.0 } else { progress },
            is_complete,
        })
    }

    /// Get the decoded message bytes (only valid after `is_complete` is true).
    pub fn message(&self) -> Result<Option<Vec<u8>>, String> {
        self.inner
            .message()
            .map_err(|e| format!("UR message error: {:?}", e))
    }

    pub fn is_complete(&self) -> bool {
        self.inner.complete()
    }

    pub fn reset(&mut self) {
        self.inner = ur::Decoder::default();
        self.parts_received = 0;
        self.expected_parts = None;
    }
}

impl Default for UrDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode a single-part UR string directly (no fountain coding).
pub fn decode_single(uri: &str) -> Result<Vec<u8>, String> {
    let (_kind, data) = ur::decode(uri).map_err(|e| format!("UR decode error: {:?}", e))?;
    Ok(data)
}

/// Parse the total sequence count from a multi-part UR URI.
/// Format: "ur:type/seq-total/payload"
fn parse_sequence_total(uri: &str) -> Option<usize> {
    let without_scheme = uri.strip_prefix("ur:")?;
    let after_type = without_scheme.split('/').nth(1)?;
    let total_str = after_type.split('-').nth(1)?;
    total_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_decoder_roundtrip() {
        let data = b"Hello, this is test data for UR encoding roundtrip verification!";

        let mut encoder =
            UrEncoder::new(data, UR_TYPE_UNSIGNED_TX, 20).expect("encoder creation failed");

        assert!(encoder.fragment_count() > 0);

        let mut decoder = UrDecoder::new();

        for _ in 0..encoder.fragment_count() * 3 {
            let frame = encoder.next_frame().expect("next_frame failed");
            assert!(frame.size > 0);
            assert!(!frame.modules.is_empty());
            assert_eq!(frame.modules.len(), frame.size * frame.size);

            let progress = decoder.receive(&frame.uri).expect("receive failed");
            if progress.is_complete {
                break;
            }
        }

        assert!(decoder.is_complete());
        let decoded = decoder
            .message()
            .expect("message failed")
            .expect("no message");
        assert_eq!(decoded, data);
    }

    fn assert_fixture_roundtrips_as_ur(data: &[u8], ur_type: &'static str) {
        let mut encoder =
            UrEncoder::new(data, ur_type, 250).expect("fixture encoder creation failed");
        assert!(
            encoder.fragment_count() > 1,
            "fixture should exercise animated multi-frame UR encoding"
        );

        let mut decoder = UrDecoder::new();
        for _ in 0..encoder.fragment_count() * 3 {
            let frame = encoder.next_frame().expect("fixture next_frame failed");
            assert!(frame.uri.starts_with(&format!("ur:{ur_type}/")));
            assert_eq!(frame.modules.len(), frame.size * frame.size);

            let progress = decoder.receive(&frame.uri).expect("fixture receive failed");
            if progress.is_complete {
                break;
            }
        }

        assert!(decoder.is_complete(), "fixture UR stream did not complete");
        let decoded = decoder
            .message()
            .expect("fixture message failed")
            .expect("fixture UR message missing");
        assert_eq!(decoded, data);
    }

    #[test]
    fn monero_wallet2_unsigned_txset_roundtrips_as_xmr_unsigned_ur() {
        let data =
            include_bytes!("../tests/vectors/cold_signing_regtest_v0_18_5_0/unsigned_monero_tx");
        assert_fixture_roundtrips_as_ur(data, UR_TYPE_UNSIGNED_TX);
    }

    #[test]
    fn monero_wallet2_signed_txset_roundtrips_as_xmr_signed_ur() {
        let data =
            include_bytes!("../tests/vectors/cold_signing_regtest_v0_18_5_0/signed_monero_tx");
        assert_fixture_roundtrips_as_ur(data, UR_TYPE_SIGNED_TX);
    }

    #[test]
    fn monero_output_export_roundtrips_as_xmr_output_ur() {
        let data = include_bytes!("../tests/vectors/cold_signing_regtest_v0_18_5_0/outputs");
        assert_fixture_roundtrips_as_ur(data, UR_TYPE_OUTPUT);
    }

    #[test]
    fn test_qr_frame_dimensions() {
        let data = b"test";
        let mut encoder =
            UrEncoder::new(data, UR_TYPE_KEY_IMAGE, 100).expect("encoder creation failed");
        let frame = encoder.next_frame().expect("next_frame failed");

        assert_eq!(frame.modules.len(), frame.size * frame.size);
        assert!(frame.size >= 21);
    }

    #[test]
    fn test_parse_sequence_total() {
        assert_eq!(
            parse_sequence_total("ur:xmr-txunsigned/1-5/lpadbbcs"),
            Some(5)
        );
        assert_eq!(parse_sequence_total("ur:bytes/3-10/abcdef"), Some(10));
        assert_eq!(parse_sequence_total("ur:bytes/abcdef"), None);
        assert_eq!(parse_sequence_total("not-a-ur"), None);
    }

    /// Build a frame with caller-supplied CBOR sequence values.
    ///
    /// Keep the URI's text indices different from the CBOR values to check that
    /// validation reads the header used for fountain decoding.
    fn hostile_frame(sequence: u32, sequence_count: u32) -> String {
        let mut cbor = vec![0x85]; // array(5)
        for value in [sequence, sequence_count, 1024u32, 0u32] {
            cbor.push(0x1a); // unsigned integer, 4 bytes big-endian
            cbor.extend_from_slice(&value.to_be_bytes());
        }
        cbor.extend_from_slice(&[0x41, 0x00]); // bytes(1) = [0]

        format!(
            "ur:{}/1-2/{}",
            UR_TYPE_UNSIGNED_TX,
            ur::bytewords::encode(&cbor, ur::bytewords::Style::Minimal)
        )
    }

    /// Reject an excessive CBOR fragment count before the UR decoder allocates
    /// its fragment and degree arrays. One frame can otherwise request enough
    /// memory to abort a wasm instance.
    #[test]
    fn refuses_a_frame_declaring_an_absurd_fragment_count() {
        // Check that the header parses so the test exercises the count limit.
        let honest = hostile_frame(1, 2);
        assert_eq!(
            read_fountain_header(
                &ur::bytewords::decode(
                    honest.rsplit_once('/').unwrap().1,
                    ur::bytewords::Style::Minimal
                )
                .unwrap()
            ),
            Some((1, 2)),
            "the fixture must produce a header this code can read"
        );
        UrDecoder::new()
            .receive(&honest)
            .expect("a well-formed frame within the bound must still be accepted");

        // This sequence selects the branch that allocates fragment arrays.
        let uri = hostile_frame(4_000_000_001, 4_000_000_000);
        let err = UrDecoder::new()
            .receive(&uri)
            .expect_err("a frame declaring four billion fragments must be refused");
        assert!(
            err.contains("4000000000 fragments"),
            "the refusal must name the declared count: {err}"
        );
    }

    /// Reject sequence zero before `choose_fragments` subtracts one from it.
    /// The underlying decoder checks for a zero count but not a zero sequence.
    #[test]
    fn refuses_a_frame_declaring_sequence_zero() {
        let err = UrDecoder::new()
            .receive(&hostile_frame(0, 4))
            .expect_err("sequence 0 must be refused before the crate subtracts one from it");
        assert!(err.contains("sequence 0"), "unexpected error: {err}");
    }

    #[test]
    fn read_fountain_header_reads_every_integer_width() {
        // Cover immediate values through 23 and the 1-, 2-, and 4-byte encodings.
        let build = |a: &[u8], b: &[u8]| {
            let mut cbor = vec![0x85];
            cbor.extend_from_slice(a);
            cbor.extend_from_slice(b);
            cbor.extend_from_slice(&[0x00, 0x00, 0x41, 0x00]);
            cbor
        };
        assert_eq!(read_fountain_header(&build(&[23], &[1])), Some((23, 1)));
        assert_eq!(
            read_fountain_header(&build(&[0x18, 200], &[0x19, 0x01, 0x00])),
            Some((200, 256))
        );
        assert_eq!(
            read_fountain_header(&build(&[0x1a, 0, 0, 1, 0], &[0x1a, 0xff, 0xff, 0xff, 0xff])),
            Some((256, u32::MAX))
        );
        // Reject the wrong array length, a 64-bit integer, and a truncated header.
        assert_eq!(read_fountain_header(&[0x84, 1, 2, 3, 4]), None);
        assert_eq!(
            read_fountain_header(&build(&[0x1b, 0, 0, 0, 0, 0, 0, 0, 1], &[1])),
            None
        );
        assert_eq!(read_fountain_header(&[0x85, 0x1a, 0, 0]), None);
    }

    #[test]
    fn test_decoder_reset() {
        let mut decoder = UrDecoder::new();
        assert!(!decoder.is_complete());
        decoder.reset();
        assert!(!decoder.is_complete());
    }

    #[test]
    fn test_single_part_decode() {
        let data = b"short payload";
        // Encode as single-part UR
        let uri = ur::encode(data, &ur::Type::Bytes);

        // decode_single should work
        let decoded = decode_single(&uri).expect("decode failed");
        assert_eq!(decoded, data);

        // UrDecoder.receive should detect single-part and signal completion
        let mut decoder = UrDecoder::new();
        let progress = decoder.receive(&uri).expect("receive failed");
        assert!(progress.is_complete);
    }
}
