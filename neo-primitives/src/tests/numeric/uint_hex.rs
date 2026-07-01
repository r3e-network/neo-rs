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
