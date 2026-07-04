use crate::error::{PrimitiveError, PrimitiveResult};

/// Strips a leading `0x` or `0X` prefix from `s`, returning the substring
/// after the prefix (or the input unchanged if neither prefix matches).
///
/// This is a re-export of [`crate::numeric::hex_util::strip_hex_prefix`]
/// (the canonical implementation, ADR-024). Kept for backward compatibility —
/// new code should import from `hex_util` directly.
#[inline]
#[must_use]
pub fn strip_hex_prefix(s: &str) -> &str {
    crate::numeric::hex_util::strip_hex_prefix(s)
}

pub(crate) fn parse_reversed_hex<const N: usize>(s: &str) -> PrimitiveResult<[u8; N]> {
    let s = strip_hex_prefix(s);

    if s.len() != N * 2 {
        return Err(invalid_format());
    }

    let mut bytes = [0u8; N];
    hex::decode_to_slice(s, &mut bytes).map_err(|_| invalid_format())?;
    bytes.reverse();
    Ok(bytes)
}

pub(crate) fn format_reversed_hex<const N: usize>(mut bytes: [u8; N]) -> String {
    bytes.reverse();
    format!("0x{}", hex::encode(bytes))
}

fn invalid_format() -> PrimitiveError {
    PrimitiveError::InvalidFormat {
        message: "Invalid format".to_string(),
    }
}

#[cfg(test)]
#[path = "../tests/numeric/uint_hex.rs"]
mod tests;
