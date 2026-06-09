//! Script builder for the Neo Virtual Machine.
//!
//! Provides [`ScriptBuilder`], a programmatic constructor for Neo VM scripts.
//! This is the Rust counterpart of C# `Neo.VM.ScriptBuilder`; it is layered on
//! the external `neo-vm-rs` pure-VM crate (for [`OpCode`], [`StackValue`],
//! integer encoding and interop hashing) and sits *below* `neo-core` so script
//! construction is available to crypto/payload layers without depending on the
//! stateful VM host or the smart-contract engine.
//!
//! The byte-construction here is consensus-critical: emitted scripts determine
//! script hashes (and therefore addresses), so the encoding must stay
//! byte-identical to C# Neo.

use neo_vm_rs::OpCode;
use neo_vm_rs::StackValue;
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

/// Errors raised while constructing a VM script.
///
/// These correspond to programmer/usage errors (oversized integers, invalid
/// jump opcodes, non-serializable stack values, over-long syscall names). This
/// crate provides `From<ScriptBuilderError> for neo_error::CoreError` so callers
/// returning `CoreResult` can keep using `?` unchanged.
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

impl From<ScriptBuilderError> for neo_error::CoreError {
    fn from(err: ScriptBuilderError) -> Self {
        neo_error::CoreError::InvalidOperation {
            message: err.to_string(),
        }
    }
}

/// Convenience result alias for fallible script-building operations.
pub type ScriptBuilderResult<T> = Result<T, ScriptBuilderError>;

/// Helps construct VM scripts programmatically.
pub struct ScriptBuilder {
    /// The script being built
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

    /// Emits raw bytes without interpretation (utility for parity helpers).
    #[inline]
    pub fn emit_raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.emit_bytes(bytes)
    }

    /// Emits an opcode followed by the provided operand bytes.
    #[inline]
    pub fn emit_instruction(&mut self, opcode: OpCode, operand: &[u8]) -> &mut Self {
        self.emit_opcode(opcode);
        self.emit_bytes(operand);
        self
    }

    /// Emits a push operation with the given data.
    pub fn emit_push(&mut self, data: &[u8]) -> &mut Self {
        let len = data.len();

        if len <= 0xFF {
            // Always use PUSHDATA1 for small payloads to mirror C# implementation
            self.emit_opcode(OpCode::PUSHDATA1);
            self.emit(len as u8);
            self.script.extend_from_slice(data);
        } else if len <= 0xFFFF {
            // PUSHDATA2
            self.emit_opcode(OpCode::PUSHDATA2);
            self.emit((len & 0xFF) as u8);
            self.emit((len >> 8) as u8);
            self.script.extend_from_slice(data);
        } else {
            // PUSHDATA4
            self.emit_opcode(OpCode::PUSHDATA4);
            self.emit((len & 0xFF) as u8);
            self.emit(((len >> 8) & 0xFF) as u8);
            self.emit(((len >> 16) & 0xFF) as u8);
            self.emit(((len >> 24) & 0xFF) as u8);
            self.script.extend_from_slice(data);
        }

        self
    }

    /// Emits a push operation for an integer.
    pub fn emit_push_int(&mut self, value: i64) -> &mut Self {
        if value == -1 {
            return self.emit_opcode(OpCode::PUSHM1);
        }
        if (0..=16).contains(&value) {
            let opcode_value = OpCode::PUSH0.byte() + (value as u8);
            self.emit(opcode_value);
            return self;
        }

        // Match C# Neo.VM.ScriptBuilder.EmitPush(BigInteger) for integer encoding:
        // use PUSHINT8/16/32/64 with right-padding for sign extension.
        let negative = value < 0;
        let bytes = neo_vm_rs::encode_integer(value);
        let bytes_written = bytes.len();

        let (opcode, target_len) = match bytes_written {
            1 => (OpCode::PUSHINT8, 1usize),
            2 => (OpCode::PUSHINT16, 2usize),
            3 | 4 => (OpCode::PUSHINT32, 4usize),
            5..=8 => (OpCode::PUSHINT64, 8usize),
            _ => (OpCode::PUSHINT64, 8usize),
        };

        let mut operand = Vec::with_capacity(target_len);
        operand.extend_from_slice(&bytes);
        let fill = if negative { 0xFF } else { 0x00 };
        while operand.len() < target_len {
            operand.push(fill);
        }

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

    /// Emits a push operation for an arbitrary precision integer (C# parity).
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

        let mut bytes = value
            .to_i64()
            .map_or_else(|| value.to_signed_bytes_le(), neo_vm_rs::encode_integer);
        if bytes.is_empty() {
            bytes.push(0);
        }

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

        let padded = pad_signed(&bytes, target_len, negative);
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
                )))
            }
        };

        self.emit_instruction(opcode, &padded);
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
                // Create a new map instruction
                self.emit_opcode(OpCode::NEWMAP);

                // Emit key-value pairs
                for (key, value) in entries {
                    // Keep the map on stack now that SETITEM consumes the collection.
                    self.emit_opcode(OpCode::DUP);
                    // Push key and value onto stack
                    self.emit_push_stack_value(key)?;
                    self.emit_push_stack_value(value)?;

                    // Set the key-value pair in the map
                    self.emit_opcode(OpCode::SETITEM);
                }
            }
            StackValue::Pointer(_) => {
                return Err(ScriptBuilderError::invalid_operation(
                    "Cannot serialize Pointer to script".to_string(),
                ));
            }
            StackValue::Interop(_) | StackValue::Iterator(_) => {
                // Interop handles cannot be serialized to script.
                return Err(ScriptBuilderError::invalid_operation(
                    "Cannot serialize InteropInterface to script".to_string(),
                ));
            }
        }

        Ok(self)
    }

    /// Emits a jump operation, automatically upgrading to the long form when needed.
    pub fn emit_jump(&mut self, mut opcode: OpCode, offset: i32) -> ScriptBuilderResult<&mut Self> {
        let opcode_value = opcode.byte();
        if opcode_value < OpCode::JMP.byte() || opcode_value > OpCode::JMPLE_L.byte() {
            return Err(ScriptBuilderError::invalid_operation(format!(
                "Invalid jump operation: {opcode:?}"
            )));
        }

        let is_short = opcode_value % 2 == 0;
        if is_short {
            if offset < i32::from(i8::MIN) || offset > i32::from(i8::MAX) {
                opcode = OpCode::try_from(opcode_value + 1)
                    .map_err(|_| ScriptBuilderError::invalid_operation("Invalid long jump opcode"))?;
                self.emit_instruction(opcode, &offset.to_le_bytes());
            } else {
                let short = (offset as i8) as u8;
                self.emit_instruction(opcode, &[short]);
            }
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
            let short = (offset as i8) as u8;
            self.emit_instruction(OpCode::CALL, &[short]);
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

fn pad_signed(bytes: &[u8], target_len: usize, negative: bool) -> Vec<u8> {
    let mut padded = Vec::with_capacity(target_len);
    padded.extend_from_slice(bytes);
    let fill = if negative { 0xFF } else { 0x00 };
    while padded.len() < target_len {
        padded.push(fill);
    }
    padded
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_opcode() {
        let mut builder = ScriptBuilder::new();
        builder.emit_opcode(OpCode::PUSH1);
        builder.emit_opcode(OpCode::PUSH2);
        builder.emit_opcode(OpCode::ADD);

        let script = builder.to_array();
        assert_eq!(
            script,
            vec![
                OpCode::PUSH1.byte(),
                OpCode::PUSH2.byte(),
                OpCode::ADD.byte()
            ]
        );
    }

    #[test]
    fn test_emit_push_int() {
        fn assert_push(value: i64, expected: &[u8]) {
            let mut builder = ScriptBuilder::new();
            builder.emit_push_int(value);
            assert_eq!(builder.to_array(), expected);
        }

        // Special cases (-1..=16)
        assert_push(-1, &[OpCode::PUSHM1.byte()]);
        assert_push(0, &[OpCode::PUSH0.byte()]);
        assert_push(10, &[OpCode::PUSH10.byte()]);
        assert_push(16, &[OpCode::PUSH16.byte()]);

        // PUSHINT8
        assert_push(100, &[OpCode::PUSHINT8.byte(), 0x64]);
        assert_push(-100, &[OpCode::PUSHINT8.byte(), 0x9C]);
        assert_push(127, &[OpCode::PUSHINT8.byte(), 0x7F]);
        assert_push(-128, &[OpCode::PUSHINT8.byte(), 0x80]);

        // Boundary cases that require sign-extension padding
        assert_push(128, &[OpCode::PUSHINT16.byte(), 0x80, 0x00]);
        assert_push(255, &[OpCode::PUSHINT16.byte(), 0xFF, 0x00]);
        assert_push(256, &[OpCode::PUSHINT16.byte(), 0x00, 0x01]);
        assert_push(300, &[OpCode::PUSHINT16.byte(), 0x2C, 0x01]);
        assert_push(-300, &[OpCode::PUSHINT16.byte(), 0xD4, 0xFE]);
        assert_push(-129, &[OpCode::PUSHINT16.byte(), 0x7F, 0xFF]);

        // PUSHINT32 (values that require 3 bytes)
        assert_push(32768, &[OpCode::PUSHINT32.byte(), 0x00, 0x80, 0x00, 0x00]);
    }

    #[test]
    fn test_emit_push_bigint_uses_minimal_integer_encoding() {
        fn assert_push(value: i64, expected: &[u8]) {
            let mut builder = ScriptBuilder::new();
            builder
                .emit_push_bigint(BigInt::from(value))
                .expect("emit bigint");
            assert_eq!(builder.to_array(), expected);
        }

        assert_push(-1, &[OpCode::PUSHM1.byte()]);
        assert_push(0, &[OpCode::PUSH0.byte()]);
        assert_push(16, &[OpCode::PUSH16.byte()]);
        assert_push(128, &[OpCode::PUSHINT16.byte(), 0x80, 0x00]);
        assert_push(-129, &[OpCode::PUSHINT16.byte(), 0x7F, 0xFF]);
        assert_push(32768, &[OpCode::PUSHINT32.byte(), 0x00, 0x80, 0x00, 0x00]);
    }

    #[test]
    fn test_emit_push_bool() {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_bool(true);
        assert_eq!(builder.to_array(), vec![OpCode::PUSHT.byte()]);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_bool(false);
        assert_eq!(builder.to_array(), vec![OpCode::PUSHF.byte()]);
    }

    #[test]
    fn test_emit_push_byte_array() {
        let mut builder = ScriptBuilder::new();

        // Small array
        let small_array = [1, 2, 3];
        builder.emit_push_byte_array(&small_array);

        let medium_array = [0; 200];
        builder.emit_push_byte_array(&medium_array);

        let large_array = [0; 65000];
        builder.emit_push_byte_array(&large_array);

        let script = builder.to_array();

        assert_eq!(script[0], OpCode::PUSHDATA1.byte());
        assert_eq!(script[1], small_array.len() as u8);
        assert_eq!(&script[2..5], &[1, 2, 3]);

        assert_eq!(script[5], OpCode::PUSHDATA1.byte());
        assert_eq!(script[6], 200); // Length as single byte

        let large_array_offset = 5 + 2 + 200;
        assert_eq!(script[large_array_offset], OpCode::PUSHDATA2.byte());
        assert_eq!(script[large_array_offset + 1], (65000 & 0xFF) as u8);
        assert_eq!(script[large_array_offset + 2], ((65000 >> 8) & 0xFF) as u8);
    }

    #[test]
    fn test_emit_jump() {
        let mut builder = ScriptBuilder::new();
        builder
            .emit_jump(OpCode::JMP, 10)
            .expect("emit_jump failed");

        let script = builder.to_array();
        assert_eq!(script, vec![OpCode::JMP.byte(), 10]);
    }

    #[test]
    fn test_emit_syscall() {
        let mut builder = ScriptBuilder::new();
        let api_name = "System.Runtime.Log";
        builder.emit_syscall(api_name).expect("emit_syscall failed");

        let script = builder.to_array();
        let expected_hash = neo_vm_rs::interop_hash(api_name);
        assert_eq!(script.len(), 5);
        assert_eq!(script[0], OpCode::SYSCALL.byte());
        assert_eq!(&script[1..5], &expected_hash.to_le_bytes());
    }
}
