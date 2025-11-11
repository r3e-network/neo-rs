use alloc::vec::Vec;

use neo_core::script::OpCode;
use neo_vm::Instruction;

pub(super) fn apply_stack(opcode: OpCode, program: &mut Vec<Instruction>) -> bool {
    let instr = match opcode {
        OpCode::Drop => Instruction::Drop,
        OpCode::Dup => Instruction::Dup(0),
        OpCode::Over => Instruction::Over,
        OpCode::Swap => Instruction::Swap(1),
        OpCode::Roll => Instruction::Roll(1),
        _ => return false,
    };
    program.push(instr);
    true
}
