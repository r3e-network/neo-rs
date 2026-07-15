//! Minimal NeoVM integer encoding.

/// Encodes an `i64` as the minimal little-endian two's-complement byte string
/// used by NeoVM push instructions.
#[must_use]
pub fn encode_integer(value: i64) -> Vec<u8> {
    if value == 0 {
        return Vec::new();
    }

    let mut bytes = value.to_le_bytes().to_vec();
    if value > 0 {
        while bytes.len() > 1 && bytes.last() == Some(&0) {
            if bytes[bytes.len() - 2] & 0x80 != 0 {
                break;
            }
            bytes.pop();
        }
    } else {
        while bytes.len() > 1 && bytes.last() == Some(&0xff) {
            if bytes[bytes.len() - 2] & 0x80 == 0 {
                break;
            }
            bytes.pop();
        }
    }
    bytes
}
