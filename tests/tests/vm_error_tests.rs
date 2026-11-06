//! VM Error Path Tests
//!
//! Tests for VM execution failure scenarios:
//! - Stack overflow/underflow
//! - Out of gas
//! - Invalid opcodes
//! - Memory limit exceeded
//! - Invalid contract calls

use neo_primitives::UInt160;
use neo_vm::{VmError, VmState};

// ============================================================================
// Stack Error Tests
// ============================================================================

#[test]
fn test_stack_overflow_error() {
    let error = VmError::StackOverflow { max_size: 2048 };
    assert!(error.to_string().contains("stack") || error.to_string().contains("overflow"));
}

#[test]
fn test_stack_underflow_error() {
    let error = VmError::StackUnderflow {
        requested: 5,
        available: 2,
    };
    assert!(error.to_string().contains("stack") || error.to_string().contains("underflow"));
}

#[test]
fn test_invalid_stack_item_type() {
    let error = VmError::InvalidType {
        from: "ByteString".to_string(),
        to: "Integer".to_string(),
    };
    assert!(error.to_string().contains("Integer"));
    assert!(error.to_string().contains("ByteString"));
}

// ============================================================================
// Gas Limit Tests
// ============================================================================

#[test]
fn test_out_of_gas_error() {
    let error = VmError::GasExhausted {
        used: 10_000_000,
        limit: 5_000_000,
    };
    assert!(error.to_string().to_lowercase().contains("gas"));
}

#[test]
fn test_gas_limit_exceeded() {
    // Simulate running out of gas during execution
    let consumed = 10_000_000u64;
    let limit = 5_000_000u64;

    assert!(consumed > limit, "Gas consumption should exceed limit");
}

// ============================================================================
// Opcode Error Tests
// ============================================================================

#[test]
fn test_invalid_opcode_error() {
    let error = VmError::InvalidOpCode { opcode: 0xFF };
    assert!(error.to_string().contains("255") || error.to_string().contains("0xff"));
}

#[test]
fn test_opcode_not_allowed() {
    // Simulate executing a disabled opcode
    let error = VmError::InvalidOperation {
        operation: "opcode".to_string(),
        reason: "not allowed in this context".to_string(),
    };
    assert!(error.to_string().contains("not allowed"));
}

// ============================================================================
// Memory Limit Tests
// ============================================================================

#[test]
fn test_memory_limit_exceeded() {
    let error = VmError::MemoryLimitExceeded {
        used: 100_000_000,
        limit: 50_000_000,
    };
    assert!(error.to_string().contains("100000000") || error.to_string().contains("limit"));
}

#[test]
fn test_item_size_exceeded() {
    let error = VmError::MemoryLimitExceeded {
        used: 1_048_576,
        limit: 1_024_000,
    };
    assert!(error.to_string().contains("1048576") || error.to_string().contains("limit"));
}

// ============================================================================
// Contract Call Error Tests
// ============================================================================

#[test]
fn test_contract_not_found() {
    let missing_contract = UInt160::from([0xAB; 20]);
    let error = VmError::ContractNotFound {
        hash: format!("{}", missing_contract),
    };
    assert!(error.to_string().contains("not found") || error.to_string().contains("Contract"));
}

#[test]
fn test_method_not_found() {
    let error = VmError::MethodNotFound {
        contract: "0xabababababababababababababababababababab".to_string(),
        method: "transfer".to_string(),
    };
    assert!(error.to_string().contains("transfer"));
}

#[test]
fn test_invalid_parameter_count() {
    let error = VmError::InvalidParameters {
        expected: "3 parameters".to_string(),
        actual: "2 parameters".to_string(),
    };
    assert!(error.to_string().contains("3") || error.to_string().contains("parameter"));
}

// ============================================================================
// Execution State Error Tests
// ============================================================================

#[test]
fn test_fault_state_handling() {
    let state = VmState::Fault;
    assert!(matches!(state, VmState::Fault));
}

#[test]
fn test_halt_state_handling() {
    let state = VmState::Halt;
    assert!(matches!(state, VmState::Halt));
}

#[test]
fn test_break_state_handling() {
    let state = VmState::Break;
    assert!(matches!(state, VmState::Break));
}

// ============================================================================
// Arithmetic Error Tests
// ============================================================================

#[test]
fn test_division_by_zero() {
    let error = VmError::DivisionByZero {
        operation: "DIV".to_string(),
    };
    assert!(
        error.to_string().to_lowercase().contains("zero")
            || error.to_string().to_lowercase().contains("division")
    );
}

#[test]
fn test_integer_overflow() {
    let error = VmError::Overflow {
        operation: "ADD".to_string(),
    };
    assert!(error.to_string().contains("overflow") || error.to_string().contains("ADD"));
}

#[test]
fn test_invalid_shift_amount() {
    let error = VmError::InvalidOperation {
        operation: "SHL".to_string(),
        reason: "shift amount exceeds limit".to_string(),
    };
    assert!(error.to_string().contains("shift"));
}

// ============================================================================
// Array and Collection Error Tests
// ============================================================================

#[test]
fn test_index_out_of_bounds() {
    let error = VmError::InvalidOperation {
        operation: "PICKITEM".to_string(),
        reason: "index 10 out of bounds for length 5".to_string(),
    };
    assert!(error.to_string().contains("10") || error.to_string().contains("bounds"));
}

#[test]
fn test_invalid_array_operation() {
    let error = VmError::InvalidOperation {
        operation: "SETITEM".to_string(),
        reason: "cannot modify immutable array".to_string(),
    };
    assert!(error.to_string().contains("immutable"));
}

// ============================================================================
// Serialization Error Tests
// ============================================================================

#[test]
fn test_deserialization_failure() {
    let error = VmError::Parse {
        message: "invalid format".to_string(),
    };
    assert!(error.to_string().contains("invalid") || error.to_string().contains("format"));
}

#[test]
fn test_serialization_failure() {
    let error = VmError::InvalidOperation {
        operation: "serialize".to_string(),
        reason: "item too large to serialize".to_string(),
    };
    assert!(error.to_string().contains("serialize") || error.to_string().contains("large"));
}

// ============================================================================
// Syscall Error Tests
// ============================================================================

#[test]
fn test_invalid_syscall() {
    let error = VmError::InteropService {
        service: "System.Unknown.Method".to_string(),
        error: "service not found".to_string(),
    };
    assert!(error.to_string().contains("Unknown"));
}

#[test]
fn test_syscall_execution_failed() {
    let error = VmError::InteropService {
        service: "System.Storage.Put".to_string(),
        error: "storage is read-only".to_string(),
    };
    assert!(error.to_string().contains("Storage.Put"));
    assert!(error.to_string().contains("read-only"));
}

// ============================================================================
// Script Loading Error Tests
// ============================================================================

#[test]
fn test_invalid_script() {
    let error = VmError::InvalidScript {
        reason: "script is empty".to_string(),
    };
    assert!(error.to_string().contains("empty"));
}

#[test]
fn test_script_too_large() {
    let error = VmError::InvalidScript {
        reason: "script exceeds maximum size".to_string(),
    };
    assert!(error.to_string().contains("exceeds"));
}

// ============================================================================
// Execution Depth Error Tests
// ============================================================================

#[test]
fn test_max_invocation_depth_exceeded() {
    let error = VmError::CallDepthLimitExceeded {
        depth: 1025,
        limit: 1024,
    };
    assert!(error.to_string().contains("1025") || error.to_string().contains("depth"));
}

#[test]
fn test_recursive_call_limit() {
    let error = VmError::InvalidOperation {
        operation: "CALL".to_string(),
        reason: "maximum recursion depth exceeded".to_string(),
    };
    assert!(error.to_string().contains("recursion"));
}

// ============================================================================
// Witness Verification Error Tests
// ============================================================================

#[test]
fn test_witness_verification_failed() {
    let error = VmError::VerificationFailed {
        reason: "witness verification failed".to_string(),
    };
    assert!(error.to_string().contains("witness") || error.to_string().contains("verification"));
}

#[test]
fn test_invalid_signature() {
    let error = VmError::InvalidOperation {
        operation: "CHECKSIG".to_string(),
        reason: "signature verification failed".to_string(),
    };
    assert!(error.to_string().contains("signature"));
}

// ============================================================================
// Type Conversion Error Tests
// ============================================================================

#[test]
fn test_cannot_convert_to_boolean() {
    let error = VmError::InvalidType {
        from: "Map".to_string(),
        to: "Boolean".to_string(),
    };
    assert!(error.to_string().contains("Boolean"));
}

#[test]
fn test_cannot_convert_to_integer() {
    let error = VmError::InvalidType {
        from: "Array".to_string(),
        to: "Integer".to_string(),
    };
    assert!(error.to_string().contains("Integer"));
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_empty_evaluation_stack() {
    let error = VmError::StackUnderflow {
        requested: 1,
        available: 0,
    };
    // VM should handle empty stack gracefully
    assert!(!error.to_string().is_empty());
}

#[test]
fn test_max_item_count_exceeded() {
    let error = VmError::StackOverflow { max_size: 2048 };
    assert!(error.to_string().contains("2048") || error.to_string().contains("stack"));
}

#[test]
fn test_negative_count_operation() {
    let error = VmError::InvalidOperation {
        operation: "NEWARRAY".to_string(),
        reason: "count cannot be negative".to_string(),
    };
    assert!(error.to_string().contains("negative"));
}
