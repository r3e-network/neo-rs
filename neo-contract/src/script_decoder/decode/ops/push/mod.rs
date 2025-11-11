mod bool_int;
mod data;

use neo_core::script::OpCode;
use neo_vm::Instruction;

use super::super::error::ScriptDecodeError;
use super::super::helpers::{next_byte, push_fixed_int, read_bytes, read_le, unexpected_eof};

pub(super) fn apply_push(
    opcode: OpCode,
    bytes: &[u8],
    pc: &mut usize,
    program: &mut Vec<Instruction>,
) -> Result<bool, ScriptDecodeError> {
    if bool_int::handle_bool_int(opcode, program)? {
        return Ok(true);
    }

    data::handle_data(opcode, bytes, pc, program)
}
