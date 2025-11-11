use alloc::vec::Vec;

use neo_core::script::OpCode;
use neo_vm::Instruction;

use super::super::error::ScriptDecodeError;

pub(crate) fn push_fixed_int(
    program: &mut Vec<Instruction>,
    bytes: &[u8],
    pc: &mut usize,
    opcode: OpCode,
    width: usize,
) -> Result<(), ScriptDecodeError> {
    let data = read_bytes(bytes, pc, width, opcode)?;
    let value = match width {
        1 => i8::from_le_bytes([data[0]]) as i64,
        2 => i16::from_le_bytes([data[0], data[1]]) as i64,
        4 => i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as i64,
        8 => i64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    };
    program.push(Instruction::PushInt(value));
    Ok(())
}

pub(crate) fn read_le(
    bytes: &[u8],
    pc: &mut usize,
    width: usize,
    opcode: OpCode,
) -> Result<u32, ScriptDecodeError> {
    let data = read_bytes(bytes, pc, width, opcode)?;
    let mut value = 0u32;
    for (shift, byte) in data.iter().enumerate() {
        value |= (*byte as u32) << (shift * 8);
    }
    Ok(value)
}

pub(crate) fn read_bytes(
    bytes: &[u8],
    pc: &mut usize,
    len: usize,
    opcode: OpCode,
) -> Result<Vec<u8>, ScriptDecodeError> {
    if bytes.len().saturating_sub(*pc) < len {
        return Err(ScriptDecodeError::UnexpectedEof {
            opcode,
            offset: *pc,
        });
    }
    let data = bytes[*pc..*pc + len].to_vec();
    *pc += len;
    Ok(data)
}

pub(crate) fn next_byte(bytes: &[u8], pc: &mut usize) -> Option<u8> {
    if *pc >= bytes.len() {
        return None;
    }
    let value = bytes[*pc];
    *pc += 1;
    Some(value)
}

pub(crate) fn unexpected_eof(opcode: OpCode, offset: usize) -> ScriptDecodeError {
    ScriptDecodeError::UnexpectedEof { opcode, offset }
}
