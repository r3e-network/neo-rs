//! Generic, semantics-free cursor over Neo VM bytecode.
//!
//! The cursor only walks instructions, enforces expected opcodes, and decodes
//! `PUSHDATA1`/integer pushes. It deliberately owns no application semantics:
//! template interpreters (for example the strict NEP-17 transfer template) live
//! with the application engine in `neo-smart-contract` and consume this cursor.

use super::error::{FastPathError, FastPathResult};
use crate::{OpCode, StackItem};
use num_bigint::BigInt;

/// One decoded instruction, retained for canonical fee and fault-location
/// accounting during direct native dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryInstruction {
    /// Instruction opcode.
    pub opcode: OpCode,
    /// Byte offset of the instruction in the entry script.
    pub offset: usize,
}

/// Strict, forward-only cursor over a candidate entry script.
///
/// Every read either consumes the exact bytes the script declares or returns
/// [`FastPathError::ParseError`], so callers can safely fall back to the
/// canonical VM before changing state.
pub struct InstructionCursor<'a> {
    script: &'a [u8],
    offset: usize,
    instructions: Vec<EntryInstruction>,
}

impl<'a> InstructionCursor<'a> {
    /// Creates a cursor positioned at the first byte of `script`.
    pub fn new(script: &'a [u8]) -> Self {
        Self {
            script,
            offset: 0,
            instructions: Vec::new(),
        }
    }

    /// Returns the byte offset of the next unread instruction.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns every opcode decoded so far, in execution order.
    pub fn instructions(&self) -> &[EntryInstruction] {
        &self.instructions
    }

    /// Consumes the cursor and returns the decoded instruction table.
    pub fn into_instructions(self) -> Vec<EntryInstruction> {
        self.instructions
    }

    /// Returns true when the whole script has been consumed.
    pub fn is_exhausted(&self) -> bool {
        self.offset == self.script.len()
    }

    /// Reads one raw byte.
    pub fn read_byte(&mut self) -> FastPathResult<u8> {
        let byte = self.script.get(self.offset).copied().ok_or_else(|| {
            parse_error(format!(
                "Unexpected end of script at offset {}",
                self.offset
            ))
        })?;
        self.offset += 1;
        Ok(byte)
    }

    /// Reads one opcode and records it in the instruction table.
    pub fn read_opcode(&mut self) -> FastPathResult<(OpCode, usize)> {
        let offset = self.offset;
        let byte = self.read_byte()?;
        let opcode = OpCode::try_from(byte)
            .map_err(|_| parse_error(format!("Invalid opcode 0x{byte:02x} at offset {offset}")))?;
        self.instructions.push(EntryInstruction { opcode, offset });
        Ok((opcode, offset))
    }

    /// Reads one opcode and requires it to equal `expected`.
    pub fn expect_opcode(&mut self, expected: OpCode) -> FastPathResult<()> {
        let (opcode, offset) = self.read_opcode()?;
        if opcode != expected {
            return Err(parse_error(format!(
                "Expected opcode {expected:?} (0x{:02x}) at offset {offset}, got {opcode:?}",
                expected as u8
            )));
        }
        Ok(())
    }

    /// Reads a `PUSHDATA1` literal, labelling failures with `field`.
    pub fn read_pushdata1(&mut self, field: &str) -> FastPathResult<Vec<u8>> {
        let (opcode, offset) = self.read_opcode()?;
        if opcode != OpCode::PUSHDATA1 {
            return Err(parse_error(format!(
                "Expected PUSHDATA1 for {field} at offset {offset}, got {opcode:?}"
            )));
        }
        let length = usize::from(self.read_byte()?);
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| parse_error(format!("PUSHDATA1 length overflow for {field}")))?;
        let data = self.script.get(self.offset..end).ok_or_else(|| {
            parse_error(format!(
                "PUSHDATA1 length {length} for {field} exceeds script bounds"
            ))
        })?;
        self.offset = end;
        Ok(data.to_vec())
    }

    /// Reads an integer push and returns its canonical signed little-endian
    /// bytes (`0` is the empty vector).
    pub fn read_integer(&mut self) -> FastPathResult<Vec<u8>> {
        let (opcode, offset) = self.read_opcode()?;
        let value = match opcode {
            OpCode::PUSHM1 => BigInt::from(-1),
            OpCode::PUSH0 => BigInt::from(0),
            opcode
                if opcode as u8 >= OpCode::PUSH1 as u8 && opcode as u8 <= OpCode::PUSH16 as u8 =>
            {
                BigInt::from(opcode as u8 - OpCode::PUSH0 as u8)
            }
            opcode
                if opcode as u8 >= OpCode::PUSHINT8 as u8
                    && opcode as u8 <= OpCode::PUSHINT256 as u8 =>
            {
                let length = 1usize << (opcode as u8 - OpCode::PUSHINT8 as u8);
                let end = self
                    .offset
                    .checked_add(length)
                    .ok_or_else(|| parse_error("PUSHINT length overflow".to_string()))?;
                let bytes = self.script.get(self.offset..end).ok_or_else(|| {
                    parse_error(format!(
                        "PUSHINT length {length} at offset {offset} exceeds script bounds"
                    ))
                })?;
                self.offset = end;
                BigInt::from_signed_bytes_le(bytes)
            }
            _ => {
                return Err(parse_error(format!(
                    "Expected integer push at offset {offset}, got {opcode:?}"
                )));
            }
        };
        StackItem::from_int(value)
            .as_bytes()
            .map_err(|error| parse_error(format!("Invalid integer push: {error}")))
    }

    /// Reads a little-endian `u32` operand.
    pub fn read_u32(&mut self) -> FastPathResult<u32> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or_else(|| parse_error("u32 operand offset overflow".to_string()))?;
        let bytes: [u8; 4] = self
            .script
            .get(self.offset..end)
            .ok_or_else(|| parse_error("Truncated u32 operand".to_string()))?
            .try_into()
            .expect("slice length checked to four bytes");
        self.offset = end;
        Ok(u32::from_le_bytes(bytes))
    }
}

fn parse_error(message: String) -> FastPathError {
    FastPathError::ParseError(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_offsets_for_every_decoded_instruction() {
        let script = [OpCode::PUSH1 as u8, OpCode::PUSH2 as u8, OpCode::ADD as u8];
        let mut cursor = InstructionCursor::new(&script);

        cursor.expect_opcode(OpCode::PUSH1).unwrap();
        cursor.expect_opcode(OpCode::PUSH2).unwrap();
        cursor.expect_opcode(OpCode::ADD).unwrap();

        assert!(cursor.is_exhausted());
        assert_eq!(
            cursor.instructions(),
            &[
                EntryInstruction {
                    opcode: OpCode::PUSH1,
                    offset: 0
                },
                EntryInstruction {
                    opcode: OpCode::PUSH2,
                    offset: 1
                },
                EntryInstruction {
                    opcode: OpCode::ADD,
                    offset: 2
                },
            ]
        );
    }

    #[test]
    fn rejects_wrong_opcode_and_truncated_pushdata() {
        let mut cursor = InstructionCursor::new(&[OpCode::PUSH1 as u8]);
        assert!(cursor.expect_opcode(OpCode::PUSH2).is_err());

        let mut cursor = InstructionCursor::new(&[OpCode::PUSHDATA1 as u8, 4, 1, 2]);
        assert!(cursor.read_pushdata1("literal").is_err());
    }

    #[test]
    fn decodes_integer_pushes_into_canonical_bytes() {
        for (opcode, expected) in [
            (OpCode::PUSHM1, BigInt::from(-1)),
            (OpCode::PUSH0, BigInt::from(0)),
            (OpCode::PUSH16, BigInt::from(16)),
        ] {
            let script = [opcode as u8];
            let mut cursor = InstructionCursor::new(&script);
            let bytes = cursor.read_integer().unwrap();
            assert_eq!(BigInt::from_signed_bytes_le(&bytes), expected);
        }
        let zero_script = [OpCode::PUSH0 as u8];
        let mut zero = InstructionCursor::new(&zero_script);
        assert!(zero.read_integer().unwrap().is_empty());
    }
}
