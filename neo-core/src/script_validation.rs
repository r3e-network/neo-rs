use std::collections::HashSet;

use neo_vm_rs::OpCode;

type ValidationResult<T> = Result<T, String>;

#[derive(Debug, Clone)]
pub struct ValidatedScript {
    instruction_offsets: HashSet<usize>,
}

impl ValidatedScript {
    #[must_use]
    pub fn has_instruction_at(&self, offset: usize) -> bool {
        self.instruction_offsets.contains(&offset)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScriptInstruction<'a> {
    pointer: usize,
    opcode: OpCode,
    operand: &'a [u8],
    size: usize,
}

impl<'a> ScriptInstruction<'a> {
    #[must_use]
    pub const fn pointer(&self) -> usize {
        self.pointer
    }

    #[must_use]
    pub const fn opcode(&self) -> OpCode {
        self.opcode
    }

    #[must_use]
    pub const fn operand(&self) -> &'a [u8] {
        self.operand
    }

    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    #[must_use]
    pub fn token_u16(&self) -> u16 {
        let mut bytes = [0u8; 2];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            *slot = *self.operand.get(idx).unwrap_or(&0);
        }
        u16::from_le_bytes(bytes)
    }

    #[must_use]
    pub fn token_u32(&self) -> u32 {
        let mut bytes = [0u8; 4];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            *slot = *self.operand.get(idx).unwrap_or(&0);
        }
        u32::from_le_bytes(bytes)
    }
}

/// Validates NeoVM bytecode using the shared neo-vm-rs opcode metadata.
pub fn validate_strict_script(script: &[u8]) -> ValidationResult<()> {
    validate_script(script, true).map(|_| ())
}

/// Validates NeoVM bytecode and returns instruction offsets for ABI checks.
pub fn validate_script(script: &[u8], strict: bool) -> ValidationResult<ValidatedScript> {
    let instructions = parse_script_instructions(script)?;
    let instruction_offsets = instructions
        .iter()
        .map(|instruction| instruction.pointer)
        .collect::<HashSet<_>>();

    if strict {
        for instruction in &instructions {
            validate_instruction(instruction, &instruction_offsets)?;
        }
    }

    Ok(ValidatedScript {
        instruction_offsets,
    })
}

pub fn parse_script_instructions(script: &[u8]) -> ValidationResult<Vec<ScriptInstruction<'_>>> {
    let mut position = 0;
    let mut instructions = Vec::new();

    while position < script.len() {
        let instruction = parse_instruction(script, position)?;
        position = position
            .checked_add(instruction.size)
            .ok_or_else(|| "instruction position overflow".to_string())?;
        instructions.push(instruction);
    }

    Ok(instructions)
}

fn parse_instruction(script: &[u8], position: usize) -> ValidationResult<ScriptInstruction<'_>> {
    let opcode_byte = *script
        .get(position)
        .ok_or_else(|| "position out of bounds".to_string())?;
    let opcode =
        OpCode::try_from(opcode_byte).map_err(|_| format!("invalid opcode: {opcode_byte:#04x}"))?;

    let operand_prefix = opcode.operand_prefix();
    let (operand_start, operand_len) = if operand_prefix == 0 {
        let operand_size = opcode.operand_size();
        let operand_start = position
            .checked_add(1)
            .ok_or_else(|| format!("operand start overflow for {opcode:?}"))?;
        (operand_start, operand_size)
    } else {
        let prefix_start = position
            .checked_add(1)
            .ok_or_else(|| format!("operand prefix start overflow for {opcode:?}"))?;
        let prefix_end = prefix_start
            .checked_add(operand_prefix)
            .ok_or_else(|| format!("operand prefix end overflow for {opcode:?}"))?;
        if prefix_end > script.len() {
            return Err(format!("{opcode:?} missing length prefix"));
        }

        let operand_len = match operand_prefix {
            1 => usize::from(script[prefix_start]),
            2 => {
                let bytes = [script[prefix_start], script[prefix_start + 1]];
                usize::from(u16::from_le_bytes(bytes))
            }
            4 => {
                let bytes = [
                    script[prefix_start],
                    script[prefix_start + 1],
                    script[prefix_start + 2],
                    script[prefix_start + 3],
                ];
                u32::from_le_bytes(bytes) as usize
            }
            _ => {
                return Err(format!(
                    "unsupported operand prefix width {operand_prefix} for {opcode:?}"
                ));
            }
        };

        (prefix_end, operand_len)
    };

    let operand_end = operand_start
        .checked_add(operand_len)
        .ok_or_else(|| format!("{opcode:?} operand size overflowed script bounds"))?;
    if operand_end > script.len() {
        return Err(format!(
            "{opcode:?} operand size exceeds script bounds: {operand_start} + {operand_len} > {}",
            script.len()
        ));
    }

    let size = 1usize
        .checked_add(operand_prefix)
        .and_then(|size| size.checked_add(operand_len))
        .ok_or_else(|| format!("{opcode:?} instruction size overflowed"))?;

    Ok(ScriptInstruction {
        pointer: position,
        opcode,
        operand: &script[operand_start..operand_end],
        size,
    })
}

fn validate_instruction(
    instruction: &ScriptInstruction<'_>,
    instruction_offsets: &HashSet<usize>,
) -> ValidationResult<()> {
    match instruction.opcode {
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
            let offset = i64::from(read_i8(instruction, 0)?);
            validate_jump_target(instruction, offset, instruction_offsets)?;
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
            let offset = i64::from(read_i32(instruction, 0)?);
            validate_jump_target(instruction, offset, instruction_offsets)?;
        }
        OpCode::TRY => {
            validate_try_target(
                instruction,
                i64::from(read_i8(instruction, 0)?),
                "catch",
                instruction_offsets,
            )?;
            validate_try_target(
                instruction,
                i64::from(read_i8(instruction, 1)?),
                "finally",
                instruction_offsets,
            )?;
        }
        OpCode::TRY_L => {
            validate_try_target(
                instruction,
                i64::from(read_i32(instruction, 0)?),
                "catch",
                instruction_offsets,
            )?;
            validate_try_target(
                instruction,
                i64::from(read_i32(instruction, 4)?),
                "finally",
                instruction_offsets,
            )?;
        }
        OpCode::NEWARRAY_T | OpCode::ISTYPE | OpCode::CONVERT => {
            let type_byte = read_u8(instruction, 0)?;
            if !is_valid_stack_item_type(type_byte) {
                return Err(format!(
                    "invalid stack item type {type_byte:#04x} at position {} for {:?}",
                    instruction.pointer, instruction.opcode
                ));
            }
            if instruction.opcode != OpCode::NEWARRAY_T
                && type_byte == neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ANY
            {
                return Err(format!(
                    "invalid Any stack item type at position {} for {:?}",
                    instruction.pointer, instruction.opcode
                ));
            }
        }
        _ => {}
    }

    Ok(())
}

fn validate_jump_target(
    instruction: &ScriptInstruction<'_>,
    offset: i64,
    instruction_offsets: &HashSet<usize>,
) -> ValidationResult<()> {
    let target = jump_target(instruction, offset)?;
    if instruction_offsets.contains(&target) {
        Ok(())
    } else {
        Err(format!(
            "invalid jump target at position {}: {:?}",
            instruction.pointer, instruction.opcode
        ))
    }
}

fn validate_try_target(
    instruction: &ScriptInstruction<'_>,
    offset: i64,
    label: &str,
    instruction_offsets: &HashSet<usize>,
) -> ValidationResult<()> {
    let target = jump_target(instruction, offset)?;
    if instruction_offsets.contains(&target) {
        Ok(())
    } else {
        Err(format!(
            "invalid {label} target at position {}: {:?}",
            instruction.pointer, instruction.opcode
        ))
    }
}

fn jump_target(instruction: &ScriptInstruction<'_>, offset: i64) -> ValidationResult<usize> {
    let next_ip = instruction
        .pointer
        .checked_add(instruction.size)
        .ok_or_else(|| format!("next instruction overflow at {}", instruction.pointer))?;
    let target = next_ip as i64 + offset;
    if target < 0 {
        return Err(format!(
            "negative jump target at position {}: {:?}",
            instruction.pointer, instruction.opcode
        ));
    }
    Ok(target as usize)
}

fn read_u8(instruction: &ScriptInstruction<'_>, offset: usize) -> ValidationResult<u8> {
    instruction
        .operand
        .get(offset)
        .copied()
        .ok_or_else(|| format!("missing operand byte for {:?}", instruction.opcode))
}

fn read_i8(instruction: &ScriptInstruction<'_>, offset: usize) -> ValidationResult<i8> {
    Ok(read_u8(instruction, offset)? as i8)
}

fn read_i32(instruction: &ScriptInstruction<'_>, offset: usize) -> ValidationResult<i32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| format!("operand offset overflow for {:?}", instruction.opcode))?;
    let bytes = instruction
        .operand
        .get(offset..end)
        .ok_or_else(|| format!("missing i32 operand for {:?}", instruction.opcode))?;
    Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn is_valid_stack_item_type(type_byte: u8) -> bool {
    matches!(
        type_byte,
        neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ANY
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_POINTER
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BOOLEAN
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEGER
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BYTESTRING
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BUFFER
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ARRAY
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_STRUCT
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_MAP
            | neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_script_instructions, validate_script, validate_strict_script};
    use neo_vm_rs::OpCode;

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
    fn relaxed_validation_parses_offsets_without_strict_jump_checks() {
        let script = validate_script(&[OpCode::JMP.byte(), 10, OpCode::RET.byte()], false).unwrap();
        assert!(script.has_instruction_at(0));
        assert!(script.has_instruction_at(2));
        assert!(!script.has_instruction_at(1));
    }

    #[test]
    fn rejects_any_type_for_convert() {
        assert!(validate_strict_script(&[
            OpCode::CONVERT.byte(),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ANY,
            OpCode::RET.byte()
        ])
        .is_err());
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
