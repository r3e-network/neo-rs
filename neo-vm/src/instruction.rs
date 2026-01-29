//! Instruction module for the Neo Virtual Machine.
//!
//! This module provides instruction representation and parsing functionality.

use crate::error::{VmError, VmResult};
use crate::op_code::OpCode;
use neo_io;
// Hash size constant - used in future VM operations
// const HASH_SIZE: usize = 32;

/// Represents the size of an operand for an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperandSizePrefix(pub u8);

impl OperandSizePrefix {
    /// Returns the size of the operand in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        match self.0 {
            0 => 0,
            1 => 1,
            2 => 2,
            4 => 4,
            8 => 8,
            16 => 16,
            32 => 32,
            _ => 0,
        }
    }
}

/// Represents an instruction in the Neo Virtual Machine.
#[derive(Debug, Clone)]
pub struct Instruction {
    /// The position of the instruction in the script
    pub pointer: usize,

    /// The opcode of the instruction
    pub opcode: OpCode,

    /// The operand data
    pub operand: Vec<u8>,
}

impl Instruction {
    /// Parses an instruction from a byte array.
    pub fn parse(script: &[u8], position: usize) -> VmResult<Self> {
        if position >= script.len() {
            return Err(VmError::parse("Position out of bounds"));
        }

        let opcode = script[position];
        let opcode = OpCode::try_from(opcode)
            .map_err(|_| VmError::parse(format!("Invalid opcode: {opcode}")))?;

        let operand = if matches!(
            opcode,
            OpCode::PUSHDATA1 | OpCode::PUSHDATA2 | OpCode::PUSHDATA4
        ) {
            let length_prefix_start = position + 1;
            match opcode {
                OpCode::PUSHDATA1 => {
                    if length_prefix_start >= script.len() {
                        return Err(VmError::parse("PUSHDATA1 missing length byte"));
                    }
                    let length = script[length_prefix_start] as usize;
                    let data_start = length_prefix_start + 1;
                    let data_end = data_start.checked_add(length).ok_or_else(|| {
                        VmError::parse("PUSHDATA1 operand size overflowed script bounds")
                    })?;
                    if data_end > script.len() {
                        return Err(VmError::parse(format!(
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
                        return Err(VmError::parse("PUSHDATA2 missing length bytes"));
                    }
                    let length_bytes =
                        [script[length_prefix_start], script[length_prefix_start + 1]];
                    let length = u16::from_le_bytes(length_bytes) as usize;
                    let data_start = length_prefix_start + 2;
                    let data_end = data_start.checked_add(length).ok_or_else(|| {
                        VmError::parse("PUSHDATA2 operand size overflowed script bounds")
                    })?;
                    if data_end > script.len() {
                        return Err(VmError::parse(format!(
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
                        return Err(VmError::parse("PUSHDATA4 missing length bytes"));
                    }
                    let length_bytes = [
                        script[length_prefix_start],
                        script[length_prefix_start + 1],
                        script[length_prefix_start + 2],
                        script[length_prefix_start + 3],
                    ];
                    let length = u32::from_le_bytes(length_bytes) as usize;
                    let data_start = length_prefix_start + 4;
                    let data_end = data_start.checked_add(length).ok_or_else(|| {
                        VmError::parse("PUSHDATA4 operand size overflowed script bounds")
                    })?;
                    if data_end > script.len() {
                        return Err(VmError::parse(format!(
                            "PUSHDATA4 operand size exceeds script bounds: {} + {} > {}",
                            data_start,
                            length,
                            script.len()
                        )));
                    }
                    script[data_start..data_end].to_vec()
                }
                _ => {
                    return Err(VmError::parse(format!(
                        "Unexpected opcode in PUSHDATA handling: {opcode:?}"
                    )));
                }
            }
        } else {
            let operand_size = Self::get_operand_size(opcode);
            let operand_start = position + 1;
            let operand_end = operand_start + operand_size.size();

            if operand_end > script.len() {
                return Err(VmError::parse(format!(
                    "Operand size exceeds script bounds for opcode: {opcode:?}"
                )));
            }

            if operand_size.size() > 0 {
                script[operand_start..operand_end].to_vec()
            } else {
                Vec::new()
            }
        };

        Ok(Self {
            pointer: position,
            opcode,
            operand,
        })
    }

    /// Creates a new instruction with the given opcode and operand.
    /// This is primarily used for testing.
    #[must_use]
    pub fn new(opcode: OpCode, operand: &[u8]) -> Self {
        Self {
            pointer: 0,
            opcode,
            operand: operand.to_vec(),
        }
    }

    /// Parses an instruction from a neo-io `MemoryReader`.
    pub fn parse_from_neo_io_reader(reader: &mut neo_io::MemoryReader) -> VmResult<Self> {
        let pointer = reader.position();

        if pointer >= reader.len() {
            return Err(VmError::parse("Position out of bounds"));
        }

        let opcode = reader.read_byte()?;
        let opcode = OpCode::try_from(opcode)
            .map_err(|_| VmError::parse(format!("Invalid opcode: {opcode}")))?;

        let operand = if matches!(
            opcode,
            OpCode::PUSHDATA1 | OpCode::PUSHDATA2 | OpCode::PUSHDATA4
        ) {
            match opcode {
                OpCode::PUSHDATA1 => {
                    let length = reader.read_byte()? as usize;
                    if length == 0 {
                        Vec::new()
                    } else {
                        reader.read_bytes(length)?
                    }
                }
                OpCode::PUSHDATA2 => {
                    let length_bytes = reader.read_bytes(2)?;
                    let length = u16::from_le_bytes([length_bytes[0], length_bytes[1]]) as usize;
                    if length == 0 {
                        Vec::new()
                    } else {
                        reader.read_bytes(length)?
                    }
                }
                OpCode::PUSHDATA4 => {
                    let length_bytes = reader.read_bytes(4)?;
                    let length = u32::from_le_bytes([
                        length_bytes[0],
                        length_bytes[1],
                        length_bytes[2],
                        length_bytes[3],
                    ]) as usize;
                    if length == 0 {
                        Vec::new()
                    } else {
                        reader.read_bytes(length)?
                    }
                }
                _ => {
                    return Err(VmError::parse(format!(
                        "Unexpected opcode in PUSHDATA handling: {opcode:?}"
                    )));
                }
            }
        } else {
            let operand_size = Self::get_operand_size(opcode);
            if operand_size.size() > 0 {
                reader.read_bytes(operand_size.size())?.clone()
            } else {
                Vec::new()
            }
        };

        Ok(Self {
            pointer,
            opcode,
            operand,
        })
    }

    /// Parses an instruction from a reader.
    pub fn parse_from_reader(reader: &mut neo_io::MemoryReader) -> VmResult<Self> {
        let pointer = reader.position();

        if pointer >= reader.len() {
            return Err(VmError::parse("Position out of bounds"));
        }

        let opcode = reader.read_byte()?;
        let opcode = OpCode::try_from(opcode)
            .map_err(|_| VmError::parse(format!("Invalid opcode: {opcode}")))?;

        let operand = if matches!(
            opcode,
            OpCode::PUSHDATA1 | OpCode::PUSHDATA2 | OpCode::PUSHDATA4
        ) {
            match opcode {
                OpCode::PUSHDATA1 => {
                    let length = reader.read_byte()? as usize;
                    if length == 0 {
                        Vec::new()
                    } else {
                        reader.read_bytes(length)?
                    }
                }
                OpCode::PUSHDATA2 => {
                    let length_bytes = reader.read_bytes(2)?;
                    let length = u16::from_le_bytes([length_bytes[0], length_bytes[1]]) as usize;
                    if length == 0 {
                        Vec::new()
                    } else {
                        reader.read_bytes(length)?
                    }
                }
                OpCode::PUSHDATA4 => {
                    let length_bytes = reader.read_bytes(4)?;
                    let length = u32::from_le_bytes([
                        length_bytes[0],
                        length_bytes[1],
                        length_bytes[2],
                        length_bytes[3],
                    ]) as usize;
                    if length == 0 {
                        Vec::new()
                    } else {
                        reader.read_bytes(length)?
                    }
                }
                _ => {
                    return Err(VmError::parse(format!(
                        "Unexpected opcode in PUSHDATA handling: {opcode:?}"
                    )));
                }
            }
        } else {
            let operand_size = Self::get_operand_size(opcode);
            if operand_size.size() > 0 {
                reader.read_bytes(operand_size.size())?.clone()
            } else {
                Vec::new()
            }
        };

        Ok(Self {
            pointer,
            opcode,
            operand,
        })
    }

    /// Returns the opcode of the instruction.
    #[must_use]
    pub const fn opcode(&self) -> OpCode {
        self.opcode
    }

    /// Returns the position of the instruction in the script.
    #[must_use]
    pub const fn pointer(&self) -> usize {
        self.pointer
    }

    /// Returns the operand data.
    #[must_use]
    pub fn operand_data(&self) -> &[u8] {
        &self.operand
    }

    /// Returns the operand as a specific type.
    pub fn operand_as<T: FromOperand>(&self) -> VmResult<T> {
        T::from_operand(&self.operand)
    }

    /// Returns the operand data as a slice.
    #[must_use]
    pub fn operand(&self) -> &[u8] {
        &self.operand
    }

    /// Reads an i16 operand from the instruction.
    pub fn read_i16_operand(&self) -> VmResult<i16> {
        self.operand_as::<i16>()
    }

    /// Reads an i32 operand from the instruction.
    pub fn read_i32_operand(&self) -> VmResult<i32> {
        self.operand_as::<i32>()
    }

    /// Reads a u8 operand from the instruction.
    pub fn read_u8_operand(&self) -> VmResult<u8> {
        self.operand_as::<u8>()
    }

    /// Reads an i8 operand from the instruction.
    pub fn read_i8_operand(&self) -> VmResult<i8> {
        self.operand_as::<i8>()
    }

    /// Reads an i64 operand from the instruction.
    pub fn read_i64_operand(&self) -> VmResult<i64> {
        self.operand_as::<i64>()
    }

    /// Returns the size of the instruction in bytes.
    #[must_use]
    pub fn size(&self) -> usize {
        match self.opcode {
            OpCode::PUSHDATA1 => 1 + 1 + self.operand.len(),
            OpCode::PUSHDATA2 => 1 + 2 + self.operand.len(),
            OpCode::PUSHDATA4 => 1 + 4 + self.operand.len(),
            _ => {
                1 + self.operand.len() // Opcode + operand
            }
        }
    }

    /// Returns the operand size for the given opcode.
    const fn get_operand_size(opcode: OpCode) -> OperandSizePrefix {
        match opcode {
            // PUSH instructions with fixed operand sizes
            OpCode::PUSHINT8 => OperandSizePrefix(1),
            OpCode::PUSHINT16 => OperandSizePrefix(2),
            OpCode::PUSHINT32 => OperandSizePrefix(4),
            OpCode::PUSHINT64 => OperandSizePrefix(8),
            OpCode::PUSHINT128 => OperandSizePrefix(16),
            OpCode::PUSHINT256 => OperandSizePrefix(32),
            OpCode::PUSHA => OperandSizePrefix(4),
            OpCode::PUSHDATA1 => OperandSizePrefix(1),
            OpCode::PUSHDATA2 => OperandSizePrefix(2),
            OpCode::PUSHDATA4 => OperandSizePrefix(4),
            OpCode::TRY => OperandSizePrefix(4),
            // Jump instructions with 1-byte offset
            OpCode::JMP
            | OpCode::JMPIF
            | OpCode::JMPIFNOT
            | OpCode::CALL
            | OpCode::JMPEQ
            | OpCode::JMPNE
            | OpCode::JMPGT
            | OpCode::JMPGE
            | OpCode::JMPLT
            | OpCode::JMPLE
            | OpCode::ENDTRY => OperandSizePrefix(1),
            OpCode::CALLT => OperandSizePrefix(2),
            // Jump instructions with 4-byte offset
            OpCode::JMP_L
            | OpCode::JMPIF_L
            | OpCode::JMPIFNOT_L
            | OpCode::CALL_L
            | OpCode::JMPEQ_L
            | OpCode::JMPNE_L
            | OpCode::JMPGT_L
            | OpCode::JMPGE_L
            | OpCode::JMPLT_L
            | OpCode::JMPLE_L
            | OpCode::ENDTRY_L => OperandSizePrefix(4),
            OpCode::TRY_L => OperandSizePrefix(8),
            OpCode::SYSCALL => OperandSizePrefix(4),
            // Slot operations with operands
            OpCode::INITSLOT => OperandSizePrefix(2), // local_count (1 byte) + argument_count (1 byte)
            OpCode::INITSSLOT => OperandSizePrefix(1), // static_count (1 byte)
            OpCode::LDSFLD | OpCode::STSFLD => OperandSizePrefix(1), // index (1 byte)
            OpCode::LDLOC | OpCode::STLOC => OperandSizePrefix(1), // index (1 byte)
            OpCode::LDARG | OpCode::STARG => OperandSizePrefix(1), // index (1 byte)
            // Type operations with operands
            OpCode::CONVERT | OpCode::ISTYPE => OperandSizePrefix(1), // type (1 byte)
            // Compound operations with operands
            OpCode::NEWARRAY_T => OperandSizePrefix(1), // type (1 byte)
            _ => OperandSizePrefix(0),
        }
    }

    /// Creates a RET instruction.
    #[must_use]
    pub fn token_i8(&self) -> i8 {
        self.operand.first().copied().unwrap_or(0) as i8
    }

    /// Returns the second signed byte operand (used by TRY instructions).
    #[must_use]
    pub fn token_i8_1(&self) -> i8 {
        self.operand.get(1).copied().unwrap_or(0) as i8
    }

    /// Returns the first 32-bit signed operand.
    #[must_use]
    pub fn token_i32(&self) -> i32 {
        self.token_u32() as i32
    }

    /// Returns the second 32-bit signed operand.
    #[must_use]
    pub fn token_i32_1(&self) -> i32 {
        let mut bytes = [0u8; 4];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            *slot = *self.operand.get(4 + idx).unwrap_or(&0);
        }
        i32::from_le_bytes(bytes)
    }

    /// Returns the first 16-bit unsigned operand.
    #[must_use]
    pub fn token_u16(&self) -> u16 {
        let mut bytes = [0u8; 2];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            *slot = *self.operand.get(idx).unwrap_or(&0);
        }
        u16::from_le_bytes(bytes)
    }

    /// Returns the first 32-bit unsigned operand.
    #[must_use]
    pub fn token_u32(&self) -> u32 {
        let mut bytes = [0u8; 4];
        for (idx, slot) in bytes.iter_mut().enumerate() {
            *slot = *self.operand.get(idx).unwrap_or(&0);
        }
        u32::from_le_bytes(bytes)
    }

    /// Creates a RET instruction.
    #[must_use]
    pub const fn ret() -> Self {
        Self {
            pointer: 0,
            opcode: OpCode::RET,
            operand: Vec::new(),
        }
    }
}

/// A trait for types that can be converted from an operand.
pub trait FromOperand: Sized {
    /// Converts an operand to this type.
    fn from_operand(operand: &[u8]) -> VmResult<Self>;
}

impl FromOperand for i8 {
    fn from_operand(operand: &[u8]) -> VmResult<Self> {
        if operand.is_empty() {
            return Err(VmError::invalid_operand_msg("Empty operand for i8"));
        }
        Ok(operand[0] as Self)
    }
}

impl FromOperand for u8 {
    fn from_operand(operand: &[u8]) -> VmResult<Self> {
        if operand.is_empty() {
            return Err(VmError::invalid_operand_msg("Empty operand for u8"));
        }
        Ok(operand[0])
    }
}

impl FromOperand for i16 {
    fn from_operand(operand: &[u8]) -> VmResult<Self> {
        if operand.len() < 2 {
            return Err(VmError::invalid_operand_msg("Operand too small for i16"));
        }
        Ok(Self::from_le_bytes([operand[0], operand[1]]))
    }
}

impl FromOperand for u16 {
    fn from_operand(operand: &[u8]) -> VmResult<Self> {
        if operand.len() < 2 {
            return Err(VmError::invalid_operand_msg("Operand too small for u16"));
        }
        Ok(Self::from_le_bytes([operand[0], operand[1]]))
    }
}

impl FromOperand for i32 {
    fn from_operand(operand: &[u8]) -> VmResult<Self> {
        if operand.len() < 4 {
            return Err(VmError::invalid_operand_msg("Operand too small for i32"));
        }
        Ok(Self::from_le_bytes([
            operand[0], operand[1], operand[2], operand[3],
        ]))
    }
}

impl FromOperand for u32 {
    fn from_operand(operand: &[u8]) -> VmResult<Self> {
        if operand.len() < 4 {
            return Err(VmError::invalid_operand_msg("Operand too small for u32"));
        }
        Ok(Self::from_le_bytes([
            operand[0], operand[1], operand[2], operand[3],
        ]))
    }
}

impl FromOperand for i64 {
    fn from_operand(operand: &[u8]) -> VmResult<Self> {
        if operand.len() < 8 {
            return Err(VmError::invalid_operand_msg("Operand too small for i64"));
        }
        Ok(Self::from_le_bytes([
            operand[0], operand[1], operand[2], operand[3], operand[4], operand[5], operand[6],
            operand[7],
        ]))
    }
}

impl FromOperand for u64 {
    fn from_operand(operand: &[u8]) -> VmResult<Self> {
        if operand.len() < 8 {
            return Err(VmError::invalid_operand_msg("Operand too small for u64"));
        }
        Ok(Self::from_le_bytes([
            operand[0], operand[1], operand[2], operand[3], operand[4], operand[5], operand[6],
            operand[7],
        ]))
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_parsing() {
        let script = vec![
            OpCode::PUSH1 as u8,
            OpCode::JMP as u8,
            0x10, // JMP to offset 0x10 (1-byte offset)
            OpCode::PUSHDATA1 as u8,
            0x03,
            0x01,
            0x02,
            0x03, // PUSHDATA1 with 3 bytes: [1, 2, 3]
        ];

        // Parse PUSH1
        let instruction = Instruction::parse(&script, 0).expect("VM operation should succeed");
        assert_eq!(instruction.opcode(), OpCode::PUSH1);
        assert_eq!(instruction.size(), 1);
        assert!(instruction.operand_data().is_empty());

        // Parse JMP
        let instruction = Instruction::parse(&script, 1).expect("VM operation should succeed");
        assert_eq!(instruction.opcode(), OpCode::JMP);
        assert_eq!(instruction.size(), 2); // Opcode + 1-byte offset
        assert_eq!(instruction.operand_data(), &[0x10]);
        assert_eq!(
            instruction
                .operand_as::<i8>()
                .expect("VM operation should succeed"),
            16
        );

        // Parse PUSHDATA1
        let instruction = Instruction::parse(&script, 3).expect("Operation failed");
        assert_eq!(instruction.opcode(), OpCode::PUSHDATA1);
        assert_eq!(instruction.size(), 5); // Opcode + length byte + 3 data bytes
        assert_eq!(instruction.operand_data(), &[0x01, 0x02, 0x03]);
        assert_eq!(instruction.operand_data().len(), 3);
    }

    #[test]
    fn test_instruction_parsing_from_reader() {
        let script = vec![
            OpCode::PUSH1 as u8,
            OpCode::JMP as u8,
            0x10, // JMP with 1-byte offset
            OpCode::PUSHDATA1 as u8,
            0x03,
            0x01,
            0x02,
            0x03,
        ];

        let mut reader = neo_io::MemoryReader::new(&script);

        // Parse PUSH1
        let instruction =
            Instruction::parse_from_reader(&mut reader).expect("VM operation should succeed");
        assert_eq!(instruction.opcode(), OpCode::PUSH1);
        assert_eq!(reader.position(), 1);

        // Parse JMP
        let instruction =
            Instruction::parse_from_reader(&mut reader).expect("VM operation should succeed");
        assert_eq!(instruction.opcode(), OpCode::JMP);
        assert_eq!(reader.position(), 3); // Position after 1-byte opcode + 1-byte operand
        assert_eq!(
            instruction
                .operand_as::<i8>()
                .expect("VM operation should succeed"),
            16
        );

        // Parse PUSHDATA1
        let instruction =
            Instruction::parse_from_reader(&mut reader).expect("VM operation should succeed");
        assert_eq!(instruction.opcode(), OpCode::PUSHDATA1);
        assert_eq!(reader.position(), 8); // Position after 1-byte opcode + 1-byte size + 3 data bytes
        assert_eq!(instruction.operand_data(), &[0x01, 0x02, 0x03]);
        assert_eq!(instruction.operand_data().len(), 3);
    }

    #[test]
    fn test_operand_size() {
        // Instructions with no operand
        assert_eq!(Instruction::get_operand_size(OpCode::PUSH1).size(), 0);
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHNULL).size(), 0);

        // PUSH instructions with fixed operand sizes
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHINT8).size(), 1);
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHINT16).size(), 2);
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHINT32).size(), 4);
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHINT64).size(), 8);
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHINT128).size(), 16);
        assert_eq!(
            Instruction::get_operand_size(OpCode::PUSHINT256).size(),
            32 // HASH_SIZE constant
        );
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHA).size(), 4);

        assert_eq!(Instruction::get_operand_size(OpCode::PUSHDATA1).size(), 1);
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHDATA2).size(), 2);
        assert_eq!(Instruction::get_operand_size(OpCode::PUSHDATA4).size(), 4);

        // Jump instructions with 1-byte offset
        assert_eq!(Instruction::get_operand_size(OpCode::JMP).size(), 1);
        assert_eq!(Instruction::get_operand_size(OpCode::JMPIF).size(), 1);
        assert_eq!(Instruction::get_operand_size(OpCode::JMPIFNOT).size(), 1);
        assert_eq!(Instruction::get_operand_size(OpCode::CALL).size(), 1);

        // Jump instructions with 4-byte offset
        assert_eq!(Instruction::get_operand_size(OpCode::JMP_L).size(), 4);
        assert_eq!(Instruction::get_operand_size(OpCode::JMPIF_L).size(), 4);
        assert_eq!(Instruction::get_operand_size(OpCode::JMPIFNOT_L).size(), 4);
        assert_eq!(Instruction::get_operand_size(OpCode::CALL_L).size(), 4);
        assert_eq!(Instruction::get_operand_size(OpCode::SYSCALL).size(), 4);
    }

    #[test]
    fn test_operand_conversion() {
        // Test i8/u8
        let operand = vec![0x42];
        assert_eq!(
            i8::from_operand(&operand).expect("VM operation should succeed"),
            0x42
        );
        assert_eq!(
            u8::from_operand(&operand).expect("VM operation should succeed"),
            0x42
        );

        // Test i16/u16
        let operand = vec![0x42, 0x01];
        assert_eq!(
            i16::from_operand(&operand).expect("VM operation should succeed"),
            0x0142
        );
        assert_eq!(
            u16::from_operand(&operand).expect("VM operation should succeed"),
            0x0142
        );

        // Test i32/u32
        let operand = vec![0x42, 0x01, 0x00, 0x00];
        assert_eq!(
            i32::from_operand(&operand).expect("VM operation should succeed"),
            0x00000142
        );
        assert_eq!(
            u32::from_operand(&operand).expect("VM operation should succeed"),
            0x00000142
        );

        // Test i64/u64
        let operand = vec![0x42, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            i64::from_operand(&operand).expect("VM operation should succeed"),
            0x0000000000000142
        );
        assert_eq!(
            u64::from_operand(&operand).expect("VM operation should succeed"),
            0x0000000000000142
        );

        // Test error cases
        let operand = vec![];
        assert!(i8::from_operand(&operand).is_err());
        assert!(u8::from_operand(&operand).is_err());

        let operand = vec![0x42];
        assert!(i16::from_operand(&operand).is_err());
        assert!(u16::from_operand(&operand).is_err());
    }
}
