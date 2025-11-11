use neo_core::script::OpCode;
use neo_vm::Instruction;

use super::super::super::error::ScriptDecodeError;
use super::super::helpers::{next_byte, push_fixed_int, read_bytes, read_le, unexpected_eof};

pub(super) fn handle_data(
    opcode: OpCode,
    bytes: &[u8],
    pc: &mut usize,
    program: &mut Vec<Instruction>,
) -> Result<bool, ScriptDecodeError> {
    let handled = match opcode {
        OpCode::PushData1 => {
            let len = next_byte(bytes, pc)
                .ok_or_else(|| unexpected_eof(opcode, pc.saturating_sub(1)))?
                as usize;
            push_bytes(program, bytes, pc, len, opcode)?;
            true
        }
        OpCode::PushData2 => {
            let len = read_le(bytes, pc, 2, opcode)? as usize;
            push_bytes(program, bytes, pc, len, opcode)?;
            true
        }
        OpCode::PushData4 => {
            let len = read_le(bytes, pc, 4, opcode)? as usize;
            push_bytes(program, bytes, pc, len, opcode)?;
            true
        }
        OpCode::PushInt8 => {
            push_fixed_int(program, bytes, pc, opcode, 1)?;
            true
        }
        OpCode::PushInt16 => {
            push_fixed_int(program, bytes, pc, opcode, 2)?;
            true
        }
        OpCode::PushInt32 => {
            push_fixed_int(program, bytes, pc, opcode, 4)?;
            true
        }
        OpCode::PushInt64 => {
            push_fixed_int(program, bytes, pc, opcode, 8)?;
            true
        }
        OpCode::PushInt128 | OpCode::PushInt256 => {
            let width = if opcode == OpCode::PushInt128 { 16 } else { 32 };
            let data = read_bytes(bytes, pc, width, opcode)?;
            program.push(Instruction::PushBytes(data));
            true
        }
        _ => false,
    };
    Ok(handled)
}

fn push_bytes(
    program: &mut Vec<Instruction>,
    bytes: &[u8],
    pc: &mut usize,
    len: usize,
    opcode: OpCode,
) -> Result<(), ScriptDecodeError> {
    let data = read_bytes(bytes, pc, len, opcode)?;
    program.push(Instruction::PushBytes(data));
    Ok(())
}
