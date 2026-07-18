use super::*;
use crate::ExecutionEngineLimits;

#[test]
fn test_error_creation() {
    let error = VmError::parse("test message");
    assert!(matches!(error, VmError::Parse { .. }));
    assert_eq!(error.to_string(), "Parse error: test message");
}

#[test]
fn test_error_categories() {
    assert_eq!(VmError::parse("test").category(), "parse");
    assert_eq!(VmError::invalid_opcode(0x42).category(), "instruction");
    assert_eq!(VmError::stack_underflow(1, 0).category(), "stack");
}

#[test]
fn test_error_classification() {
    assert!(VmError::io("test").is_retryable());
    assert!(!VmError::parse("test").is_retryable());

    assert!(VmError::gas_exhausted(1000, 500).is_resource_limit());
    assert!(!VmError::parse("test").is_resource_limit());

    assert!(VmError::parse("test").is_user_error());
    assert!(!VmError::io("test").is_user_error());

    assert!(VmError::stack_underflow(1, 0).should_fault());
    assert!(!VmError::parse("test").should_fault());
}

#[test]
fn test_stack_errors() {
    let error = VmError::stack_underflow(5, 2);
    assert_eq!(
        error.to_string(),
        "Stack underflow: attempted to access 5 items, but only 2 available"
    );

    let error = VmError::insufficient_stack_items(3, 1);
    assert_eq!(
        error.to_string(),
        "Insufficient stack items: required 3, available 1"
    );
}

#[test]
fn test_resource_limit_errors() {
    let limit = ExecutionEngineLimits::DEFAULT.max_item_size as usize;
    let error = VmError::memory_limit_exceeded(2048, limit);
    // C#: ushort.MaxValue = 65535
    assert_eq!(
        error.to_string(),
        format!(
            "Memory limit exceeded: used 2048 bytes, limit {} bytes",
            limit
        )
    );

    let error = VmError::gas_exhausted(1000, 800);
    assert_eq!(error.to_string(), "Gas exhausted: used 1000, limit 800");
}
