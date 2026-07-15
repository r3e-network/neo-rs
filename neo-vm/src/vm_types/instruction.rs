//! Shared NeoVM instruction parsing and operand decoding.

use alloc::{format, string::String, vec::Vec};

use super::OpCode;

/// Error returned while parsing an instruction or decoding its operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionError {
    kind: InstructionErrorKind,
    message: String,
}

/// Category of an instruction error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionErrorKind {
    /// Instruction byte parsing failed.
    Parse,
    /// Operand decoding failed.
    Operand,
}

impl InstructionError {
    /// Creates a new instruction parse error.
    pub fn parse(message: impl Into<String>) -> Self {
        Self {
            kind: InstructionErrorKind::Parse,
            message: message.into(),
        }
    }

    /// Creates a new operand decoding error.
    pub fn operand(message: impl Into<String>) -> Self {
        Self {
            kind: InstructionErrorKind::Operand,
            message: message.into(),
        }
    }

    /// Returns the error category.
    pub const fn kind(&self) -> InstructionErrorKind {
        self.kind
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl core::fmt::Display for InstructionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InstructionError {}

/// Result type for shared NeoVM instruction operations.
pub type InstructionResult<T> = Result<T, InstructionError>;

/// Represents a parsed NeoVM instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    /// The position of the instruction in the script.
    pub pointer: usize,
    /// The opcode of the instruction.
    pub opcode: OpCode,
    /// The operand data.
    pub operand: Vec<u8>,
    cached_size: usize,
}

impl Instruction {
    fn compute_size(opcode: OpCode, operand_len: usize) -> usize {
        1 + opcode.operand_prefix() + operand_len
    }

    /// Parses an instruction from a byte array.
    pub fn parse(script: &[u8], position: usize) -> InstructionResult<Self> {
        if position >= script.len() {
            return Err(InstructionError::parse("Position out of bounds"));
        }

        let opcode = script[position];
        let opcode = OpCode::try_from(opcode)
            .map_err(|_| InstructionError::parse(format!("Invalid opcode: {opcode}")))?;

        let operand = if matches!(
            opcode,
            OpCode::PUSHDATA1 | OpCode::PUSHDATA2 | OpCode::PUSHDATA4
        ) {
            let length_prefix_start = position + 1;
            match opcode {
                OpCode::PUSHDATA1 => {
                    if length_prefix_start >= script.len() {
                        return Err(InstructionError::parse("PUSHDATA1 missing length byte"));
                    }
                    let length = script[length_prefix_start] as usize;
                    let data_start = length_prefix_start + 1;
                    let data_end = data_start.checked_add(length).ok_or_else(|| {
                        InstructionError::parse("PUSHDATA1 operand size overflowed script bounds")
                    })?;
                    if data_end > script.len() {
                        return Err(InstructionError::parse(format!(
                            "PUSHDATA1 operand size exceeds script bounds: {} + {} > {}",
                            data_start,
                            length,
                            script.len()
                        )));
                    }
                    script[data_start..data_end].to_vec()
                }
                OpCode::PUSHDATA2 => {
                    if length_prefix_start + 1 >= script.len() {
                        return Err(InstructionError::parse("PUSHDATA2 missing length bytes"));
                    }
                    let length = u16::from_le_bytes([
                        script[length_prefix_start],
                        script[length_prefix_start + 1],
                    ]) as usize;
                    let data_start = length_prefix_start + 2;
                    let data_end = data_start.checked_add(length).ok_or_else(|| {
                        InstructionError::parse("PUSHDATA2 operand size overflowed script bounds")
                    })?;
                    if data_end > script.len() {
                        return Err(InstructionError::parse(format!(
                            "PUSHDATA2 operand size exceeds script bounds: {} + {} > {}",
                            data_start,
                            length,
                            script.len()
                        )));
                    }
                    script[data_start..data_end].to_vec()
                }
                OpCode::PUSHDATA4 => {
                    if length_prefix_start + 3 >= script.len() {
                        return Err(InstructionError::parse("PUSHDATA4 missing length bytes"));
                    }
                    let length = u32::from_le_bytes([
                        script[length_prefix_start],
                        script[length_prefix_start + 1],
                        script[length_prefix_start + 2],
                        script[length_prefix_start + 3],
                    ]) as usize;
                    let data_start = length_prefix_start + 4;
                    let data_end = data_start.checked_add(length).ok_or_else(|| {
                        InstructionError::parse("PUSHDATA4 operand size overflowed script bounds")
                    })?;
                    if data_end > script.len() {
                        return Err(InstructionError::parse(format!(
                            "PUSHDATA4 operand size exceeds script bounds: {} + {} > {}",
                            data_start,
                            length,
                            script.len()
                        )));
                    }
                    script[data_start..data_end].to_vec()
                }
                _ => {
                    return Err(InstructionError::parse(format!(
                        "Unexpected opcode in PUSHDATA handling: {opcode:?}"
                    )));
                }
            }
        } else {
            let operand_size = opcode.operand_size();
            let operand_start = position + 1;
            let operand_end = operand_start
                .checked_add(operand_size)
                .ok_or_else(|| InstructionError::parse("Operand size overflowed script bounds"))?;

            if operand_end > script.len() {
                return Err(InstructionError::parse(format!(
                    "Operand size exceeds script bounds for opcode: {opcode:?}"
                )));
            }

            script[operand_start..operand_end].to_vec()
        };

        let cached_size = Self::compute_size(opcode, operand.len());
        Ok(Self {
            pointer: position,
            opcode,
            operand,
            cached_size,
        })
    }

    /// Creates a new instruction with the given opcode and operand.
    pub fn new(opcode: OpCode, operand: &[u8]) -> Self {
        Self {
            pointer: 0,
            opcode,
            operand: operand.to_vec(),
            cached_size: Self::compute_size(opcode, operand.len()),
        }
    }

    /// Creates a RET instruction.
    pub fn ret() -> Self {
        Self::new(OpCode::RET, &[])
    }

    /// Returns the opcode of the instruction.
    pub const fn opcode(&self) -> OpCode {
        self.opcode
    }

    /// Returns the position of the instruction in the script.
    pub const fn pointer(&self) -> usize {
        self.pointer
    }

    /// Returns the operand data.
    pub fn operand_data(&self) -> &[u8] {
        &self.operand
    }

    /// Returns the operand as a specific type.
    pub fn operand_as<T: FromOperand>(&self) -> InstructionResult<T> {
        T::from_operand(&self.operand)
    }

    /// Returns the operand data as a slice.
    pub fn operand(&self) -> &[u8] {
        &self.operand
    }

    /// Reads an i16 operand from the instruction.
    pub fn read_i16_operand(&self) -> InstructionResult<i16> {
        self.operand_as::<i16>()
    }

    /// Reads an i32 operand from the instruction.
    pub fn read_i32_operand(&self) -> InstructionResult<i32> {
        self.operand_as::<i32>()
    }

    /// Reads a u8 operand from the instruction.
    pub fn read_u8_operand(&self) -> InstructionResult<u8> {
        self.operand_as::<u8>()
    }

    /// Reads an i8 operand from the instruction.
    pub fn read_i8_operand(&self) -> InstructionResult<i8> {
        self.operand_as::<i8>()
    }

    /// Reads an i64 operand from the instruction.
    pub fn read_i64_operand(&self) -> InstructionResult<i64> {
        self.operand_as::<i64>()
    }

    /// Returns the size of the instruction in bytes.
    pub const fn size(&self) -> usize {
        self.cached_size
    }

    /// Returns the first signed byte operand.
    pub fn token_i8(&self) -> i8 {
        self.operand.first().copied().unwrap_or(0) as i8
    }

    /// Returns the second signed byte operand.
    pub fn token_i8_1(&self) -> i8 {
        self.operand.get(1).copied().unwrap_or(0) as i8
    }

    /// Returns the first 32-bit signed operand.
    pub fn token_i32(&self) -> i32 {
        self.token_u32() as i32
    }

    /// Returns the second 32-bit signed operand.
    pub fn token_i32_1(&self) -> i32 {
        let mut bytes = [0u8; 4];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            *slot = *self.operand.get(4 + idx).unwrap_or(&0);
        }
        i32::from_le_bytes(bytes)
    }

    /// Returns the first 16-bit unsigned operand.
    pub fn token_u16(&self) -> u16 {
        let mut bytes = [0u8; 2];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            *slot = *self.operand.get(idx).unwrap_or(&0);
        }
        u16::from_le_bytes(bytes)
    }

    /// Returns the first 32-bit unsigned operand.
    pub fn token_u32(&self) -> u32 {
        let mut bytes = [0u8; 4];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            *slot = *self.operand.get(idx).unwrap_or(&0);
        }
        u32::from_le_bytes(bytes)
    }
}

/// A trait for types that can be converted from an operand.
pub trait FromOperand: Sized {
    /// Converts an operand to this type.
    fn from_operand(operand: &[u8]) -> InstructionResult<Self>;
}

impl FromOperand for i8 {
    fn from_operand(operand: &[u8]) -> InstructionResult<Self> {
        if operand.is_empty() {
            return Err(InstructionError::operand("Empty operand for i8"));
        }
        Ok(operand[0] as Self)
    }
}

impl FromOperand for u8 {
    fn from_operand(operand: &[u8]) -> InstructionResult<Self> {
        if operand.is_empty() {
            return Err(InstructionError::operand("Empty operand for u8"));
        }
        Ok(operand[0])
    }
}

impl FromOperand for i16 {
    fn from_operand(operand: &[u8]) -> InstructionResult<Self> {
        if operand.len() < 2 {
            return Err(InstructionError::operand("Operand too small for i16"));
        }
        Ok(Self::from_le_bytes([operand[0], operand[1]]))
    }
}

impl FromOperand for u16 {
    fn from_operand(operand: &[u8]) -> InstructionResult<Self> {
        if operand.len() < 2 {
            return Err(InstructionError::operand("Operand too small for u16"));
        }
        Ok(Self::from_le_bytes([operand[0], operand[1]]))
    }
}

impl FromOperand for i32 {
    fn from_operand(operand: &[u8]) -> InstructionResult<Self> {
        if operand.len() < 4 {
            return Err(InstructionError::operand("Operand too small for i32"));
        }
        Ok(Self::from_le_bytes([
            operand[0], operand[1], operand[2], operand[3],
        ]))
    }
}

impl FromOperand for u32 {
    fn from_operand(operand: &[u8]) -> InstructionResult<Self> {
        if operand.len() < 4 {
            return Err(InstructionError::operand("Operand too small for u32"));
        }
        Ok(Self::from_le_bytes([
            operand[0], operand[1], operand[2], operand[3],
        ]))
    }
}

impl FromOperand for i64 {
    fn from_operand(operand: &[u8]) -> InstructionResult<Self> {
        if operand.len() < 8 {
            return Err(InstructionError::operand("Operand too small for i64"));
        }
        Ok(Self::from_le_bytes([
            operand[0], operand[1], operand[2], operand[3], operand[4], operand[5], operand[6],
            operand[7],
        ]))
    }
}

impl FromOperand for u64 {
    fn from_operand(operand: &[u8]) -> InstructionResult<Self> {
        if operand.len() < 8 {
            return Err(InstructionError::operand("Operand too small for u64"));
        }
        Ok(Self::from_le_bytes([
            operand[0], operand[1], operand[2], operand[3], operand[4], operand[5], operand[6],
            operand[7],
        ]))
    }
}
