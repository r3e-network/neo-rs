//! Stack manipulation opcode tests
//!
//! Tests for stack operations like DUP, SWAP, ROT, etc.

use crate::csharp_tests::{JsonTestRunner, resolve_test_dir};

/// Test OpCodes Stack category (matches C# TestOpCodesStack)
#[test]
fn test_opcodes_stack() {
    if let Some(test_path) = resolve_test_dir("OpCodes/Stack") {
        let mut runner = JsonTestRunner::new();
        runner
            .test_json_directory(test_path.to_str().expect("valid UTF-8 path"))
            .unwrap();
    } else {
        eprintln!("C# test directory not found: OpCodes/Stack");
    }
}
