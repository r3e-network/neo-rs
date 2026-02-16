//! Array opcode tests
//!
//! Tests for array operations such as PACK, UNPACK, PICKITEM, etc.

use crate::csharp_tests::{JsonTestRunner, resolve_test_dir};

#[test]
fn test_opcodes_arrays() {
    if let Some(test_path) = resolve_test_dir("OpCodes/Arrays") {
        let mut runner = JsonTestRunner::new();
        runner
            .test_json_directory(test_path.to_str().expect("valid UTF-8 path"))
            .unwrap();
    } else {
        eprintln!("C# test directory not found: OpCodes/Arrays");
    }
}
