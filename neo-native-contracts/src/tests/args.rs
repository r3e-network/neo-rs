use super::*;

#[test]
fn raw_integer_helpers_decode_vm_signed_little_endian_bytes() {
    let args = vec![
        BigInt::from(-1).to_signed_bytes_le(),
        BigInt::from(0x1234_u32).to_signed_bytes_le(),
        BigInt::from(0x7f_i32).to_signed_bytes_le(),
    ];

    assert_eq!(raw_i64_arg(&args, 0, "test").unwrap(), -1);
    assert_eq!(raw_u32_arg(&args, 1, "test").unwrap(), 0x1234);
    assert_eq!(raw_i32_arg(&args, 1, "test").unwrap(), 0x1234);
    assert_eq!(raw_u8_arg(&args, 2, "test").unwrap(), 0x7f);
}

#[test]
fn raw_integer_helpers_reject_missing_or_out_of_range_args() {
    let too_large_for_u8 = vec![BigInt::from(256_u16).to_signed_bytes_le()];
    assert!(raw_u8_arg(&too_large_for_u8, 0, "test").is_err());
    assert!(raw_i64_arg(&[], 0, "test").is_err());
}

#[test]
fn raw_integer_byte_helpers_decode_vm_signed_little_endian_bytes() {
    let positive = BigInt::from(0x1234_u32).to_signed_bytes_le();
    let negative = BigInt::from(-1).to_signed_bytes_le();

    assert_eq!(raw_integer_bytes(&positive), BigInt::from(0x1234_u32));
    assert_eq!(
        raw_integer_bytes_to_u32(&positive, "value").unwrap(),
        0x1234
    );
    assert_eq!(
        raw_integer_bytes_to_i32(&positive, "value").unwrap(),
        0x1234
    );
    assert_eq!(raw_integer_bytes_to_i64(&negative, "value").unwrap(), -1);
    assert_eq!(raw_integer_bytes_to_u32(&[], "empty").unwrap(), 0);
    assert!(raw_integer_bytes_to_u8(&positive, "value").is_err());
    assert!(raw_integer_bytes_to_u32(&negative, "value").is_err());
}

#[test]
fn raw_required_integer_arg_preserves_domain_missing_context() {
    let args = vec![BigInt::from(42).to_signed_bytes_le()];
    assert_eq!(
        raw_required_integer_arg(&args, 0, "Token::transfer", "an amount").unwrap(),
        BigInt::from(42)
    );

    let missing = raw_required_integer_arg(&[], 0, "Token::transfer", "an amount")
        .expect_err("missing named integer should fault");
    assert!(missing.to_string().contains("requires an amount"));
}

#[test]
fn raw_string_arg_decodes_utf8_with_named_errors() {
    let args = vec![b"balanceOf".to_vec()];
    assert_eq!(
        raw_string_arg(&args, 0, "Contract::method", "method name").unwrap(),
        "balanceOf"
    );

    let missing = raw_string_arg(&[], 0, "Contract::method", "method name")
        .expect_err("missing string arg should fault");
    assert!(missing.to_string().contains("requires a method name"));

    let invalid = raw_string_arg(&[vec![0xff]], 0, "Contract::method", "method name")
        .expect_err("invalid UTF-8 should fault");
    assert!(invalid.to_string().contains("bad method name"));
}
