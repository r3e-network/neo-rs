use super::{builder::ScriptBuilder, utils::syscall_hash};
use crate::script::opcode::OpCode;

#[test]
fn push_bool_emits_expected_opcode() {
    let mut builder = ScriptBuilder::new();
    builder.push_bool(true);
    builder.push_bool(false);
    assert_eq!(
        builder.as_bytes(),
        &[OpCode::PushTrue as u8, OpCode::PushFalse as u8]
    );
}

#[test]
fn push_small_int_uses_single_opcode() {
    let mut builder = ScriptBuilder::new();
    builder.push_int(-1).push_int(0).push_int(5);
    assert_eq!(
        builder.as_bytes(),
        &[
            OpCode::PushM1 as u8,
            OpCode::Push0 as u8,
            OpCode::Push5 as u8
        ]
    );
}

#[test]
fn push_large_int_uses_pushdata() {
    let mut builder = ScriptBuilder::new();
    builder.push_int(1024);
    let script = builder.as_bytes();
    assert_eq!(script[0], OpCode::PushData1 as u8);
    assert_eq!(script[1], 2);
    assert_eq!(script[2..4], [0x00, 0x04]);
}

#[test]
fn push_string_round_trips() {
    let mut builder = ScriptBuilder::new();
    builder.push_string("neo-rs");
    let bytes = builder.into_bytes();
    assert_eq!(
        bytes,
        [&[OpCode::PushData1 as u8, 6u8][..], b"neo-rs"].concat()
    );
}

#[test]
fn push_syscall_writes_hash() {
    let mut builder = ScriptBuilder::new();
    builder.push_syscall("System.Runtime.Log");
    let bytes = builder.into_bytes();
    assert_eq!(bytes[0], OpCode::Syscall as u8);
    let hash = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    assert_eq!(hash, syscall_hash("System.Runtime.Log"));
}
