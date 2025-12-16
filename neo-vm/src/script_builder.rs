//! Script builder module for the Neo Virtual Machine.
//!
//! This module provides a way to programmatically construct scripts for the Neo VM.

use crate::error::{VmError, VmResult};
use crate::op_code::OpCode;
use crate::script::Script;
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;
use sha2::{Digest, Sha256};

/// Helps construct VM scripts programmatically.
pub struct ScriptBuilder {
    /// The script being built
    script: Vec<u8>,
}

impl ScriptBuilder {
    /// Creates a new script builder.
    #[inline]
    pub fn new() -> Self {
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
        self.script.push(op as u8);
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
            let opcode_value = OpCode::PUSH0 as u8 + (value as u8);
            self.emit(opcode_value);
            return self;
        }

        let mut bytes = Vec::new();
        let mut v = value;

        // Convert to little-endian byte representation
        while v != 0 && v != -1 {
            bytes.push((v & 0xFF) as u8);
            v >>= 8;
        }

        // Handle sign bit
        if v == -1 && (bytes.last().unwrap_or(&0) & 0x80) == 0 {
            bytes.push(0xFF);
        } else if v == 0
            && !bytes.is_empty()
            && (bytes.last().expect("collection should not be empty") & 0x80) != 0
        {
            bytes.push(0x00);
        }

        self.emit_push(&bytes)
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
    pub fn emit_push_bigint(&mut self, value: BigInt) -> VmResult<&mut Self> {
        if value >= BigInt::from(-1) && value <= BigInt::from(16) {
            if let Some(v) = value.to_i64() {
                return Ok(self.emit_push_int(v));
            }
        }

        let mut bytes = value.to_signed_bytes_le();
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
            return Err(VmError::invalid_operation_msg(
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
            _ => unreachable!(),
        };

        self.emit_instruction(opcode, &padded);
        Ok(self)
    }

    /// Emits a push operation for a stack item.
    pub fn emit_push_stack_item(
        &mut self,
        item: crate::stack_item::StackItem,
    ) -> VmResult<&mut Self> {
        use crate::stack_item::StackItem;

        match item {
            StackItem::Null => {
                self.emit_opcode(OpCode::PUSHNULL);
            }
            StackItem::Boolean(b) => {
                self.emit_push_bool(b);
            }
            StackItem::Integer(i) => {
                self.emit_push_bigint(i)?;
            }
            StackItem::ByteString(bytes) => {
                self.emit_push(&bytes);
            }
            StackItem::Buffer(buffer) => {
                self.emit_push(buffer.data());
            }
            StackItem::Array(array) => {
                let items: Vec<_> = array.into_iter().collect();
                let items_len = items.len();
                for item in items.into_iter().rev() {
                    self.emit_push_stack_item(item)?;
                }
                self.emit_push_int(items_len as i64);
                self.emit_pack();
            }
            StackItem::Struct(structure) => {
                let items: Vec<_> = structure.into_iter().collect();
                let items_len = items.len();
                for item in items.into_iter().rev() {
                    self.emit_push_stack_item(item)?;
                }
                self.emit_push_int(items_len as i64);
                self.emit_pack();
            }
            StackItem::Map(map) => {
                // 1. Emit map size (production map format)
                self.emit_push_int(map.len() as i64);

                // 2. Create new map instruction (production map creation)
                self.emit_opcode(OpCode::NEWMAP);

                // 3. Emit key-value pairs (production map population)
                for (key, value) in map {
                    self.emit_opcode(OpCode::DUP);

                    // Push key and value onto stack
                    self.emit_push_stack_item(key)?;
                    self.emit_push_stack_item(value)?;

                    // Set the key-value pair in the map
                    self.emit_opcode(OpCode::SETITEM);
                }
            }
            StackItem::Pointer(_) => {
                return Err(VmError::invalid_operation_msg(
                    "Cannot serialize Pointer to script".to_string(),
                ));
            }
            StackItem::InteropInterface(_) => {
                // InteropInterface cannot be serialized to script
                return Err(VmError::invalid_operation_msg(
                    "Cannot serialize InteropInterface to script".to_string(),
                ));
            }
        }

        Ok(self)
    }

    /// Emits a jump operation, automatically upgrading to the long form when needed.
    pub fn emit_jump(&mut self, mut opcode: OpCode, offset: i32) -> VmResult<&mut Self> {
        let opcode_value = opcode as u8;
        if opcode_value < OpCode::JMP as u8 || opcode_value > OpCode::JMPLE_L as u8 {
            return Err(VmError::invalid_operation_msg(format!(
                "Invalid jump operation: {:?}",
                opcode
            )));
        }

        let is_short = opcode_value % 2 == 0;
        if is_short {
            if offset < i8::MIN as i32 || offset > i8::MAX as i32 {
                opcode = OpCode::try_from(opcode_value + 1)
                    .map_err(|_| VmError::invalid_operation_msg("Invalid long jump opcode"))?;
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
    pub fn emit_call(&mut self, offset: i32) -> VmResult<&mut Self> {
        if offset < i8::MIN as i32 || offset > i8::MAX as i32 {
            self.emit_instruction(OpCode::CALL_L, &offset.to_le_bytes());
        } else {
            let short = (offset as i8) as u8;
            self.emit_instruction(OpCode::CALL, &[short]);
        }
        Ok(self)
    }

    /// Emits a syscall operation.
    pub fn emit_syscall(&mut self, api: &str) -> VmResult<&mut Self> {
        let hash = Self::hash_syscall(api)?;
        Ok(self.emit_syscall_hash(hash))
    }

    /// Emits a syscall using a precomputed hash.
    pub fn emit_syscall_hash(&mut self, hash: u32) -> &mut Self {
        self.emit_instruction(OpCode::SYSCALL, &hash.to_le_bytes())
    }

    /// Computes the C#-compatible syscall hash (single SHA-256 over raw ASCII, little-endian u32).
    pub fn hash_syscall(api: &str) -> VmResult<u32> {
        if api.len() > 252 {
            return Err(VmError::invalid_operation_msg(format!(
                "Syscall API too long: {} bytes (max 252)",
                api.len()
            )));
        }

        let mut hasher = Sha256::new();
        hasher.update(api.as_bytes());
        let digest = hasher.finalize();
        Ok(u32::from_le_bytes([
            digest[0], digest[1], digest[2], digest[3],
        ]))
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

    /// Converts the builder to a script.
    #[inline]
    pub fn to_script(&self) -> Script {
        Script::new_relaxed(self.script.clone())
    }

    /// Converts the builder to a byte array.
    #[inline]
    pub fn to_array(&self) -> Vec<u8> {
        self.script.clone()
    }

    /// Returns the current script length.
    #[inline]
    pub fn len(&self) -> usize {
        self.script.len()
    }

    /// Returns true when no opcodes have been emitted.
    #[inline]
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
            vec![OpCode::PUSH1 as u8, OpCode::PUSH2 as u8, OpCode::ADD as u8]
        );
    }

    #[test]
    fn test_emit_push_int() {
        let mut builder = ScriptBuilder::new();

        // Special cases
        builder.emit_push_int(-1);
        builder.emit_push_int(0);
        builder.emit_push_int(10);

        // Larger integers
        builder.emit_push_int(100);
        builder.emit_push_int(-100);

        let script = builder.to_array();

        // Check special cases
        assert_eq!(script[0], OpCode::PUSHM1 as u8);
        assert_eq!(script[1], OpCode::PUSH0 as u8);
        assert_eq!(script[2], OpCode::PUSH10 as u8);

        assert!(script.len() > 5);
    }

    #[test]
    fn test_emit_push_bool() {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_bool(true);
        assert_eq!(builder.to_array(), vec![OpCode::PUSHT as u8]);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_bool(false);
        assert_eq!(builder.to_array(), vec![OpCode::PUSHF as u8]);
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

        assert_eq!(script[0], OpCode::PUSHDATA1 as u8);
        assert_eq!(script[1], small_array.len() as u8);
        assert_eq!(&script[2..5], &[1, 2, 3]);

        assert_eq!(script[5], OpCode::PUSHDATA1 as u8);
        assert_eq!(script[6], 200); // Length as single byte

        let large_array_offset = 5 + 2 + 200;
        assert_eq!(script[large_array_offset], OpCode::PUSHDATA2 as u8);
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
        assert_eq!(script, vec![OpCode::JMP as u8, 10]);
    }

    #[test]
    fn test_emit_syscall() {
        let mut builder = ScriptBuilder::new();
        let api_name = "System.Runtime.Log";
        builder.emit_syscall(api_name).expect("emit_syscall failed");

        let script = builder.to_array();
        let expected_hash = ScriptBuilder::hash_syscall(api_name).unwrap();
        assert_eq!(script.len(), 5);
        assert_eq!(script[0], OpCode::SYSCALL as u8);
        assert_eq!(&script[1..5], &expected_hash.to_le_bytes());
    }

    #[test]
    fn test_to_script() {
        let mut builder = ScriptBuilder::new();
        builder.emit_opcode(OpCode::PUSH1);
        builder.emit_opcode(OpCode::RET);

        let script = builder.to_script();

        assert_eq!(script.len(), 2);
    }
}
