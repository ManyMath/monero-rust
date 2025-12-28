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
