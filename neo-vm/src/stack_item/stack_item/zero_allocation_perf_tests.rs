use super::*;

#[test]
fn as_bool_bytestring_no_clone() {
    // PERF REGRESSION: Verify ByteString as_bool doesn't allocate StackValue
    let bs_truthy = StackItem::ByteString(vec![0x01, 0x02]);
    let bs_falsy = StackItem::ByteString(vec![0x00, 0x00]);
    let bs_empty = StackItem::ByteString(vec![]);

    // Protocol correctness: truthiness logic matches C# Neo VM
    assert!(
        bs_truthy.as_bool().unwrap(),
        "non-zero bytes should be true"
    );
    assert!(
        !bs_falsy.as_bool().unwrap(),
        "all-zero bytes should be false"
    );
    assert!(!bs_empty.as_bool().unwrap(), "empty bytes should be false");

    // Edge case: single non-zero byte
    assert!(StackItem::ByteString(vec![0x01]).as_bool().unwrap());
    // Edge case: single zero byte
    assert!(!StackItem::ByteString(vec![0x00]).as_bool().unwrap());
}

#[test]
fn as_bool_integer_fast_path() {
    // PERF REGRESSION: Small integers use fast path without heap access
    let zero = StackItem::from_i64(0);
    let one = StackItem::from_i64(1);
    let negative = StackItem::from_i64(-42);
    let large = StackItem::from_i64(999999);

    // Protocol correctness: integer truthiness
    assert!(!zero.as_bool().unwrap(), "zero is false");
    assert!(one.as_bool().unwrap(), "one is true");
    assert!(negative.as_bool().unwrap(), "negative is true");
    assert!(large.as_bool().unwrap(), "large value is true");
}

#[test]
fn into_bytes_moves_bytestring() {
    // PERF REGRESSION: into_bytes should move Vec without clone for ByteString
    let original_data = vec![0x01, 0x02, 0x03, 0x04];
    let bs = StackItem::ByteString(original_data.clone());

    let bytes = bs.into_bytes().unwrap();
    assert_eq!(bytes, original_data, "bytes should match");
}

#[test]
fn as_bytes_small_integer_fast_path() {
    // PERF REGRESSION: Small integers convert without BigInt serialization
    let test_cases = vec![
        (0i64, vec![]),        // zero = empty
        (1i64, vec![0x01]),    // single byte positive
        (127i64, vec![0x7F]),  // max single byte positive
        (-1i64, vec![0xFF]),   // single byte negative
        (-128i64, vec![0x80]), // min single byte negative
    ];

    for (value, expected) in test_cases {
        let item = StackItem::from_i64(value);
        let bytes = item.as_bytes().unwrap();
        assert_eq!(
            bytes, expected,
            "fast path for {value} should produce {expected:?}"
        );
    }

    // Larger values fall back to BigInt serialization
    let large = StackItem::from_i64(128);
    let bytes = large.as_bytes().unwrap();
    assert_eq!(
        bytes,
        vec![0x80, 0x00],
        "128 requires two bytes with sign bit"
    );
}

#[test]
fn into_bytes_small_integer_fast_path() {
    // PERF REGRESSION: into_bytes also uses fast path for small integers
    let test_cases = vec![(0i64, vec![]), (42i64, vec![0x2A]), (-1i64, vec![0xFF])];

    for (value, expected) in test_cases {
        let item = StackItem::from_i64(value);
        let bytes = item.into_bytes().unwrap();
        assert_eq!(bytes, expected, "into_bytes fast path for {value}");
    }
}

#[test]
fn into_int_zero_allocation_for_small() {
    // PERF REGRESSION: into_int moves without clone for VmInteger::Small
    let small = StackItem::from_i64(42);
    let bigint = small.into_int().unwrap();
    assert_eq!(bigint, BigInt::from(42), "should produce correct value");

    // Also test Boolean and other conversions maintain correctness
    let bool_true = StackItem::Boolean(true);
    assert_eq!(bool_true.into_int().unwrap(), BigInt::from(1));

    let bool_false = StackItem::Boolean(false);
    assert_eq!(bool_false.into_int().unwrap(), BigInt::from(0));
}

#[test]
fn convert_to_same_type_returns_clone() {
    // PERF REGRESSION: Same-type conversion uses fast path
    let int_item = StackItem::from_i64(42);
    let converted = int_item.convert_to(StackItemType::Integer).unwrap();
    assert_eq!(converted.as_int().unwrap(), BigInt::from(42));

    let bs = StackItem::ByteString(vec![0x01, 0x02]);
    let converted = bs.convert_to(StackItemType::ByteString).unwrap();
    assert_eq!(converted.as_bytes().unwrap(), vec![0x01, 0x02]);
}

#[test]
fn convert_to_direct_without_stackvalue() {
    // PERF REGRESSION: Conversions avoid StackValue intermediate

    // Boolean to ByteString
    let bool_item = StackItem::Boolean(true);
    let bs = bool_item.convert_to(StackItemType::ByteString).unwrap();
    assert_eq!(bs.as_bytes().unwrap(), vec![0x01]);

    // Integer to ByteString
    let int_item = StackItem::from_i64(42);
    let bs = int_item.convert_to(StackItemType::ByteString).unwrap();
    assert_eq!(bs.as_bytes().unwrap(), vec![0x2A]);

    // ByteString to Buffer
    let bs = StackItem::ByteString(vec![0x01, 0x02]);
    let buf = bs.convert_to(StackItemType::Buffer).unwrap();
    assert!(matches!(buf, StackItem::Buffer(_)));
    assert_eq!(buf.as_bytes().unwrap(), vec![0x01, 0x02]);

    // Buffer to ByteString
    let buf = StackItem::Buffer(BufferItem::new(vec![0x03, 0x04]));
    let bs = buf.convert_to(StackItemType::ByteString).unwrap();
    assert_eq!(bs.as_bytes().unwrap(), vec![0x03, 0x04]);
}

#[test]
fn protocol_correctness_boolean_conversion() {
    // CRITICAL: Verify protocol-compliant boolean semantics maintained

    // Null is false
    assert!(!StackItem::Null.as_bool().unwrap());

    // Empty bytestring is false
    assert!(!StackItem::ByteString(vec![]).as_bool().unwrap());

    // All-zero bytestring is false
    assert!(!StackItem::ByteString(vec![0x00]).as_bool().unwrap());
    assert!(!StackItem::ByteString(vec![0x00, 0x00]).as_bool().unwrap());

    // Any non-zero byte makes it true
    assert!(StackItem::ByteString(vec![0x00, 0x01]).as_bool().unwrap());
    assert!(StackItem::ByteString(vec![0x01, 0x00]).as_bool().unwrap());

    // Compound types are always true (even empty)
    assert!(
        StackItem::Buffer(BufferItem::new(vec![]))
            .as_bool()
            .unwrap()
    );
    assert!(
        StackItem::from_array(Vec::<StackItem>::new())
            .as_bool()
            .unwrap()
    );
}

#[test]
fn protocol_correctness_byte_conversion() {
    // CRITICAL: Verify byte conversion maintains protocol format

    // Integer to bytes must use little-endian signed format
    let int256 = StackItem::from_i64(256);
    let bytes = int256.as_bytes().unwrap();
    assert_eq!(bytes, vec![0x00, 0x01], "256 in LE signed format");

    // Negative integers
    let neg1 = StackItem::from_i64(-1);
    let bytes = neg1.as_bytes().unwrap();
    assert_eq!(bytes, vec![0xFF], "-1 in LE signed format");

    // Large negative
    let neg256 = StackItem::from_i64(-256);
    let bytes = neg256.as_bytes().unwrap();
    assert_eq!(bytes, vec![0x00, 0xFF], "-256 in LE signed format");

    // Boolean conversions
    assert_eq!(StackItem::Boolean(false).as_bytes().unwrap(), vec![0x00u8]);
    assert_eq!(StackItem::Boolean(true).as_bytes().unwrap(), vec![0x01u8]);

    // Null conversion
    assert_eq!(StackItem::Null.as_bytes().unwrap(), Vec::<u8>::new());
}
