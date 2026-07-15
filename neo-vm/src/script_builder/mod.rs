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
//! - `control`: control-flow and syscall emission helpers.
//! - `error`: typed script-building errors.
//! - `invocation`: witness invocation-script helpers.
//! - `push`: value-to-push-instruction serialization helpers.
//! - `redeem_script`: redeem-script construction helpers.

mod control;
mod error;
mod invocation;
mod push;
pub mod redeem_script;

use crate::OpCode;

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
