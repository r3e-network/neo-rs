use crate::error::{PrimitiveError, PrimitiveResult};

pub(crate) fn parse_reversed_hex<const N: usize>(s: &str) -> PrimitiveResult<[u8; N]> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);

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
