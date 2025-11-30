#![allow(clippy::mutable_key_type, clippy::format_collect)]
//! Type conversion compatibility tests
//!
//! Tests to verify the Rust VM's type conversion behavior matches the C# implementation exactly.

use neo_vm::{
    execution_engine::{ExecutionEngine, VMState},
    op_code::OpCode,
    script::Script,
    script_builder::ScriptBuilder,
    stack_item::{StackItem, StackItemType},
};
use std::collections::BTreeMap;

/// Tests conversion from boolean to other types
#[test]
fn test_boolean_conversions() {
    let test_cases = vec![
        (
            vec![
                OpCode::PUSHT as u8,
                OpCode::CONVERT as u8,
                StackItemType::Integer as u8,
            ],
            "1", // C# VM converts true to integer 1
        ),
        (
            vec![
                OpCode::PUSHF as u8,
                OpCode::CONVERT as u8,
                StackItemType::Integer as u8,
            ],
            "0", // C# VM converts false to integer 0
        ),
        (
            vec![
                OpCode::PUSHT as u8,
                OpCode::CONVERT as u8,
                StackItemType::ByteString as u8,
            ],
            "01", // C# VM converts true to "01" hex string
        ),
        (
            vec![
                OpCode::PUSHF as u8,
                OpCode::CONVERT as u8,
                StackItemType::ByteString as u8,
            ],
            "00", // C# VM converts false to "00" hex string
        ),
    ];

    for (script_bytes, expected_result) in test_cases {
        // Create the execution engine
        let script = Script::new(script_bytes, false).unwrap();
        let mut engine = ExecutionEngine::new(None);

        // Execute the script
        let _ = engine.load_script(script, -1, 0);
        let _ = engine.execute();

        // Verify execution state
        assert_eq!(engine.state(), VMState::HALT, "VM execution failed");

        let result_stack = engine.result_stack();
        assert_eq!(result_stack.len(), 1, "Expected one item on stack");

        let result = result_stack.iter().next().unwrap();
        match result {
            item if item.stack_item_type() == StackItemType::Integer => match item.as_int() {
                Ok(value) => assert_eq!(value.to_string(), expected_result),
                Err(_) => panic!("Failed to convert to integer"),
            },
            item if item.stack_item_type() == StackItemType::ByteString => match item.as_bytes() {
                Ok(bytes) => {
                    let hex_string = bytes
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<String>();
                    assert_eq!(hex_string, expected_result);
                }
                Err(_) => panic!("Failed to convert to byte string"),
            },
            _ => panic!("Unexpected stack item type"),
        }
    }
}

/// Tests conversion from integer to other types
#[test]
fn test_integer_conversions() {
    let test_cases = vec![
        (
            vec![
                OpCode::PUSH1 as u8,
                OpCode::CONVERT as u8,
                StackItemType::Boolean as u8,
            ],
            "true", // C# VM converts non-zero to true
        ),
        (
            vec![
                OpCode::PUSH0 as u8,
                OpCode::CONVERT as u8,
                StackItemType::Boolean as u8,
            ],
            "false", // C# VM converts zero to false
        ),
        (
            vec![
                OpCode::PUSHINT8 as u8,
                0xFF, // Push 255
                OpCode::CONVERT as u8,
                StackItemType::ByteString as u8,
            ],
            "ff", // C# VM converts 255 to "ff" hex string (little endian)
        ),
        (
            vec![
                OpCode::PUSHINT16 as u8,
                0x00,
                0x01, // Push 256 (little-endian)
                OpCode::CONVERT as u8,
                StackItemType::ByteString as u8,
            ],
            "0001", // C# VM converts 256 to "0001" hex string (little endian)
        ),
    ];

    for (script_bytes, expected_result) in test_cases {
        // Create the execution engine
        let script = Script::new(script_bytes, false).unwrap();
        let mut engine = ExecutionEngine::new(None);

        // Execute the script
        let _ = engine.load_script(script, -1, 0);
        let _ = engine.execute();

        // Verify execution state
        assert_eq!(engine.state(), VMState::HALT, "VM execution failed");

        let result_stack = engine.result_stack();
        assert_eq!(result_stack.len(), 1, "Expected one item on stack");

        let result = result_stack.iter().next().unwrap();
        match result {
            item if item.stack_item_type() == StackItemType::Boolean => match item.as_bool() {
                Ok(value) => assert_eq!(value.to_string(), expected_result),
                Err(_) => panic!("Failed to convert to boolean"),
            },
            item if item.stack_item_type() == StackItemType::ByteString => match item.as_bytes() {
                Ok(bytes) => {
                    let hex_string = bytes
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<String>();
                    assert_eq!(hex_string, expected_result);
                }
                Err(_) => panic!("Failed to convert to byte string"),
            },
            _ => panic!("Unexpected stack item type"),
        }
    }
}

/// Tests conversion from byte strings to other types
#[test]
fn test_byte_string_conversions() {
    fn build_conversion_script(target: StackItemType) -> Script {
        let mut builder = ScriptBuilder::new();
        builder.emit_instruction(OpCode::CONVERT, &[target.to_byte()]);
        builder.to_script()
    }

    let test_cases = vec![
        // Empty ByteString to Integer should be 0
        (
            Vec::new(),
            StackItemType::Integer,
            "0", // C# VM converts empty string to 0
        ),
        // Single byte 0x01 to Integer should be 1
        (
            vec![0x01],
            StackItemType::Integer,
            "1", // C# VM converts 0x01 to 1
        ),
        (
            vec![0x00, 0x01],
            StackItemType::Integer,
            "256", // C# VM converts 0x0001 to 256
        ),
        // ByteString with non-zero bytes to Boolean should be true
        (
            vec![0x01],
            StackItemType::Boolean,
            "true", // C# VM converts non-empty to true
        ),
        // Empty ByteString to Boolean should be false
        (
            Vec::new(),
            StackItemType::Boolean,
            "false", // C# VM converts empty to false
        ),
    ];

    for (data, target_type, expected_result) in test_cases {
        let script = build_conversion_script(target_type);
        let mut engine = ExecutionEngine::new(None);

        // Execute the script
        let _ = engine.load_script(script, -1, 0);
        engine
            .push(StackItem::from_byte_string(data.clone()))
            .expect("push should succeed");
        let _ = engine.execute();

        // Verify execution state
        assert_eq!(engine.state(), VMState::HALT, "VM execution failed");

        let result_stack = engine.result_stack();
        assert_eq!(result_stack.len(), 1, "Expected one item on stack");

        let result = result_stack.iter().next().unwrap();
        match target_type {
            StackItemType::Integer => {
                let value = result.as_int().expect("Failed to convert to integer");
                assert_eq!(value.to_string(), expected_result);
            }
            StackItemType::Boolean => {
                let value = result.as_bool().expect("Failed to convert to boolean");
                assert_eq!(value.to_string(), expected_result);
            }
            _ => panic!("Unexpected target type"),
        };
    }
}

/// Tests array and struct type conversions
#[test]
fn test_array_and_struct_conversions() {
    let base_array = StackItem::from_array(vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(3),
    ]);

    let test_cases = vec![
        (
            {
                let mut builder = ScriptBuilder::new();
                builder
                    .emit_push_stack_item(base_array.clone())
                    .expect("script should build");
                builder.emit_instruction(OpCode::CONVERT, &[StackItemType::Struct.to_byte()]);
                builder.to_script()
            },
            vec!["3", "2", "1"], // Order reflects PACK behaviour in ScriptBuilder
        ),
        (
            {
                let mut builder = ScriptBuilder::new();
                builder
                    .emit_push_stack_item(base_array.clone())
                    .expect("script should build");
                builder.emit_instruction(OpCode::CONVERT, &[StackItemType::Struct.to_byte()]);
                builder.emit_instruction(OpCode::CONVERT, &[StackItemType::Array.to_byte()]);
                builder.to_script()
            },
            vec!["3", "2", "1"], // C# VM converts struct to array with same elements
        ),
    ];

    for (script_bytes, expected_result) in test_cases {
        // Create the execution engine
        let script = script_bytes;
        let mut engine = ExecutionEngine::new(None);

        // Execute the script
        let _ = engine.load_script(script, -1, 0);
        let _ = engine.execute();

        // Verify execution state
        assert_eq!(engine.state(), VMState::HALT, "VM execution failed");

        let result_stack = engine.result_stack();
        assert!(!result_stack.is_empty(), "Expected result on stack");

        let result = result_stack.iter().next().unwrap();
        let array = result.as_array().expect("Failed to convert to array");

        // Verify array contents
        assert_eq!(array.len(), expected_result.len(), "Array length mismatch");

        let actual_values: Vec<String> = array
            .iter()
            .map(|value| value.as_int().unwrap().to_string())
            .collect();
        assert_eq!(actual_values, expected_result);
    }
}

/// Tests invalid conversion handling
#[test]
fn test_invalid_conversions() {
    let array_to_int = {
        let mut builder = ScriptBuilder::new();
        builder
            .emit_push_stack_item(StackItem::from_array(vec![StackItem::from_int(1)]))
            .expect("script should build");
        builder.emit_instruction(OpCode::CONVERT, &[StackItemType::Integer.to_byte()]);
        builder.to_script()
    };

    let map_to_bool = {
        let mut builder = ScriptBuilder::new();
        builder
            .emit_push_stack_item(StackItem::from_map(BTreeMap::new()))
            .expect("script should build");
        builder.emit_instruction(OpCode::CONVERT, &[StackItemType::Boolean.to_byte()]);
        builder.to_script()
    };

    let test_cases = vec![
        (array_to_int, VMState::FAULT, None),
        (map_to_bool, VMState::HALT, Some("false")),
    ];

    for (script, expected_state, expected_bool) in test_cases {
        let mut engine = ExecutionEngine::new(None);
        let _ = engine.load_script(script, -1, 0);
        let _ = engine.execute();

        assert_eq!(engine.state(), expected_state, "Unexpected VM state");

        match expected_state {
            VMState::FAULT => assert!(
                engine.uncaught_exception().is_some(),
                "Should have exception"
            ),
            VMState::HALT => {
                if let Some(expected) = expected_bool {
                    let result = engine
                        .result_stack()
                        .iter()
                        .next()
                        .expect("result should exist");
                    let value = result.as_bool().expect("boolean conversion");
                    assert_eq!(value.to_string(), expected);
                }
            }
            _ => {}
        }
    }
}

/// Tests complex nested conversions
#[test]
fn test_complex_conversions() {
    let value_array = StackItem::from_array(vec![StackItem::from_int(2), StackItem::from_int(3)]);
    let mut map_entries = BTreeMap::new();
    map_entries.insert(StackItem::from_int(1), value_array.clone());
    let map_item = StackItem::from_map(map_entries);

    let script = {
        let mut builder = ScriptBuilder::new();
        builder
            .emit_push_stack_item(map_item.clone())
            .expect("script should build");
        builder
            .emit_push_stack_item(value_array)
            .expect("script should build");
        builder.emit_instruction(OpCode::CONVERT, &[StackItemType::Struct.to_byte()]);
        builder.to_script()
    };

    // Create the execution engine
    let mut engine = ExecutionEngine::new(None);

    // Execute the script
    let _ = engine.load_script(script, -1, 0);
    let _ = engine.execute();

    // Verify execution state
    assert_eq!(engine.state(), VMState::HALT, "VM execution failed");

    let result_stack = engine.result_stack();
    assert!(result_stack.len() >= 2, "Expected struct and map on stack");

    let stack_items: Vec<_> = result_stack.iter().collect();

    let struct_item = stack_items
        .iter()
        .find(|item| item.stack_item_type() == StackItemType::Struct)
        .expect("Expected Struct");
    let struct_items = struct_item.as_array().expect("Failed to get struct items");
    assert_eq!(struct_items.len(), 2, "Struct should have 2 items");

    let struct_values: Vec<String> = struct_items
        .iter()
        .map(|item| item.as_int().unwrap().to_string())
        .collect();
    assert_eq!(struct_values, vec!["3".to_string(), "2".to_string()]);

    assert!(
        stack_items
            .iter()
            .any(|item| item.stack_item_type() == StackItemType::Map),
        "Expected Map"
    );
}
