use neo_core::big_decimal::BigDecimal;
use num_bigint::BigInt;

#[test]
fn test_bigdecimal_comprehensive() {
    println!("Testing BigDecimal implementation...");

    // Test basic constructor
    let bd = BigDecimal::new(BigInt::from(12345), 2);
    println!("BigDecimal::new(12345, 2) = {}", bd);
    assert_eq!(bd.to_string(), "123.45");

    // Test change_decimals
    let original_value = BigDecimal::new(BigInt::from(12300), 5);
    println!("Original value: {}", original_value);

    // Increase decimals
    let result1 = original_value.change_decimals(7).unwrap();
    println!(
        "Changed to 7 decimals: {} (value: {})",
        result1,
        result1.value()
    );
    assert_eq!(result1.value(), &BigInt::from(1230000));
    assert_eq!(result1.decimals(), 7);

    // Decrease decimals
    let result2 = original_value.change_decimals(3).unwrap();
    println!(
        "Changed to 3 decimals: {} (value: {})",
        result2,
        result2.value()
    );
    assert_eq!(result2.value(), &BigInt::from(123));
    assert_eq!(result2.decimals(), 3);

    // Same decimals
    let result3 = original_value.change_decimals(5).unwrap();
    println!("Same decimals: {} (value: {})", result3, result3.value());
    assert_eq!(result3.value(), original_value.value());

    // Test error case - would lose precision
    let result = original_value.change_decimals(2);
    println!("Should fail (precision loss): {:?}", result);
    assert!(result.is_err());

    // Test parsing
    let parsed = BigDecimal::parse("123.45", 2).unwrap();
    println!(
        "Parsed '123.45' with 2 decimals: {} (value: {})",
        parsed,
        parsed.value()
    );
    assert_eq!(parsed.value(), &BigInt::from(12345));
    assert_eq!(parsed.decimals(), 2);

    // Test try_parse
    let (success, result) = BigDecimal::try_parse("123.45", 2);
    println!("try_parse '123.45': success={}, result={}", success, result);
    assert!(success);
    assert_eq!(result.value(), &BigInt::from(12345));

    let (success, _) = BigDecimal::try_parse("invalid", 2);
    println!("try_parse 'invalid': success={}", success);
    assert!(!success);

    // Test comparison operators
    let a = BigDecimal::new(BigInt::from(1000), 2); // 10.00
    let b = BigDecimal::new(BigInt::from(10000), 3); // 10.000
    let c = BigDecimal::new(BigInt::from(10001), 2); // 100.01

    println!("Comparing: a={}, b={}, c={}", a, b, c);
    assert!(a == b); // 10.00 == 10.000
    assert!(a != c); // 10.00 != 100.01
    assert!(a < c); // 10.00 < 100.01
    assert!(c > a); // 100.01 > 10.00

    println!("All BigDecimal tests passed!");
}
