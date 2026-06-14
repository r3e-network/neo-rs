use hex::{decode as hex_decode, encode as hex_encode};
use neo_error::{CoreError, CoreResult};
use neo_io::{IoError, IoResult, MemoryReader};

/// Size of a compressed ECPoint in bytes.
pub const ECPOINT_COMPRESSED_SIZE: usize = 33;
/// Size of an uncompressed ECPoint in bytes.
pub const ECPOINT_UNCOMPRESSED_SIZE: usize = 65;

/// Encodes bytes as a lowercase hex string.
pub fn encode_hex(bytes: &[u8]) -> String {
    hex_encode(bytes)
}

fn decode_hex(value: &str) -> CoreResult<Vec<u8>> {
    hex_decode(neo_primitives::strip_hex_prefix(value))
        .map_err(|e| CoreError::other(format!("Invalid hex string: {e}")))
}

/// Parses a hex-encoded ECPoint group, validating the byte length.
pub fn parse_group_bytes(value: &str) -> CoreResult<Vec<u8>> {
    let bytes = decode_hex(value)?;
    // Validate compressed/uncompressed ECPoint length without ECCurve dependency.
    // Full ECPoint validation is performed in neo-core at deserialization time.
    match bytes.len() {
        ECPOINT_COMPRESSED_SIZE | ECPOINT_UNCOMPRESSED_SIZE => Ok(bytes),
        _ => Err(CoreError::other(format!(
            "Invalid ECPoint length: expected {} or {} bytes, got {}",
            ECPOINT_COMPRESSED_SIZE,
            ECPOINT_UNCOMPRESSED_SIZE,
            bytes.len()
        ))),
    }
}

/// Reads a group (ECPoint) from the reader, validating the encoding prefix.
pub fn read_group_bytes(reader: &mut MemoryReader) -> IoResult<Vec<u8>> {
    let prefix = reader.peek()?;
    let encoded_len = match prefix {
        0x02 | 0x03 => ECPOINT_COMPRESSED_SIZE,
        0x04 => ECPOINT_UNCOMPRESSED_SIZE,
        _ => {
            return Err(IoError::invalid_data(
                "Invalid ECPoint encoding prefix for witness group",
            ));
        }
    };
    reader.read_bytes(encoded_len)
}
