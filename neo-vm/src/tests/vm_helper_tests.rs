// Converted from C# Neo.UnitTests.VM.UT_Helper
use crate::op_code::OpCode;
use crate::script_builder::ScriptBuilder;

#[test]
fn test_emit() {
    let mut sb = ScriptBuilder::new();
    sb.emit(OpCode::PUSH0 as u8);
    let result = sb.to_array();
    assert_eq!(result, vec![OpCode::PUSH0 as u8]);
}

#[test]
fn test_emit_push_int() {
    let mut sb = ScriptBuilder::new();
    sb.emit_push_int(42);
    let result = sb.to_array();

    // Should emit PUSH1 followed by the value 42
    assert!(!result.is_empty());
    assert_eq!(result[0], OpCode::PUSHINT8 as u8);
    assert_eq!(result[1], 42);
}

#[test]
fn test_emit_push_bool() {
    let mut sb = ScriptBuilder::new();
    sb.emit_push_bool(true);
    let result = sb.to_array();
    assert_eq!(result, vec![OpCode::PUSHT as u8]);

    let mut sb = ScriptBuilder::new();
    sb.emit_push_bool(false);
    let result = sb.to_array();
    assert_eq!(result, vec![OpCode::PUSHF as u8]);
}

#[test]
fn test_emit_push_string() {
    let mut sb = ScriptBuilder::new();
    sb.emit_push_string("hello");
    let result = sb.to_array();

    // Should emit PUSHDATA1 followed by length and string bytes
    assert!(!result.is_empty());
    assert_eq!(result[0], OpCode::PUSHDATA1 as u8);
    assert_eq!(result[1], 5); // length of "hello"
}

#[test]
fn test_emit_push_bytes() {
    let mut sb = ScriptBuilder::new();
    let data = vec![1, 2, 3, 4, 5];
    sb.emit_push(&data);
    let result = sb.to_array();

    assert!(!result.is_empty());
    assert_eq!(result[0], OpCode::PUSHDATA1 as u8);
    assert_eq!(result[1], 5); // length
    assert_eq!(&result[2..7], &[1, 2, 3, 4, 5]);
}

#[test]
fn test_emit_syscall() {
    let mut sb = ScriptBuilder::new();
    sb.emit_syscall("System.Runtime.CheckWitness").unwrap();
    let result = sb.to_array();

    assert!(!result.is_empty());
    assert_eq!(result[0], OpCode::SYSCALL as u8);
}

#[test]
fn test_emit_jump() {
    let mut sb = ScriptBuilder::new();
    sb.emit_jump(OpCode::JMP, 10).unwrap();
    let result = sb.to_array();

    assert!(!result.is_empty());
    assert_eq!(result[0], OpCode::JMP as u8);
}
