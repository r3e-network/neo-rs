use alloc::vec::Vec;

use neo_core::script::OpCode;
use neo_vm::Instruction;

use crate::script_decoder::syscall::syscall_name_from_hash;

use super::super::super::error::ScriptDecodeError;
use super::super::helpers::read_le;

pub(super) fn apply_syscall(
    opcode: OpCode,
    bytes: &[u8],
    pc: &mut usize,
    program: &mut Vec<Instruction>,
) -> Result<bool, ScriptDecodeError> {
    if opcode != OpCode::Syscall {
        return Ok(false);
    }
    let hash = read_le(bytes, pc, 4, opcode)?;
    let name = syscall_name_from_hash(hash).ok_or(ScriptDecodeError::UnknownSyscall { hash })?;
    program.push(Instruction::Syscall(name));
    Ok(true)
}
