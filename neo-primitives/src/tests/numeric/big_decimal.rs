use super::*;
// No extra imports needed for these unit tests

#[test]
fn test_big_decimal_new() {
    let bd = BigDecimal::new(BigInt::from(123), 2);
    assert_eq!(bd.value(), &BigInt::from(123));
    assert_eq!(bd.decimals(), 2);
}
#[test]
fn test_big_decimal_sign() {
    let positive = BigDecimal::new(BigInt::from(123), 2);
    let zero = BigDecimal::new(BigInt::from(0), 2);
    let negative = BigDecimal::new(BigInt::from(-123), 2);
    assert_eq!(positive.sign(), 1);
    assert_eq!(zero.sign(), 0);
    assert_eq!(negative.sign(), -1);
}
#[test]
fn test_big_decimal_change_decimals() {
    let bd = BigDecimal::new(BigInt::from(123), 2);
    // Increase precision
    let increased = bd.change_decimals(4).unwrap();
    assert_eq!(increased.value(), &BigInt::from(12300));
    assert_eq!(increased.decimals(), 4);
    // Decrease precision with no remainder
    let bd = BigDecimal::new(BigInt::from(12300), 4);
    let decreased = bd.change_decimals(2).unwrap();
    assert_eq!(decreased.value(), &BigInt::from(123));
    assert_eq!(decreased.decimals(), 2);
    let bd = BigDecimal::new(BigInt::from(1234), 3);
    let result = bd.change_decimals(2);
    assert!(result.is_err());
}
#[test]
fn test_big_decimal_parse_trailing_zero_amounts() {
    // Regression: trailing-zero fractional amounts ("0.0", "10.0", "100.0",
    // "0.000", "1.230") used to underflow the usize decimal-place counter
    // (panic under overflow-checks, garbage scale in release).
    for (input, expected) in [
        ("0.0", 0i64),
        ("10.0", 10 * 100_000_000),
        ("100.0", 100 * 100_000_000),
        ("0.000", 0),
        ("1.230", 123_000_000),
    ] {
        let bd = BigDecimal::parse(input, 8).unwrap();
        assert_eq!(bd.value(), &BigInt::from(expected), "parse({input})");
        assert_eq!(bd.decimals(), 8);
    }
}
#[test]
fn test_big_decimal_parse() {
    // Simple integer
    let bd = BigDecimal::parse("123", 2).unwrap();
    assert_eq!(bd.value(), &BigInt::from(12300));
    assert_eq!(bd.decimals(), 2);
    // Decimal
    let bd = BigDecimal::parse("123.45", 2).unwrap();
    assert_eq!(bd.value(), &BigInt::from(12345));
    assert_eq!(bd.decimals(), 2);
    // Scientific notation
    let bd = BigDecimal::parse("1.2345e2", 2).unwrap();
    assert_eq!(bd.value(), &BigInt::from(12345));
    assert_eq!(bd.decimals(), 2);
    // Negative
    let bd = BigDecimal::parse("-123.45", 2).unwrap();
    assert_eq!(bd.value(), &BigInt::from(-12345));
    assert_eq!(bd.decimals(), 2);
}
#[test]
fn test_big_decimal_display() {
    // Integer
    let bd = BigDecimal::new(BigInt::from(123), 0);
    assert_eq!(bd.to_string(), "123");
    // With decimals
    let bd = BigDecimal::new(BigInt::from(12345), 2);
    assert_eq!(bd.to_string(), "123.45");
    // With trailing zeros
    let bd = BigDecimal::new(BigInt::from(12300), 2);
    assert_eq!(bd.to_string(), "123");
    // Negative
    let bd = BigDecimal::new(BigInt::from(-12345), 2);
    assert_eq!(bd.to_string(), "-123.45");
}

#[test]
fn test_big_decimal_to_big_integer() {
    let amount = BigDecimal::parse("1.23456789", 9).expect("parse");
    let result = amount.to_big_integer(9).expect("bigint");
    assert_eq!(result, BigInt::from(1234567890u64));

    let amount = BigDecimal::parse("1.23456789", 18).expect("parse");
    let result = amount.to_big_integer(18).expect("bigint");
    let expected = BigInt::from_str("1234567890000000000").expect("bigint");
    assert_eq!(result, expected);

    let amount = BigDecimal::parse("1.23456789", 9).expect("parse");
    let result = amount.to_big_integer(4);
    assert!(result.is_err());
}

#[test]
fn test_big_decimal_mul_preserves_large_combined_scale() {
    let left = BigDecimal::new(BigInt::from(2), 200);
    let right = BigDecimal::new(BigInt::from(3), 100);

    let product = left * right;

    assert_eq!(product.value(), &BigInt::from(6));
    assert_eq!(product.decimals(), 300);
}

#[test]
fn test_big_decimal_comparison() {
    let bd1 = BigDecimal::new(BigInt::from(12345), 2);
    let bd2 = BigDecimal::new(BigInt::from(12345), 2);
    let bd3 = BigDecimal::new(BigInt::from(12346), 2);
    let bd4 = BigDecimal::new(BigInt::from(123450), 3);
    assert_eq!(bd1, bd2);
    assert!(bd1 < bd3);
    assert_eq!(bd1, bd4);
}
