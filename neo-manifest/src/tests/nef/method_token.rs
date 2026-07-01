use super::*;

#[test]
fn default_is_empty_token() {
    let t = MethodToken::default();
    assert_eq!(t.hash, UInt160::zero());
    assert_eq!(t.method, "");
    assert_eq!(t.parameters_count, 0);
    assert!(!t.has_return_value);
    assert_eq!(t.call_flags, CallFlags::NONE);
}

#[test]
fn new_rejects_underscore_prefix() {
    let result = MethodToken::new(
        UInt160::zero(),
        "_private".to_string(),
        0,
        false,
        CallFlags::NONE,
    );
    assert!(result.is_err());
}

#[test]
fn new_rejects_long_method_name() {
    let result = MethodToken::new(
        UInt160::zero(),
        "a".repeat(MethodToken::MAX_METHOD_LENGTH + 1),
        0,
        false,
        CallFlags::NONE,
    );
    assert!(result.is_err());
}
