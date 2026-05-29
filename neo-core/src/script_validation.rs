//! Script validation re-exported from neo-vm.

pub use crate::neo_vm::{
    parse_script_instructions, validate_script, validate_strict_script, ScriptInstruction,
    ValidatedScript, ValidationResult,
};

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
