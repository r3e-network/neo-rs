//! # neo-vm::script_builder
//!
//! Helpers for constructing NeoVM scripts deterministically.
//!
//! ## Boundary
//!
//! This module belongs to `neo-vm`. This VM crate owns deterministic script
//! execution and must not own ledger persistence, network transport, or node
//! composition.
//!
//! ## Contents
//!
//! - `error`: typed script-building errors.
//! - `invocation`: witness invocation-script helpers.
//! - `push`: value-to-push-instruction serialization helpers.
//! - `redeem_script`: redeem-script construction helpers.

mod error;
mod invocation;
mod push;
pub mod redeem_script;

use neo_vm_rs::OpCode;

pub use error::{ScriptBuilderError, ScriptBuilderResult};
pub use invocation::signature_from_invocation;
pub use redeem_script::{RedeemScript, RedeemScriptError};

/// Helps construct VM scripts programmatically.
pub struct ScriptBuilder {
    script: Vec<u8>,
}

impl ScriptBuilder {
    /// Creates a new script builder.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self { script: Vec::new() }
    }

    /// Emits a single byte to the script.
    #[inline]
    pub fn emit(&mut self, op: u8) -> &mut Self {
        self.script.push(op);
        self
    }

    /// Emits an opcode to the script.
    #[inline]
    pub fn emit_opcode(&mut self, op: OpCode) -> &mut Self {
        self.script.push(op.byte());
        self
    }

    /// Emits raw bytes to the script.
    #[inline]
    pub fn emit_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.script.extend_from_slice(bytes);
        self
    }

    /// Emits raw bytes without interpretation.
    #[inline]
    pub fn emit_raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.emit_bytes(bytes)
    }

    /// Emits an opcode followed by operand bytes.
    #[inline]
    pub fn emit_instruction(&mut self, opcode: OpCode, operand: &[u8]) -> &mut Self {
        self.emit_opcode(opcode);
        self.emit_bytes(operand);
        self
    }

    /// Emits a jump operation, automatically upgrading to long form when needed.
    pub fn emit_jump(&mut self, mut opcode: OpCode, offset: i32) -> ScriptBuilderResult<&mut Self> {
        let opcode_value = opcode.byte();
        if opcode_value < OpCode::JMP.byte() || opcode_value > OpCode::JMPLE_L.byte() {
            return Err(ScriptBuilderError::invalid_operation(format!(
                "Invalid jump operation: {opcode:?}"
            )));
        }

        let is_short = opcode_value % 2 == 0;
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

        Ok(self.emit_syscall_hash(neo_vm_rs::interop_hash(api)))
    }

    /// Emits a syscall using a precomputed hash.
    pub fn emit_syscall_hash(&mut self, hash: u32) -> &mut Self {
        self.emit_instruction(OpCode::SYSCALL, &hash.to_le_bytes())
    }

    /// Emits an append operation.
    #[inline]
    pub fn emit_append(&mut self) -> &mut Self {
        self.emit_opcode(OpCode::APPEND)
    }

    /// Emits a pack operation.
    #[inline]
    pub fn emit_pack(&mut self) -> &mut Self {
        self.emit_opcode(OpCode::PACK)
    }

    /// Converts the builder to a byte array.
    #[inline]
    #[must_use]
    pub fn to_array(&self) -> Vec<u8> {
        self.script.clone()
    }

    /// Returns the current script length.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.script.len()
    }

    /// Returns true when no opcodes have been emitted.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.script.is_empty()
    }
}

impl Default for ScriptBuilder {
    fn default() -> Self {
        Self::new()
    }
}
