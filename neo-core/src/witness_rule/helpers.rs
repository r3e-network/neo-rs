use crate::neo_io::{IoError, IoResult, MemoryReader};
use crate::{ECCurve, ECPoint};
use hex::{decode as hex_decode, encode as hex_encode};

pub(super) const ECPOINT_COMPRESSED_SIZE: usize = 33;
pub(super) const ECPOINT_UNCOMPRESSED_SIZE: usize = 65;

fn strip_0x(value: &str) -> &str {
    value.strip_prefix("0x").unwrap_or(value)
}

pub(super) fn encode_hex(bytes: &[u8]) -> String {
    hex_encode(bytes)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    hex_decode(strip_0x(value)).map_err(|e| format!("Invalid hex string: {e}"))
}

pub(super) fn parse_group_bytes(value: &str) -> Result<Vec<u8>, String> {
    let bytes = decode_hex(value)?;
    let point = ECPoint::decode(&bytes, ECCurve::secp256r1())
        .map_err(|e| format!("Invalid ECPoint: {e}"))?;
    point
        .encode_point(true)
        .map_err(|e| format!("Failed to encode ECPoint: {e}"))
}

pub(super) fn read_group_bytes(reader: &mut MemoryReader) -> IoResult<Vec<u8>> {
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
    let bytes = reader.read_bytes(encoded_len)?;
    let point = ECPoint::decode(&bytes, ECCurve::secp256r1())
        .map_err(|e| IoError::invalid_data(e.to_string()))?;
    point
        .encode_point(true)
        .map_err(|e| IoError::invalid_data(e.to_string()))
}
