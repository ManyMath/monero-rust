//! Structural guard for the pinned Epee deserializer, whose varint reader
//! assumes a nonempty buffer and whose unknown fields recurse without a depth
//! bound. Keep all untrusted Epee entry points behind this module, including on wasm.

use serde::de::DeserializeOwned;

const HEADER: &[u8] = b"\x01\x11\x01\x01\x01\x01\x02\x01\x01";
// RPC responses and account exports use only a handful of object levels. Leave
// ample room for extensions while bounding both this walk and serde's stack.
const MAX_DEPTH: usize = 64;
// String and object array elements decode to at least 24 bytes each from as
// little as one byte on the wire, so their number is bounded by the input size.
const ARRAY_ELEMENTS_FLOOR: usize = 1024;
const BYTES_PER_ARRAY_ELEMENT: usize = 32;

pub(crate) fn from_bytes<T: DeserializeOwned, B: AsRef<[u8]>>(bytes: B) -> Result<T, String> {
    let bytes = bytes.as_ref();
    let mut reader = Reader {
        bytes: bytes.strip_prefix(HEADER).ok_or("Invalid Epee header")?,
        array_elements_left: ARRAY_ELEMENTS_FLOOR + bytes.len() / BYTES_PER_ARRAY_ELEMENT,
    };
    reader.object(0)?;
    if !reader.bytes.is_empty() {
        return Err("Trailing bytes after Epee object".into());
    }
    monero_epee_bin_serde::from_bytes(bytes).map_err(|e| e.to_string())
}

struct Reader<'a> {
    bytes: &'a [u8],
    array_elements_left: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8], String> {
        if len > self.bytes.len() {
            return Err("Truncated Epee value".into());
        }
        let (value, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self) -> Result<usize, String> {
        let first = *self.bytes.first().ok_or("Truncated Epee varint")?;
        let len = 1usize << (first & 3);
        let mut encoded = [0u8; 8];
        encoded[..len].copy_from_slice(self.take(len)?);
        // Convert only after shifting, and reject integers unrepresentable on
        // 32-bit wasm instead of truncating them like the dependency's reader.
        usize::try_from(u64::from_le_bytes(encoded) >> 2)
            .map_err(|_| "Epee length exceeds usize".into())
    }

    fn object(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("Epee nesting limit exceeded".into());
        }
        let fields = self.varint()?;
        // Even an empty field name needs its length, a marker and a value byte.
        if fields > self.bytes.len() / 3 {
            return Err("Truncated Epee fields".into());
        }
        for _ in 0..fields {
            let name_len = usize::from(self.byte()?);
            std::str::from_utf8(self.take(name_len)?).map_err(|_| "Invalid Epee field name")?;
            let marker = self.byte()?;
            let element = marker & 0x7f;
            if !(1..=12).contains(&element) {
                return Err("Invalid Epee type marker".into());
            }
            if marker & 0x80 != 0 {
                let count = self.varint()?;
                // Every array element occupies at least one byte, including
                // empty strings/objects. This also bounds work on bogus counts.
                if count > self.bytes.len() {
                    return Err("Truncated Epee array".into());
                }
                if matches!(element, 10 | 12) {
                    self.array_elements_left = self
                        .array_elements_left
                        .checked_sub(count)
                        .ok_or("Too many Epee array elements for the size of the input")?;
                }
                for _ in 0..count {
                    self.value(element, depth + 1)?;
                }
            } else {
                self.value(element, depth + 1)?;
            }
        }
        Ok(())
    }

    fn value(&mut self, marker: u8, depth: usize) -> Result<(), String> {
        let size = match marker {
            1 | 5 | 9 => 8,
            2 | 6 => 4,
            3 | 7 => 2,
            4 | 8 => 1,
            10 => self.varint()?,
            11 => {
                return match self.byte()? {
                    0 | 1 => Ok(()),
                    _ => Err("Invalid Epee bool".into()),
                }
            }
            12 => return self.object(depth),
            _ => return Err("Invalid Epee type marker".into()),
        };
        self.take(size)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Nested {
        number: u64,
        children: Vec<Leaf>,
    }
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Leaf {
        flag: bool,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    }
    #[derive(Debug, Deserialize)]
    struct Empty {}

    fn nested_bytes() -> Vec<u8> {
        monero_epee_bin_serde::to_bytes(&Nested {
            number: 17,
            children: vec![
                Leaf {
                    flag: true,
                    bytes: vec![0, 255, 1],
                },
                Leaf {
                    flag: false,
                    bytes: vec![],
                },
            ],
        })
        .unwrap()
    }

    #[test]
    fn accepts_nested_objects_arrays_blobs_and_unknown_fields() {
        let bytes = nested_bytes();
        let expected: Nested = monero_epee_bin_serde::from_bytes(&bytes).unwrap();
        assert_eq!(from_bytes::<Nested, _>(&bytes).unwrap(), expected);
        from_bytes::<Empty, _>(&bytes).unwrap();
        // Fixed wire vector, independent of the serializer: {x: {y: 42u64}}.
        let fixed = hex::decode("0111010101010201010401780c040179052a00000000000000").unwrap();
        from_bytes::<Empty, _>(fixed).unwrap();
    }

    #[test]
    fn every_truncation_returns_an_error_without_unwinding() {
        let bytes = nested_bytes();
        for end in 0..bytes.len() {
            assert!(from_bytes::<Empty, _>(&bytes[..end]).is_err(), "end={end}");
        }
        // Each length encoding, including missing first byte and partial u64.
        for marker in [0u8, 1, 2, 3] {
            let mut bytes = HEADER.to_vec();
            bytes.extend_from_slice(&[4, 1, b'x', 10, marker]);
            if marker == 0 {
                bytes.pop();
            }
            assert!(from_bytes::<Empty, _>(bytes).is_err());
        }
    }

    #[test]
    fn rejects_recursive_unknown_objects_and_object_arrays() {
        for array in [false, true] {
            let mut bytes = HEADER.to_vec();
            for _ in 0..MAX_DEPTH + 2 {
                bytes.extend_from_slice(&[4, 1, b'x', if array { 0x8c } else { 12 }]);
                if array {
                    bytes.push(4);
                }
            }
            bytes.push(0);
            assert!(from_bytes::<Empty, _>(bytes)
                .unwrap_err()
                .contains("nesting"));
        }
    }

    #[test]
    fn rejects_impossible_lengths_and_trailing_bytes() {
        let mut bytes = HEADER.to_vec();
        bytes.extend_from_slice(&[4, 1, b'x', 10]);
        bytes.extend_from_slice(&[255; 8]);
        assert!(from_bytes::<Empty, _>(&bytes).is_err());
        let mut bytes = nested_bytes();
        bytes.push(0);
        assert!(from_bytes::<Empty, _>(bytes).is_err());
    }
    #[test]
    fn rejects_arrays_of_more_small_elements_than_the_input_pays_for() {
        let array = |element: u8, count: u32, encoded: &[u8]| {
            let mut bytes = HEADER.to_vec();
            bytes.extend_from_slice(&[4, 1, b'x', 0x80 | element]);
            bytes.extend_from_slice(&((u64::from(count) << 2) | 3).to_le_bytes());
            for _ in 0..count {
                bytes.extend_from_slice(encoded);
            }
            bytes
        };
        for element in [10u8, 12] {
            for width_tag in 0..4 {
                let zero = &[width_tag, 0, 0, 0, 0, 0, 0, 0][..1 << width_tag];
                from_bytes::<Empty, _>(array(element, 1000, zero)).unwrap();
                let error = from_bytes::<Empty, _>(array(element, 100_000, zero)).unwrap_err();
                assert!(error.contains("array elements"), "{error}");
            }
        }
        let error = from_bytes::<Empty, _>(array(10, 100_000, &[4, b'a'])).unwrap_err();
        assert!(error.contains("array elements"), "{error}");
        let one_field = [4, 1, b'a', 5, 0, 0, 0, 0, 0, 0, 0, 0];
        let error = from_bytes::<Empty, _>(array(12, 100_000, &one_field)).unwrap_err();
        assert!(error.contains("array elements"), "{error}");
        let mut string = vec![31 << 2];
        string.extend_from_slice(&[7; 31]);
        from_bytes::<Empty, _>(array(10, 100_000, &string)).unwrap();
        from_bytes::<Empty, _>(array(8, 100_000, &[1])).unwrap();
    }

    #[test]
    fn valid_wire_types_mismatched_with_rust_types_return_without_panicking() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Field<T> {
            x: T,
        }
        // Exercise typed scalar, string, tuple, sequence, option and struct
        // dispatch on the same fully validated wire trees. No catch_unwind:
        // these calls must remain safe when the target aborts on panic.
        for marker in 1u8..=12 {
            let value = match marker {
                1 | 5 | 9 => vec![0; 8],
                2 | 6 => vec![0; 4],
                3 | 7 => vec![0; 2],
                4 | 8 | 11 => vec![0],
                10 => vec![8, 0, 255],         // two arbitrary blob bytes
                12 => vec![4, 1, b'y', 8, 42], // nested {y: 42u8}
                _ => unreachable!(),
            };
            for array in [false, true] {
                // Include all four encodings of the root field count (1).
                // Over-wide encodings are legal in the pinned backend.
                for size_tag in 0..4 {
                    let mut wire = HEADER.to_vec();
                    let encoded = (4u64 | size_tag).to_le_bytes();
                    wire.extend_from_slice(&encoded[..1 << size_tag]);
                    wire.extend_from_slice(&[1, b'x', marker | if array { 128 } else { 0 }]);
                    if array {
                        wire.push(8);
                    }
                    wire.extend_from_slice(&value);
                    if array {
                        wire.extend_from_slice(&value);
                    }
                    from_bytes::<Empty, _>(&wire).unwrap();
                    let _ = from_bytes::<Field<u64>, _>(&wire);
                    let _ = from_bytes::<Field<bool>, _>(&wire);
                    let _ = from_bytes::<Field<String>, _>(&wire);
                    let _ = from_bytes::<Field<[u8; 32]>, _>(&wire);
                    let _ = from_bytes::<Field<Vec<u8>>, _>(&wire);
                    let _ = from_bytes::<Field<Vec<Empty>>, _>(&wire);
                    let _ = from_bytes::<Field<Option<Empty>>, _>(&wire);
                    let _ = from_bytes::<Field<serde_bytes::ByteBuf>, _>(&wire);
                }
            }
        }
    }
}
