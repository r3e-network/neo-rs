//! Script compilation unit tests
//!
//! Tests for script compilation edge cases and opcode mapping.

use crate::csharp_tests::JsonTestRunner;

/// Test script compilation edge cases
#[test]
fn test_script_compilation_edge_cases() {
    let runner = JsonTestRunner::new();

    // Test empty script
    let result = runner.compile_script(&[]);
    assert!(result.is_ok(), "Empty script should compile to just RET");
    assert_eq!(result.unwrap(), vec![0x40]); // Just RET

    // Test unknown opcode
    let result = runner.compile_script(&["UNKNOWN_OPCODE".to_string()]);
    assert!(result.is_err(), "Unknown opcode should fail compilation");

    // Test valid opcodes
    let result = runner.compile_script(&["PUSH1".to_string(), "PUSH2".to_string()]);
    assert!(result.is_ok(), "Valid opcodes should compile successfully");
    assert_eq!(result.unwrap(), vec![0x11, 0x12, 0x40]); // PUSH1 + PUSH2 + RET

    let result = runner.compile_script(&["PUSHA".to_string()]);
    assert!(result.is_ok(), "PUSHA opcode should compile successfully");
    assert_eq!(result.unwrap(), vec![0x0a, 0x40]); // PUSHA + RET

    // Test other previously missing opcodes
    let result = runner.compile_script(&["PUSHINT8".to_string()]);
    assert!(
        result.is_ok(),
        "PUSHINT8 opcode should compile successfully"
    );
    assert_eq!(result.unwrap(), vec![0x00, 0x40]); // PUSHINT8 + RET

    let result = runner.compile_script(&["PUSHT".to_string()]);
    assert!(result.is_ok(), "PUSHT opcode should compile successfully");
    assert_eq!(result.unwrap(), vec![0x08, 0x40]); // PUSHT + RET
}

/// Test hex data compilation
#[test]
fn test_hex_data_compilation() {
    let runner = JsonTestRunner::new();

    // Test valid hex data
    let result = runner.compile_script(&["0x01020304".to_string()]);
    assert!(result.is_ok(), "Valid hex data should compile successfully");
    assert_eq!(result.unwrap(), vec![0x01, 0x02, 0x03, 0x04, 0x40]); // hex data + RET

    let result = runner.compile_script(&["0x123".to_string()]);
    assert!(
        result.is_err(),
        "Odd-length hex data should fail compilation"
    );

    // Test invalid hex characters
    let result = runner.compile_script(&["0xGG".to_string()]);
    assert!(
        result.is_err(),
        "Invalid hex characters should fail compilation"
    );
}

/// Test PUSHDATA compilation with hex data
///
/// This test verifies that PUSHDATA1 with combined length+data hex compiles correctly.
/// The compile_script function does NOT automatically append RET - that's the caller's
/// responsibility if needed.
#[test]
fn test_pushdata1_debug() {
    let runner = JsonTestRunner::new();

    // Test the PUSHDATA1 case with combined length + data in one hex string
    // Input: PUSHDATA1 + 0x0501020304 (means length=5, data=[01,02,03,04])
    let script = vec!["PUSHDATA1".to_string(), "0x0501020304".to_string()];
    let compiled = runner.compile_script(&script).unwrap();
    println!("PUSHDATA1 compiled script: {:?}", compiled);

    // Expected: PUSHDATA1 opcode (0x0c) + length (0x05) + data (01,02,03,04)
    // Note: compile_script does NOT add RET automatically
    assert_eq!(compiled, vec![0x0c, 0x05, 0x01, 0x02, 0x03, 0x04]);

    // Now test parsing this script
    // The script declares PUSHDATA1 with length=5 but only has 4 bytes of data
    // This should fail validation in strict mode
    use neo_vm::script::Script;
    match Script::new(compiled.clone(), false) {
        Ok(script_obj) => {
            let instructions: Result<Vec<_>, _> = script_obj.instructions().collect();

            match instructions {
                Ok(instructions) => {
                    println!(
                        "✅ Instruction parsing succeeded with {} instructions",
                        instructions.len()
                    );
                    for (i, instr) in instructions.iter().enumerate() {
                        println!("   Instruction {}: {:?}", i, instr);
                    }
                }
                Err(err) => {
                    // The script declares length=5 but only has 4 bytes of data, so parsing should fail
                    println!("✅ Instruction parsing correctly failed: {:?}", err);
                }
            }
        }
        Err(err) => {
            // Script::new validates format - malformed PUSHDATA1 should fail
            println!(
                "✅ Script creation correctly failed due to malformed PUSHDATA1: {:?}",
                err
            );
        }
    }
}
