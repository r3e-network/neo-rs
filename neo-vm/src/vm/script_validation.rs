//! Shared NeoVM script parsing and structural validation.

use alloc::{collections::BTreeSet, format, string::String, string::ToString, vec::Vec};

use super::{Instruction, OpCode};
use crate::StackItemType;

/// Result type for script validation operations.
pub type ValidationResult<T> = Result<T, String>;

/// Validated NeoVM script metadata.
#[derive(Debug, Clone)]
pub struct ValidatedScript {
    instruction_offsets: BTreeSet<usize>,
}

impl ValidatedScript {
    /// Returns true when the byte offset starts a parsed instruction.
    #[must_use]
    pub fn has_instruction_at(&self, offset: usize) -> bool {
        self.instruction_offsets.contains(&offset)
    }
}

/// Parsed instruction metadata used by script disassembly and validation callers.
pub type ScriptInstruction = Instruction;

/// Validates NeoVM bytecode and rejects invalid control-flow/type operands.
pub fn validate_strict_script(script: &[u8]) -> ValidationResult<()> {
    validate_script(script, true).map(|_| ())
}

/// Validates NeoVM bytecode and returns instruction offsets for ABI checks.
pub fn validate_script(script: &[u8], strict: bool) -> ValidationResult<ValidatedScript> {
    let instructions = parse_script_instructions(script)?;
    let instruction_offsets = instructions
        .iter()
        .map(Instruction::pointer)
        .collect::<BTreeSet<_>>();

    if strict {
        for instruction in &instructions {
            validate_instruction(instruction, &instruction_offsets)?;
        }
    }

    Ok(ValidatedScript {
        instruction_offsets,
    })
}

/// Parses all instructions in a NeoVM bytecode script.
pub fn parse_script_instructions(script: &[u8]) -> ValidationResult<Vec<ScriptInstruction>> {
    let mut position = 0;
    let mut instructions = Vec::new();

    while position < script.len() {
        let instruction =
            Instruction::parse(script, position).map_err(|error| error.to_string())?;
        position = position
            .checked_add(instruction.size())
            .ok_or_else(|| "instruction position overflow".to_string())?;
        instructions.push(instruction);
    }

    Ok(instructions)
}

/// Calculates the absolute target for a jump-like instruction.
pub fn instruction_jump_target(instruction: &Instruction) -> ValidationResult<usize> {
    let offset = match instruction.opcode() {
        OpCode::JMP
        | OpCode::JMPIF
        | OpCode::JMPIFNOT
        | OpCode::JMPEQ
        | OpCode::JMPNE
        | OpCode::JMPGT
        | OpCode::JMPGE
        | OpCode::JMPLT
        | OpCode::JMPLE
        | OpCode::CALL
        | OpCode::ENDTRY => i64::from(read_i8(instruction, 0)?),
        OpCode::PUSHA
        | OpCode::JMP_L
        | OpCode::JMPIF_L
        | OpCode::JMPIFNOT_L
        | OpCode::JMPEQ_L
        | OpCode::JMPNE_L
        | OpCode::JMPGT_L
        | OpCode::JMPGE_L
        | OpCode::JMPLT_L
        | OpCode::JMPLE_L
        | OpCode::CALL_L
        | OpCode::ENDTRY_L => i64::from(read_i32(instruction, 0)?),
        opcode => {
            return Err(format!("not a jump instruction: {opcode:?}"));
        }
    };

    jump_target(instruction, offset)
}

/// Calculates the absolute catch/finally targets for a TRY instruction.
pub fn instruction_try_targets(instruction: &Instruction) -> ValidationResult<(usize, usize)> {
    let (catch_offset, finally_offset) = match instruction.opcode() {
        OpCode::TRY => (
            i64::from(read_i8(instruction, 0)?),
            i64::from(read_i8(instruction, 1)?),
        ),
        OpCode::TRY_L => (
            i64::from(read_i32(instruction, 0)?),
            i64::from(read_i32(instruction, 4)?),
        ),
        opcode => {
            return Err(format!("not a TRY instruction: {opcode:?}"));
        }
    };

    Ok((
        jump_target(instruction, catch_offset)?,
        jump_target(instruction, finally_offset)?,
    ))
}

fn validate_instruction(
    instruction: &ScriptInstruction,
    instruction_offsets: &BTreeSet<usize>,
) -> ValidationResult<()> {
    match instruction.opcode() {
        OpCode::JMP
        | OpCode::JMPIF
        | OpCode::JMPIFNOT
        | OpCode::JMPEQ
        | OpCode::JMPNE
        | OpCode::JMPGT
        | OpCode::JMPGE
        | OpCode::JMPLT
        | OpCode::JMPLE
        | OpCode::CALL
        | OpCode::ENDTRY => {
            validate_jump_target(instruction, instruction_offsets)?;
        }
        OpCode::PUSHA
        | OpCode::JMP_L
        | OpCode::JMPIF_L
        | OpCode::JMPIFNOT_L
        | OpCode::JMPEQ_L
        | OpCode::JMPNE_L
        | OpCode::JMPGT_L
        | OpCode::JMPGE_L
        | OpCode::JMPLT_L
        | OpCode::JMPLE_L
        | OpCode::CALL_L
        | OpCode::ENDTRY_L => {
            validate_jump_target(instruction, instruction_offsets)?;
        }
        OpCode::TRY => {
            validate_try_targets(instruction, instruction_offsets)?;
        }
        OpCode::TRY_L => {
            validate_try_targets(instruction, instruction_offsets)?;
        }
        OpCode::NEWARRAY_T | OpCode::ISTYPE | OpCode::CONVERT => {
            let type_byte = read_u8(instruction, 0)?;
            if !is_valid_stack_item_type(type_byte) {
                return Err(format!(
                    "invalid stack item type {type_byte:#04x} at position {} for {:?}",
                    instruction.pointer(),
                    instruction.opcode()
                ));
            }
            if instruction.opcode() != OpCode::NEWARRAY_T
                && type_byte == StackItemType::Any.to_byte()
            {
                return Err(format!(
                    "invalid Any stack item type at position {} for {:?}",
                    instruction.pointer(),
                    instruction.opcode()
                ));
            }
        }
        _ => {}
    }

    Ok(())
}

fn validate_jump_target(
    instruction: &ScriptInstruction,
    instruction_offsets: &BTreeSet<usize>,
) -> ValidationResult<()> {
    let target = instruction_jump_target(instruction)?;
    if instruction_offsets.contains(&target) {
        Ok(())
    } else {
        Err(format!(
            "invalid jump target at position {}: {:?}",
            instruction.pointer(),
            instruction.opcode()
        ))
    }
}

fn validate_try_targets(
    instruction: &ScriptInstruction,
    instruction_offsets: &BTreeSet<usize>,
) -> ValidationResult<()> {
    let (catch_target, finally_target) = instruction_try_targets(instruction)?;
    if !instruction_offsets.contains(&catch_target) {
        return Err(format!(
            "invalid catch target at position {}: {:?}",
            instruction.pointer(),
            instruction.opcode()
        ));
    }
    if !instruction_offsets.contains(&finally_target) {
        return Err(format!(
            "invalid finally target at position {}: {:?}",
            instruction.pointer(),
            instruction.opcode()
        ));
    }
    Ok(())
}

fn jump_target(instruction: &ScriptInstruction, offset: i64) -> ValidationResult<usize> {
    // NeoVM relative offsets are based on the current instruction pointer, not
    // the pointer of the following instruction (Script.cs: ip + TokenI8/I32).
    let target = (instruction.pointer() as i64).checked_add(offset);
    match target {
        Some(target) if target >= 0 => Ok(target as usize),
        _ => Err(format!(
            "invalid jump target at position {}: {:?}",
            instruction.pointer(),
            instruction.opcode()
        )),
    }
}

fn read_u8(instruction: &ScriptInstruction, offset: usize) -> ValidationResult<u8> {
    instruction
        .operand()
        .get(offset)
        .copied()
        .ok_or_else(|| format!("missing operand byte for {:?}", instruction.opcode()))
}

fn read_i8(instruction: &ScriptInstruction, offset: usize) -> ValidationResult<i8> {
    Ok(read_u8(instruction, offset)? as i8)
}

fn read_i32(instruction: &ScriptInstruction, offset: usize) -> ValidationResult<i32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| format!("operand offset overflow for {:?}", instruction.opcode()))?;
    let bytes = instruction
        .operand()
        .get(offset..end)
        .ok_or_else(|| format!("missing i32 operand for {:?}", instruction.opcode()))?;
    Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn is_valid_stack_item_type(type_byte: u8) -> bool {
    StackItemType::from_byte(type_byte).is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        OpCode, instruction_jump_target, parse_script_instructions, validate_script,
        validate_strict_script,
    };

    #[test]
    fn accepts_empty_and_simple_scripts() {
        assert!(validate_strict_script(&[]).is_ok());
        assert!(validate_strict_script(&[OpCode::PUSH1.byte(), OpCode::RET.byte()]).is_ok());
    }

    #[test]
    fn rejects_unknown_opcodes() {
        assert!(validate_strict_script(&[0xff]).is_err());
    }

    #[test]
    fn rejects_truncated_pushdata() {
        assert!(validate_strict_script(&[OpCode::PUSHDATA1.byte(), 2, 1]).is_err());
    }

    #[test]
    fn rejects_invalid_jump_targets() {
        assert!(validate_strict_script(&[OpCode::JMP.byte(), 10, OpCode::RET.byte()]).is_err());
    }

    #[test]
    fn strict_validation_resolves_jumps_from_the_current_instruction() {
        // MainNet block 4,125,644 deploys a contract whose CALL_L at offset
        // 247 has a +2215 displacement and lands at 2462, not 2467.
        let script = [
            OpCode::PUSH1.byte(),
            OpCode::CALL_L.byte(),
            5,
            0,
            0,
            0,
            OpCode::RET.byte(),
            OpCode::NOP.byte(),
        ];
        let validated = validate_script(&script, true).unwrap();
        assert!(validated.has_instruction_at(6));

        let instructions = parse_script_instructions(&script).unwrap();
        assert_eq!(instruction_jump_target(&instructions[1]).unwrap(), 6);
    }

    #[test]
    fn relaxed_validation_parses_offsets_without_strict_jump_checks() {
        let script = validate_script(&[OpCode::JMP.byte(), 10, OpCode::RET.byte()], false).unwrap();
        assert!(script.has_instruction_at(0));
        assert!(script.has_instruction_at(2));
        assert!(!script.has_instruction_at(1));
    }

    #[test]
    fn rejects_any_type_for_convert() {
        assert!(
            validate_strict_script(&[
                OpCode::CONVERT.byte(),
                crate::NEOVM_STACK_ITEM_TYPE_ANY,
                OpCode::RET.byte()
            ])
            .is_err()
        );
    }

    #[test]
    fn exposes_instruction_metadata_for_disassembly_tools() {
        let script = [
            OpCode::PUSHDATA1.byte(),
            3,
            b'n',
            b'e',
            b'o',
            OpCode::SYSCALL.byte(),
            1,
            2,
            3,
            4,
            OpCode::RET.byte(),
        ];

        let instructions = parse_script_instructions(&script).unwrap();
        assert_eq!(instructions.len(), 3);
        assert_eq!(instructions[0].pointer(), 0);
        assert_eq!(instructions[0].opcode(), OpCode::PUSHDATA1);
        assert_eq!(instructions[0].operand(), b"neo");
        assert_eq!(instructions[0].size(), 5);
        assert_eq!(instructions[1].token_u32(), 0x0403_0201);
    }
}
