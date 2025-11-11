use alloc::vec::Vec;

use neo_core::script::OpCode;
use neo_vm::Instruction;

pub(super) fn apply_control(opcode: OpCode, program: &mut Vec<Instruction>) -> bool {
    match opcode {
        OpCode::Return => {
            program.push(Instruction::Return);
            true
        }
        OpCode::Nop => true,
        _ => false,
    }
}
