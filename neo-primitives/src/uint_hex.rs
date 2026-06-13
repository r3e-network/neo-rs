use crate::error::{PrimitiveError, PrimitiveResult};

/// Strips a leading `0x` or `0X` prefix from `s`, returning the substring
/// after the prefix (or the input unchanged if neither prefix matches).
///
/// Re-exported as a public helper because the same prefix-stripping logic
/// is duplicated across at least six sites in the workspace
/// (`neo-p2p/src/witness_rule/helpers.rs`, `neo-rpc/src/client/utility.rs`,
/// `neo-rpc/src/client/utility/witness_rule.rs`,
/// `neo-oracle-service/src/neofs/json/helpers.rs`, etc.).
#[inline]
#[must_use]
pub fn strip_hex_prefix(s: &str) -> &str {
    s.strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s)
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
mod tests {
    use super::*;

    #[test]
    fn strip_hex_prefix_handles_both_prefixes() {
        assert_eq!(strip_hex_prefix("0xabcd"), "abcd");
        assert_eq!(strip_hex_prefix("0Xabcd"), "abcd");
        assert_eq!(strip_hex_prefix("abcd"), "abcd");
        assert_eq!(strip_hex_prefix(""), "");
        // 0x-like substring that isn't a real prefix:
        assert_eq!(strip_hex_prefix("0x0"), "0");
    }

    #[test]
    fn parse_reversed_hex_accepts_optional_prefixes() {
        let expected = [1, 0];

        assert_eq!(parse_reversed_hex::<2>("0001").unwrap(), expected);
        assert_eq!(parse_reversed_hex::<2>("0x0001").unwrap(), expected);
        assert_eq!(parse_reversed_hex::<2>("0X0001").unwrap(), expected);
    }

    #[test]
    fn parse_reversed_hex_rejects_invalid_length_or_digits() {
        assert!(parse_reversed_hex::<2>("001").is_err());
        assert!(parse_reversed_hex::<2>("00001").is_err());
        assert!(parse_reversed_hex::<2>("00zz").is_err());
    }

    #[test]
    fn format_reversed_hex_uses_0x_prefix_and_reversed_bytes() {
        assert_eq!(format_reversed_hex([1, 2, 3]), "0x030201");
    }
}
