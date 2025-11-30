//! Bitwise / logical opcode tests.

use crate::csharp_tests::{resolve_test_dir, JsonTestRunner};

/// Test bitwise logic opcodes (AND, OR, XOR, INVERT, EQUAL, NOTEQUAL)
///
/// These tests run against the C# Neo.VM.Tests JSON fixtures for bitwise operations.
#[test]
fn test_opcodes_bitwise_logic() {
    if let Some(test_path) = resolve_test_dir("OpCodes/BitwiseLogic") {
        let mut runner = JsonTestRunner::new();
        runner
            .test_json_directory(test_path.to_str().expect("valid UTF-8 path"))
            .unwrap();
    } else {
        eprintln!("C# test directory not found: OpCodes/BitwiseLogic");
    }
}
