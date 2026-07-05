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
//! - `redeem_script`: redeem-script construction helpers.

pub mod redeem_script;

use neo_vm_rs::OpCode;
use neo_vm_rs::StackValue;
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

pub use redeem_script::{RedeemScript, RedeemScriptError};

/// Errors raised while constructing a VM script.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptBuilderError {
    /// The requested script-building operation is invalid.
    #[error("{0}")]
    InvalidOperation(String),
}

impl ScriptBuilderError {
    /// Creates an [`ScriptBuilderError::InvalidOperation`] from any message.
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::InvalidOperation(message.into())
    }
}

neo_error::impl_error_from_struct!(neo_error::CoreError, ScriptBuilderError => InvalidOperation);

/// Convenience result alias for fallible script-building operations.
pub type ScriptBuilderResult<T> = Result<T, ScriptBuilderError>;

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

    /// Emits a push operation with the given byte payload.
    pub fn emit_push(&mut self, data: &[u8]) -> &mut Self {
        let len = data.len();

        if len <= 0xFF {
            self.emit_opcode(OpCode::PUSHDATA1);
            self.emit(len as u8);
        } else if len <= 0xFFFF {
            self.emit_opcode(OpCode::PUSHDATA2);
            self.emit((len & 0xFF) as u8);
            self.emit((len >> 8) as u8);
        } else {
            self.emit_opcode(OpCode::PUSHDATA4);
            self.emit((len & 0xFF) as u8);
            self.emit(((len >> 8) & 0xFF) as u8);
            self.emit(((len >> 16) & 0xFF) as u8);
            self.emit(((len >> 24) & 0xFF) as u8);
        }
        self.script.extend_from_slice(data);

        self
    }

    /// Emits a push operation for a signed 64-bit integer.
    pub fn emit_push_int(&mut self, value: i64) -> &mut Self {
        if value == -1 {
            return self.emit_opcode(OpCode::PUSHM1);
        }
        if (0..=16).contains(&value) {
            return self.emit(OpCode::PUSH0.byte() + value as u8);
        }

        let negative = value < 0;
        let bytes = neo_vm_rs::encode_integer(value);
        let (opcode, target_len) = match bytes.len() {
            1 => (OpCode::PUSHINT8, 1usize),
            2 => (OpCode::PUSHINT16, 2usize),
            3 | 4 => (OpCode::PUSHINT32, 4usize),
            5..=8 => (OpCode::PUSHINT64, 8usize),
            _ => (OpCode::PUSHINT64, 8usize),
        };

        let operand = pad_signed(&bytes, target_len, negative);
        self.emit_instruction(opcode, &operand);
        self
    }

    /// Emits a push operation for a boolean.
    #[inline]
    pub fn emit_push_bool(&mut self, value: bool) -> &mut Self {
        if value {
            self.emit_opcode(OpCode::PUSHT)
        } else {
            self.emit_opcode(OpCode::PUSHF)
        }
    }

    /// Emits a push operation for a byte array.
    #[inline]
    pub fn emit_push_byte_array(&mut self, data: &[u8]) -> &mut Self {
        self.emit_push(data)
    }

    /// Emits a push operation for a string.
    #[inline]
    pub fn emit_push_string(&mut self, value: &str) -> &mut Self {
        self.emit_push(value.as_bytes())
    }

    /// Emits a push operation for raw bytes.
    #[inline]
    pub fn emit_push_bytes(&mut self, data: &[u8]) -> &mut Self {
        self.emit_push(data)
    }

    /// Emits a push operation for an arbitrary precision integer.
    pub fn emit_push_bigint(&mut self, value: BigInt) -> ScriptBuilderResult<&mut Self> {
        if value >= BigInt::from(-1) && value <= BigInt::from(16) {
            if let Some(v) = value.to_i64() {
                if v == -1 {
                    self.emit_opcode(OpCode::PUSHM1);
                } else {
                    self.emit(OpCode::PUSH0.byte().wrapping_add(v as u8));
                }
                return Ok(self);
            }
        }

        let bytes = value
            .to_i64()
            .map_or_else(|| value.to_signed_bytes_le(), neo_vm_rs::encode_integer);
        let negative = matches!(value.sign(), Sign::Minus);
        let target_len = if bytes.len() <= 1 {
            1
        } else if bytes.len() <= 2 {
            2
        } else if bytes.len() <= 4 {
            4
        } else if bytes.len() <= 8 {
            8
        } else if bytes.len() <= 16 {
            16
        } else if bytes.len() <= 32 {
            32
        } else {
            return Err(ScriptBuilderError::invalid_operation(
                "BigInteger value exceeds PUSHINT256 capacity",
            ));
        };

        let opcode = match target_len {
            1 => OpCode::PUSHINT8,
            2 => OpCode::PUSHINT16,
            4 => OpCode::PUSHINT32,
            8 => OpCode::PUSHINT64,
            16 => OpCode::PUSHINT128,
            32 => OpCode::PUSHINT256,
            _ => {
                return Err(ScriptBuilderError::invalid_operation(format!(
                    "Invalid integer size for push: {target_len}"
                )));
            }
        };

        let operand = pad_signed(&bytes, target_len, negative);
        self.emit_instruction(opcode, &operand);
        Ok(self)
    }

    /// Emits a push operation for a `neo-vm-rs` stack value.
    pub fn emit_push_stack_value(&mut self, item: &StackValue) -> ScriptBuilderResult<&mut Self> {
        match item {
            StackValue::Null => {
                self.emit_opcode(OpCode::PUSHNULL);
            }
            StackValue::Boolean(value) => {
                self.emit_push_bool(*value);
            }
            StackValue::Integer(value) => {
                self.emit_push_int(*value);
            }
            StackValue::BigInteger(bytes) => {
                self.emit_push_bigint(BigInt::from_signed_bytes_le(bytes))?;
            }
            StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => {
                self.emit_push(bytes);
            }
            StackValue::Array(items) | StackValue::Struct(items) => {
                for item in items.iter().rev() {
                    self.emit_push_stack_value(item)?;
                }
                self.emit_push_int(items.len() as i64);
                self.emit_pack();
            }
            StackValue::Map(entries) => {
                self.emit_opcode(OpCode::NEWMAP);
                for (key, value) in entries {
                    self.emit_opcode(OpCode::DUP);
                    self.emit_push_stack_value(key)?;
                    self.emit_push_stack_value(value)?;
                    self.emit_opcode(OpCode::SETITEM);
                }
            }
            StackValue::Pointer(_) => {
                return Err(ScriptBuilderError::invalid_operation(
                    "Cannot serialize Pointer to script",
                ));
            }
            StackValue::Interop(_) | StackValue::Iterator(_) => {
                return Err(ScriptBuilderError::invalid_operation(
                    "Cannot serialize InteropInterface to script",
                ));
            }
        }

        Ok(self)
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

    /// Push a signature onto the stack as a single-sig invocation script.
    ///
    /// For a 64-byte secp256r1 signature this produces the canonical
    /// `PUSHDATA1 0x40 <64-byte sig>` sequence (66 bytes total) that
    /// Neo witness invocation scripts use. The output is byte-identical
    /// to the hand-rolled `PUSHDATA1 + len + sig` construction that was
    /// previously duplicated across neo-consensus and neo-node.
    ///
    /// This is the inverse of [`signature_from_invocation`].
    pub fn invocation_from_signature(&mut self, signature: &[u8]) -> &mut Self {
        self.emit_push(signature)
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

/// Extract the raw signature from a `PUSHDATA1 0x40 <64-byte sig>` invocation
/// script.
///
/// Returns `None` if the script doesn't match this exact shape (wrong length,
/// wrong opcode, or wrong length byte). This is the inverse of
/// [`ScriptBuilder::invocation_from_signature`].
///
/// The returned slice borrows from `script` — no allocation.
pub fn signature_from_invocation(script: &[u8]) -> Option<&[u8]> {
    if script.len() != 66 {
        return None;
    }
    if script[0] != OpCode::PUSHDATA1.byte() || script[1] != 0x40 {
        return None;
    }
    Some(&script[2..66])
}

fn pad_signed(bytes: &[u8], target_len: usize, negative: bool) -> Vec<u8> {
    let mut padded = Vec::with_capacity(target_len);
    padded.extend_from_slice(bytes);
    let fill = if negative { 0xFF } else { 0x00 };
    while padded.len() < target_len {
        padded.push(fill);
    }
    padded
}
