use alloc::vec::Vec;

use neo_core::script::OpCode;
use neo_vm::Instruction;

use super::super::super::error::ScriptDecodeError;

pub(super) fn apply_arithmetic(opcode: OpCode, program: &mut Vec<Instruction>) -> bool {
    let instr = match opcode {
        OpCode::Add => Instruction::Add,
        OpCode::Sub => Instruction::Sub,
        OpCode::Mul => Instruction::Mul,
        OpCode::Div => Instruction::Div,
        OpCode::Mod => Instruction::Mod,
        OpCode::Inc => Instruction::Inc,
        OpCode::Dec => Instruction::Dec,
        OpCode::Negate => Instruction::Negate,
        OpCode::Equal => Instruction::Equal,
        OpCode::NotEqual => Instruction::NotEqual,
        OpCode::Gt => Instruction::Greater,
        OpCode::Ge => Instruction::GreaterOrEqual,
        OpCode::Lt => Instruction::Less,
        OpCode::Le => Instruction::LessOrEqual,
        OpCode::Not => Instruction::Not,
        OpCode::And | OpCode::BoolAnd => Instruction::And,
        OpCode::Or | OpCode::BoolOr => Instruction::Or,
        OpCode::Xor => Instruction::Xor,
        OpCode::Shl => Instruction::Shl,
        OpCode::Shr => Instruction::Shr,
        _ => return false,
    };
    program.push(instr);
    true
}
