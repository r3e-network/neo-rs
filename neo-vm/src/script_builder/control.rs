//! Control-flow and syscall emission helpers for [`ScriptBuilder`].

use crate::OpCode;

use super::{ScriptBuilder, ScriptBuilderError, ScriptBuilderResult};

impl ScriptBuilder {
    /// Emits a jump operation, automatically upgrading to long form when needed.
    pub fn emit_jump(&mut self, mut opcode: OpCode, offset: i32) -> ScriptBuilderResult<&mut Self> {
        let opcode_value = opcode.byte();
        if opcode_value < OpCode::JMP.byte() || opcode_value > OpCode::JMPLE_L.byte() {
            return Err(ScriptBuilderError::invalid_operation(format!(
                "Invalid jump operation: {opcode:?}"
            )));
        }

        let is_short = opcode_value.is_multiple_of(2);
        if is_short && (offset < i32::from(i8::MIN) || offset > i32::from(i8::MAX)) {
            opcode = OpCode::try_from(opcode_value + 1)
                .map_err(|_| ScriptBuilderError::invalid_operation("Invalid long jump opcode"))?;
            self.emit_instruction(opcode, &offset.to_le_bytes());
        } else if is_short {
            self.emit_instruction(opcode, &[(offset as i8) as u8]);
        } else {
            self.emit_instruction(opcode, &offset.to_le_bytes());
        }

        Ok(self)
    }

    /// Emits a call operation.
    pub fn emit_call(&mut self, offset: i32) -> ScriptBuilderResult<&mut Self> {
        if offset < i32::from(i8::MIN) || offset > i32::from(i8::MAX) {
            self.emit_instruction(OpCode::CALL_L, &offset.to_le_bytes());
        } else {
            self.emit_instruction(OpCode::CALL, &[(offset as i8) as u8]);
        }
        Ok(self)
    }

    /// Emits a syscall operation.
    pub fn emit_syscall(&mut self, api: &str) -> ScriptBuilderResult<&mut Self> {
        if api.len() > 252 {
            return Err(ScriptBuilderError::invalid_operation(format!(
                "Syscall API too long: {} bytes (max 252)",
                api.len()
            )));
        }

        Ok(self.emit_syscall_hash(crate::interop_hash(api)))
    }

    /// Emits a syscall using a precomputed hash.
    pub fn emit_syscall_hash(&mut self, hash: u32) -> &mut Self {
        self.emit_instruction(OpCode::SYSCALL, &hash.to_le_bytes())
    }
}
