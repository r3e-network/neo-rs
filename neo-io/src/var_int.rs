//! Neo compact integer helpers.
//!
//! These helpers implement the Bitcoin-style compact integer encoding used by
//! Neo wire formats and binary serialization. They are intentionally not SCALE,
//! RLP, or any other external protocol codec.

const VAR_INT_U16_MARKER: u8 = 0xFD;
const VAR_INT_U32_MARKER: u8 = 0xFE;
const VAR_INT_U64_MARKER: u8 = 0xFF;

/// Attempts to read a compact integer from the beginning of `src`.
///
/// Returns `None` when the slice does not yet contain enough bytes. This is
/// useful for streaming decoders that must leave partial frames buffered.
#[must_use]
pub fn read_var_int_prefix(src: &[u8]) -> Option<(u64, usize)> {
    let prefix = *src.first()?;

    match prefix {
        VAR_INT_U16_MARKER => {
            if src.len() < 3 {
                return None;
            }
            Some((u16::from_le_bytes([src[1], src[2]]) as u64, 3))
        }
        VAR_INT_U32_MARKER => {
            if src.len() < 5 {
                return None;
            }
            Some((
                u32::from_le_bytes([src[1], src[2], src[3], src[4]]) as u64,
                5,
            ))
        }
        VAR_INT_U64_MARKER => {
            if src.len() < 9 {
                return None;
            }
            Some((
                u64::from_le_bytes([
                    src[1], src[2], src[3], src[4], src[5], src[6], src[7], src[8],
                ]),
                9,
            ))
        }
        value => Some((value as u64, 1)),
    }
}

/// Appends `value` using Neo compact integer encoding.
pub fn write_var_int(value: u64, dst: &mut Vec<u8>) {
    if value < VAR_INT_U16_MARKER as u64 {
        dst.push(value as u8);
    } else if value <= 0xFFFF {
        dst.push(VAR_INT_U16_MARKER);
        dst.extend_from_slice(&(value as u16).to_le_bytes());
    } else if value <= 0xFFFF_FFFF {
        dst.push(VAR_INT_U32_MARKER);
        dst.extend_from_slice(&(value as u32).to_le_bytes());
    } else {
        dst.push(VAR_INT_U64_MARKER);
        dst.extend_from_slice(&value.to_le_bytes());
    }
}

/// Returns the number of bytes required to encode `value`.
#[inline]
#[must_use]
pub const fn encoded_len(value: u64) -> usize {
    if value < VAR_INT_U16_MARKER as u64 {
        1
    } else if value <= 0xFFFF {
        3
    } else if value <= 0xFFFF_FFFF {
        5
    } else {
        9
    }
}

/// Appends a Neo var-bytes payload to `dst`.
pub fn write_var_bytes(bytes: &[u8], dst: &mut Vec<u8>) {
    write_var_int(bytes.len() as u64, dst);
    dst.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::{encoded_len, read_var_int_prefix, write_var_bytes, write_var_int};

    #[test]
    fn reads_var_int_prefix_without_consuming() {
        let cases: &[(&[u8], u64, usize)] = &[
            (&[0xFC], 0xFC, 1),
            (&[0xFD, 0xFD, 0x00], 0xFD, 3),
            (&[0xFD, 0xFF, 0xFF], 0xFFFF, 3),
            (&[0xFE, 0x00, 0x00, 0x01, 0x00], 0x1_0000, 5),
            (&[0xFE, 0xFF, 0xFF, 0xFF, 0xFF], 0xFFFF_FFFF, 5),
            (&[0xFF, 0, 0, 0, 0, 1, 0, 0, 0], 0x1_0000_0000, 9),
        ];

        for (encoded, value, width) in cases {
            assert_eq!(read_var_int_prefix(encoded), Some((*value, *width)));
        }
    }

    #[test]
    fn waits_for_incomplete_var_int_prefix() {
        for encoded in [
            &[][..],
            &[0xFD][..],
            &[0xFD, 0x01][..],
            &[0xFE, 0, 0, 0][..],
            &[0xFF, 0, 0, 0, 0, 0, 0, 0][..],
        ] {
            assert_eq!(read_var_int_prefix(encoded), None);
        }
    }

    #[test]
    fn reads_non_canonical_prefix_for_legacy_compatibility() {
        let cases: &[(&[u8], u64, usize)] = &[
            (&[0xFD, 0x01, 0x00], 1, 3),
            (&[0xFE, 0x01, 0x00, 0x00, 0x00], 1, 5),
            (
                &[0xFF, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
                1,
                9,
            ),
        ];

        for (encoded, value, width) in cases {
            assert_eq!(read_var_int_prefix(encoded), Some((*value, *width)));
        }
    }

    #[test]
    fn writes_canonical_var_int_encoding() {
        let cases: &[(u64, &[u8])] = &[
            (0xFC, &[0xFC]),
            (0xFD, &[0xFD, 0xFD, 0x00]),
            (0xFFFF, &[0xFD, 0xFF, 0xFF]),
            (0x1_0000, &[0xFE, 0x00, 0x00, 0x01, 0x00]),
            (0xFFFF_FFFF, &[0xFE, 0xFF, 0xFF, 0xFF, 0xFF]),
            (0x1_0000_0000, &[0xFF, 0, 0, 0, 0, 1, 0, 0, 0]),
        ];

        for (value, expected) in cases {
            let mut encoded = Vec::new();
            write_var_int(*value, &mut encoded);
            assert_eq!(&encoded, expected);
        }
    }

    #[test]
    fn calculates_encoded_var_int_length() {
        let cases = [
            (0, 1),
            (0xFC, 1),
            (0xFD, 3),
            (0xFFFF, 3),
            (0x1_0000, 5),
            (0xFFFF_FFFF, 5),
            (0x1_0000_0000, 9),
        ];

        for (value, expected) in cases {
            assert_eq!(encoded_len(value), expected);
        }
    }

    #[test]
    fn writes_var_bytes_without_intermediate_writer() {
        let mut encoded = Vec::new();

        write_var_bytes(&[0xAA, 0xBB], &mut encoded);

        assert_eq!(encoded, vec![0x02, 0xAA, 0xBB]);
    }
}
