//! Exception handling compatibility tests
//!
//! Tests to verify the Rust VM's exception handling matches the C# implementation exactly.

use neo_vm::{
    exception_handling::{ExceptionHandlingContext, ExceptionHandlingState},
    execution_engine::{ExecutionEngine, VMState},
    op_code::OpCode,
    script::Script,
};

/// Tests the exception handling context creation to match C# behavior
#[test]
fn test_exception_handling_context_creation() {
    // Create an exception handling context like the C# implementation would
    let context = ExceptionHandlingContext::new(10, 20, 15, 25, 30);

    // Verify the properties match C# implementation expectations
    assert_eq!(context.try_start, 10);
    assert_eq!(context.try_end, 20);
    assert_eq!(context.catch_start, 15);
    assert_eq!(context.finally_start, 25);
    assert_eq!(context.end_offset, 30);
    assert_eq!(context.state, ExceptionHandlingState::Try);

    // Verify the C#-like helper properties
    assert!(context.has_catch());
    assert!(context.has_finally());

    // Test with max values to simulate unavailable blocks (instead of -1)
    let context_no_catch = ExceptionHandlingContext::new(10, 20, usize::MAX, 25, 30);
    let context_no_finally = ExceptionHandlingContext::new(10, 20, 15, usize::MAX, 30);
    let context_no_handlers = ExceptionHandlingContext::new(10, 20, usize::MAX, usize::MAX, 30);

    assert!(!context_no_catch.has_catch());
    assert!(context_no_catch.has_finally());

    assert!(context_no_finally.has_catch());
    assert!(!context_no_finally.has_finally());

    assert!(!context_no_handlers.has_catch());
    assert!(!context_no_handlers.has_finally());
}

/// Tests the TRY/CATCH/FINALLY flow to match C# behavior
#[test]
fn test_try_catch_finally_flow() {
    // Create a script that simulates try/catch/finally
    // This script implements the following pseudocode:
    // try {
    //   PUSH1
    //   THROW  // Throws an exception
    // } catch {
    //   PUSH2  // This should execute due to the exception
    // } finally {
    //   PUSH3  // This should always execute
    // }

    // Opcodes for the script
    let script_bytes = vec![
        // TRY with catch at offset 7 (PUSH2) and finally at offset 9 (PUSH3)
        OpCode::TRY as u8,
        0x07,
        0x09,
        // Try block:
        OpCode::PUSH1 as u8,
        OpCode::THROW as u8,
        // ENDTRY to mark the end of the try block
        OpCode::ENDTRY as u8,
        // Catch block:
        OpCode::PUSH2 as u8,
        // Finally block:
        OpCode::PUSH3 as u8,
        // End of the script
        OpCode::RET as u8,
    ];

    // Create the execution engine
    let script = Script::new(script_bytes, false).expect("Failed to create script");
    let mut engine = ExecutionEngine::new(None);

    // Execute the script
    let _ = engine.load_script(script, 0, 0);
    let _ = engine.execute();

    // The script should have successfully executed
    assert_eq!(engine.state(), VMState::HALT);

    // The result stack should contain PUSH2 (from catch) and PUSH3 (from finally)
    let result_stack = engine.result_stack();
    assert_eq!(result_stack.len(), 2);

    // Verify the stack contents match what would happen in C#
    let stack_items: Vec<_> = (0..result_stack.len())
        .map(|i| result_stack.peek(i as isize).unwrap())
        .collect();

    // First item should be 3 (from finally block)
    let top_item = &stack_items[0];
    match top_item.as_int() {
        Ok(value) => assert_eq!(value.to_string(), "3"),
        Err(_) => panic!("Expected integer value on stack"),
    }

    // Second item should be 2 (from catch block)
    let second_item = &stack_items[1];
    match second_item.as_int() {
        Ok(value) => assert_eq!(value.to_string(), "2"),
        Err(_) => panic!("Expected integer value on stack"),
    }
}

/// Tests exception propagation across call frames to match C# behavior
#[test]
fn test_exception_propagation() {
    // Create a script that simulates exception propagation across call frames
    // This script implements:
    // try {
    //   CALL function_that_throws
    // } catch {
    //   PUSH5  // This should execute when exception propagates
    // }
    //
    // function_that_throws:
    //   PUSH1
    //   THROW

    // Opcodes for the script
    let script_bytes = vec![
        // TRY with catch at offset 7 and no finally (use large value instead of -1)
        OpCode::TRY as u8,
        0x07,
        0xFF, // 0xFF means no finally
        // Try block: Call function that throws
        OpCode::CALL as u8,
        0x0A, // Call offset 10
        // ENDTRY to mark the end of the try block
        OpCode::ENDTRY as u8,
        // Catch block:
        OpCode::PUSH5 as u8,
        // End of main:
        OpCode::RET as u8,
        // Function that throws (at offset 10):
        OpCode::PUSH1 as u8,
        OpCode::THROW as u8,
        OpCode::RET as u8, // Never reached
    ];

    // Create the execution engine
    let script = Script::new(script_bytes, false).expect("Failed to create script");
    let mut engine = ExecutionEngine::new(None);

    // Execute the script
    let _ = engine.load_script(script, 0, 0);
    let _ = engine.execute();

    // The script should have successfully executed
    assert_eq!(engine.state(), VMState::HALT);

    // The result stack should contain PUSH5 from the catch block
    let result_stack = engine.result_stack();
    assert_eq!(result_stack.len(), 1);

    // Verify the stack item is 5
    let stack_item = result_stack.peek(0).unwrap();
    match stack_item.as_int() {
        Ok(value) => assert_eq!(value.to_string(), "5"),
        Err(_) => panic!("Expected integer value on stack"),
    }
}

/// Tests that finally blocks always execute, even when no exception occurs
#[test]
fn test_finally_always_executes() {
    // Create a script that exercises finally blocks
    // This script implements:
    // try {
    //   PUSH1  // No exception
    // } finally {
    //   PUSH2  // Should always execute
    // }

    // Opcodes for the script
    let script_bytes = vec![
        // TRY with no catch (0xFF) and finally at offset 6
        OpCode::TRY as u8,
        0xFF,
        0x06,
        // Try block:
        OpCode::PUSH1 as u8,
        // ENDTRY to mark the end of the try block
        OpCode::ENDTRY as u8,
        // Finally block:
        OpCode::PUSH2 as u8,
        // End of the script
        OpCode::RET as u8,
    ];

    // Create the execution engine
    let script = Script::new(script_bytes, false).expect("Failed to create script");
    let mut engine = ExecutionEngine::new(None);

    // Execute the script
    let _ = engine.load_script(script, 0, 0);
    let _ = engine.execute();

    // The script should have successfully executed
    assert_eq!(engine.state(), VMState::HALT);

    // The result stack should contain PUSH1 (from try) and PUSH2 (from finally)
    let result_stack = engine.result_stack();
    assert_eq!(result_stack.len(), 2);

    // Verify the stack contents
    let stack_items: Vec<_> = (0..result_stack.len())
        .map(|i| result_stack.peek(i as isize).unwrap())
        .collect();

    // First item should be 2 (from finally block)
    let top_item = &stack_items[0];
    match top_item.as_int() {
        Ok(value) => assert_eq!(value.to_string(), "2"),
        Err(_) => panic!("Expected integer value on stack"),
    }

    // Second item should be 1 (from try block)
    let second_item = &stack_items[1];
    match second_item.as_int() {
        Ok(value) => assert_eq!(value.to_string(), "1"),
        Err(_) => panic!("Expected integer value on stack"),
    }
}

/// Tests that the VM state transitions correctly during exception handling
#[test]
fn test_vm_state_transitions() {
    // Test an unhandled exception
    // This should transition the VM to FAULT state

    // Opcodes for the script: Just PUSH1 and THROW with no try/catch
    let script_bytes = vec![OpCode::PUSH1 as u8, OpCode::THROW as u8];

    // Create the execution engine
    let script = Script::new(script_bytes, false).expect("Failed to create script");
    let mut engine = ExecutionEngine::new(None);

    // Execute the script
    let _ = engine.load_script(script, 0, 0);
    let _ = engine.execute();

    // The VM should be in FAULT state due to the unhandled exception
    assert_eq!(engine.state(), VMState::FAULT);

    // There should be an uncaught exception
    assert!(engine.uncaught_exception().is_some());
}
