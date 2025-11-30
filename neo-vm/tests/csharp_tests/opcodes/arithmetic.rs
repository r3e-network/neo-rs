//! Arithmetic opcode tests
//!
//! Tests for arithmetic operations like ADD, SUB, MUL, DIV, etc.

use crate::csharp_tests::{resolve_test_dir, JsonTestRunner};

/// Test OpCodes Arithmetic category (matches C# TestOpCodesArithmetic)
#[test]
fn test_opcodes_arithmetic() {
    if let Some(test_path) = resolve_test_dir("OpCodes/Arithmetic") {
        let mut runner = JsonTestRunner::new();
        runner
            .test_json_directory(test_path.to_str().expect("valid UTF-8 path"))
            .unwrap();
    } else {
        eprintln!("C# test directory not found: OpCodes/Arithmetic");
    }
}
