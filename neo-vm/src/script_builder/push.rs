//! Push/value serialization helpers for [`ScriptBuilder`].
//!
//! This module owns the mechanics for turning Rust/VM values into NeoVM push
//! instructions. Core byte emission and control-flow instructions stay in the
//! parent module.

use crate::{OpCode, StackItem};
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

use super::{ScriptBuilder, ScriptBuilderError, ScriptBuilderResult};

impl ScriptBuilder {
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
        let bytes = crate::encode_integer(value);
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
            .map_or_else(|| value.to_signed_bytes_le(), crate::encode_integer);
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

    /// Emits a push operation for a local NeoVM stack item.
    pub fn emit_push_stack_item(&mut self, item: &StackItem) -> ScriptBuilderResult<&mut Self> {
        match item {
            StackItem::Null => {
                self.emit_opcode(OpCode::PUSHNULL);
            }
            StackItem::Boolean(value) => {
                self.emit_push_bool(*value);
            }
            StackItem::Integer(value) => {
                self.emit_push_bigint(value.to_bigint())?;
            }
            StackItem::ByteString(bytes) => {
                self.emit_push(bytes);
            }
            StackItem::Buffer(buffer) => {
                self.emit_push(&buffer.data());
            }
            StackItem::Array(array) => {
                let items = array.items();
                for item in items.iter().rev() {
                    self.emit_push_stack_item(item)?;
                }
                self.emit_push_int(items.len() as i64);
                self.emit_pack();
            }
            StackItem::Struct(structure) => {
                let items = structure.items();
                for item in items.iter().rev() {
                    self.emit_push_stack_item(item)?;
                }
                self.emit_push_int(items.len() as i64);
                self.emit_opcode(OpCode::PACKSTRUCT);
            }
            StackItem::Map(map) => {
                self.emit_opcode(OpCode::NEWMAP);
                for (key, value) in map.iter() {
                    self.emit_opcode(OpCode::DUP);
                    self.emit_push_stack_item(&key)?;
                    self.emit_push_stack_item(&value)?;
                    self.emit_opcode(OpCode::SETITEM);
                }
            }
            StackItem::Pointer(_) => {
                return Err(ScriptBuilderError::invalid_operation(
                    "Cannot serialize Pointer to script",
                ));
            }
            StackItem::InteropInterface(_) => {
                return Err(ScriptBuilderError::invalid_operation(
                    "Cannot serialize InteropInterface to script",
                ));
            }
        }

        Ok(self)
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
mod tests {
    use super::*;

    #[test]
    fn pushing_struct_emits_packstruct_while_array_emits_pack() {
        // Neo v3.10.1 SmartContract/Helper.cs EmitPush(StackItem) preserves the
        // compound kind by selecting PACKSTRUCT for Struct and PACK for Array.
        let mut structure = ScriptBuilder::new();
        structure
            .emit_push_stack_item(&StackItem::from_struct(vec![StackItem::from_i64(1)]))
            .unwrap();
        assert_eq!(
            structure.to_array().last(),
            Some(&OpCode::PACKSTRUCT.byte())
        );

        let mut array = ScriptBuilder::new();
        array
            .emit_push_stack_item(&StackItem::from_array(vec![StackItem::from_i64(1)]))
            .unwrap();
        assert_eq!(array.to_array().last(), Some(&OpCode::PACK.byte()));
    }
}
