//! Slot opcode tests covering INITSSLOT, LDSFLD, STSFLD, etc.

use crate::csharp_tests::{JsonTestRunner, resolve_test_dir};

#[test]
fn test_opcodes_slot() {
    if let Some(test_path) = resolve_test_dir("OpCodes/Slot") {
        let mut runner = JsonTestRunner::new();
        runner
            .test_json_directory(test_path.to_str().expect("valid UTF-8 path"))
            .unwrap();
    } else {
        eprintln!("C# test directory not found: OpCodes/Slot");
    }
}
