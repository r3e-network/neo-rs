use neo_core::script::OpCode;
use neo_vm::Instruction;

use super::super::super::error::ScriptDecodeError;

pub(super) fn handle_bool_int(
    opcode: OpCode,
    program: &mut Vec<Instruction>,
) -> Result<bool, ScriptDecodeError> {
    Ok(match opcode {
        OpCode::PushTrue => {
            program.push(Instruction::PushBool(true));
            true
        }
        OpCode::PushFalse => {
            program.push(Instruction::PushBool(false));
            true
        }
        OpCode::PushM1 => {
            program.push(Instruction::PushInt(-1));
            true
        }
        OpCode::Push0 => push_small_int(program, 0),
        OpCode::Push1 => push_small_int(program, 1),
        OpCode::Push2 => push_small_int(program, 2),
        OpCode::Push3 => push_small_int(program, 3),
        OpCode::Push4 => push_small_int(program, 4),
        OpCode::Push5 => push_small_int(program, 5),
        OpCode::Push6 => push_small_int(program, 6),
        OpCode::Push7 => push_small_int(program, 7),
        OpCode::Push8 => push_small_int(program, 8),
        OpCode::Push9 => push_small_int(program, 9),
        OpCode::Push10 => push_small_int(program, 10),
        OpCode::Push11 => push_small_int(program, 11),
        OpCode::Push12 => push_small_int(program, 12),
        OpCode::Push13 => push_small_int(program, 13),
        OpCode::Push14 => push_small_int(program, 14),
        OpCode::Push15 => push_small_int(program, 15),
        OpCode::Push16 => push_small_int(program, 16),
        _ => false,
    })
}

fn push_small_int(program: &mut Vec<Instruction>, value: i64) -> bool {
    program.push(Instruction::PushInt(value));
    true
}
