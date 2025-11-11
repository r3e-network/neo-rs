use alloc::vec::Vec;

use neo_core::script::{OpCode, Script};
use neo_vm::Instruction;

use super::ops::{apply_arithmetic, apply_control, apply_push, apply_stack, apply_syscall};

use super::super::error::ScriptDecodeError;

pub fn decode_script(script: &Script) -> Result<Vec<Instruction>, ScriptDecodeError> {
    let bytes = script.as_bytes();
    let mut pc = 0;
    let mut program = Vec::new();

    while pc < bytes.len() {
        let opcode = OpCode::try_from(bytes[pc]).map_err(|_| ScriptDecodeError::InvalidOpcode {
            byte: bytes[pc],
            offset: pc,
        })?;
        pc += 1;

        if apply_push(opcode, bytes, &mut pc, &mut program)? {
            continue;
        }
        if apply_stack(opcode, &mut program) {
            continue;
        }
        if apply_arithmetic(opcode, &mut program) {
            continue;
        }
        if apply_control(opcode, &mut program) {
            continue;
        }
        if apply_syscall(opcode, bytes, &mut pc, &mut program)? {
            continue;
        }

        return Err(ScriptDecodeError::UnsupportedOpcode(opcode));
    }

    Ok(program)
}
