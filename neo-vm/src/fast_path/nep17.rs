//! Strict parser for the standard single-call NEP-17 transfer script.
//!
//! Parsing only proves that the script has the exact wallet-emitted shape. The
//! host must still check native-contract eligibility and execute all consensus
//! fees, witness checks, events, and failure semantics.

use super::error::{FastPathError, FastPathResult};
use crate::{OpCode, StackItem, interop_hash};
use num_bigint::BigInt;

/// One instruction in a parsed entry script, retained for canonical fee and
/// fault-location accounting during direct native dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryInstruction {
    /// Instruction opcode.
    pub opcode: OpCode,
    /// Byte offset of the instruction in the entry script.
    pub offset: usize,
}

/// Parsed NEP-17 transfer parameters and canonical execution metadata.
#[derive(Debug, Clone)]
pub struct TransferParams {
    /// Sender address, in Neo's 20-byte script-hash byte order.
    pub from: Vec<u8>,
    /// Recipient address, in Neo's 20-byte script-hash byte order.
    pub to: Vec<u8>,
    /// Canonical signed little-endian integer bytes (`0` is an empty vector).
    pub amount: Vec<u8>,
    /// NEP-17 contract hash, in Neo's 20-byte script-hash byte order.
    pub contract_hash: Vec<u8>,
    /// Every entry instruction in execution order, including ASSERT.
    pub entry_instructions: Vec<EntryInstruction>,
    /// Byte offset immediately after the final ASSERT.
    pub entry_end_offset: usize,
}

impl TransferParams {
    /// Returns the amount as the canonical VM integer stack item.
    #[must_use]
    pub fn amount_stack_item(&self) -> StackItem {
        StackItem::from_int(BigInt::from_signed_bytes_le(&self.amount))
    }
}

/// Strict NEP-17 transfer script parser.
pub struct Nep17Parser;

impl Nep17Parser {
    /// Parses the exact script emitted for a single wallet NEP-17 transfer.
    ///
    /// PUSHDATA variants, non-null data, non-ALL call flags, extra calls, and
    /// trailing instructions are rejected so callers can safely fall back to
    /// the canonical VM before changing state.
    pub fn parse_transfer_script(script: &[u8]) -> FastPathResult<TransferParams> {
        let mut parser = ScriptParser {
            script,
            offset: 0,
            instructions: Vec::new(),
        };
        // Wallet EmitTransfer pushes arguments in reverse logical order:
        // data, amount, to, from. PACK then produces [from, to, amount, data].
        parser.expect_opcode(OpCode::PUSHNULL)?;
        let amount = parser.read_integer()?;
        let to = parser.read_pushdata1("to")?;
        if to.len() != 20 {
            return Err(parse_error(format!(
                "Invalid 'to' address length: expected 20, got {}",
                to.len()
            )));
        }
        let from = parser.read_pushdata1("from")?;
        if from.len() != 20 {
            return Err(parse_error(format!(
                "Invalid 'from' address length: expected 20, got {}",
                from.len()
            )));
        }
        parser.expect_opcode(OpCode::PUSH4)?;
        parser.expect_opcode(OpCode::PACK)?;
        parser.expect_opcode(OpCode::PUSH15)?;
        let method = parser.read_pushdata1("method")?;
        if method != b"transfer" {
            return Err(parse_error(format!(
                "Expected 'transfer' method, got: {:?}",
                String::from_utf8_lossy(&method)
            )));
        }
        let contract_hash = parser.read_pushdata1("contract hash")?;
        if contract_hash.len() != 20 {
            return Err(parse_error(format!(
                "Invalid contract hash length: expected 20, got {}",
                contract_hash.len()
            )));
        }
        parser.expect_opcode(OpCode::SYSCALL)?;
        let syscall_hash = parser.read_u32()?;
        let expected_hash = interop_hash("System.Contract.Call");
        if syscall_hash != expected_hash {
            return Err(parse_error(format!(
                "Expected System.Contract.Call hash 0x{expected_hash:08x}, got 0x{syscall_hash:08x}"
            )));
        }
        parser.expect_opcode(OpCode::ASSERT)?;
        if parser.offset != script.len() {
            return Err(parse_error(format!(
                "Trailing instruction or bytes at offset {}",
                parser.offset
            )));
        }
        Ok(TransferParams {
            from,
            to,
            amount,
            contract_hash,
            entry_instructions: parser.instructions,
            entry_end_offset: parser.offset,
        })
    }
}

struct ScriptParser<'a> {
    script: &'a [u8],
    offset: usize,
    instructions: Vec<EntryInstruction>,
}

impl ScriptParser<'_> {
    fn read_byte(&mut self) -> FastPathResult<u8> {
        let byte = self.script.get(self.offset).copied().ok_or_else(|| {
            parse_error(format!(
                "Unexpected end of script at offset {}",
                self.offset
            ))
        })?;
        self.offset += 1;
        Ok(byte)
    }

    fn read_opcode(&mut self) -> FastPathResult<(OpCode, usize)> {
        let offset = self.offset;
        let byte = self.read_byte()?;
        let opcode = OpCode::try_from(byte)
            .map_err(|_| parse_error(format!("Invalid opcode 0x{byte:02x} at offset {offset}")))?;
        self.instructions.push(EntryInstruction { opcode, offset });
        Ok((opcode, offset))
    }

    fn expect_opcode(&mut self, expected: OpCode) -> FastPathResult<()> {
        let (opcode, offset) = self.read_opcode()?;
        if opcode != expected {
            return Err(parse_error(format!(
                "Expected opcode {expected:?} (0x{:02x}) at offset {offset}, got {opcode:?}",
                expected as u8
            )));
        }
        Ok(())
    }

    fn read_pushdata1(&mut self, field: &str) -> FastPathResult<Vec<u8>> {
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

    fn read_integer(&mut self) -> FastPathResult<Vec<u8>> {
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
                    "Expected integer push for amount at offset {offset}, got {opcode:?}"
                )));
            }
        };
        StackItem::from_int(value)
            .as_bytes()
            .map_err(|error| parse_error(format!("Invalid amount integer: {error}")))
    }

    fn read_u32(&mut self) -> FastPathResult<u32> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or_else(|| parse_error("Syscall hash offset overflow".to_string()))?;
        let bytes: [u8; 4] = self
            .script
            .get(self.offset..end)
            .ok_or_else(|| parse_error("Truncated System.Contract.Call hash".to_string()))?
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
pub(crate) fn standard_transfer_script(amount: i64) -> Vec<u8> {
    use crate::ScriptBuilder;

    let mut builder = ScriptBuilder::new();
    builder.emit_opcode(OpCode::PUSHNULL);
    builder.emit_push_int(amount);
    builder.emit_push(&[2u8; 20]);
    builder.emit_push(&[1u8; 20]);
    builder.emit_push_int(4);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(15);
    builder.emit_push(b"transfer");
    builder.emit_push(&[3u8; 20]);
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call is valid");
    builder.emit_opcode(OpCode::ASSERT);
    builder.to_array()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_wallet_transfer_and_normalizes_amount() {
        for amount in [-1000, -1, 0, 1, 16, 100, 1000, i64::MAX] {
            let params = Nep17Parser::parse_transfer_script(&standard_transfer_script(amount))
                .unwrap_or_else(|error| panic!("amount {amount}: {error}"));
            assert_eq!(params.from, vec![1u8; 20]);
            assert_eq!(params.to, vec![2u8; 20]);
            assert_eq!(params.contract_hash, vec![3u8; 20]);
            assert_eq!(
                params.amount_stack_item().as_int().unwrap(),
                BigInt::from(amount)
            );
            assert_eq!(
                params.entry_instructions.last().unwrap().opcode,
                OpCode::ASSERT
            );
        }
        assert!(
            Nep17Parser::parse_transfer_script(&standard_transfer_script(0))
                .unwrap()
                .amount
                .is_empty()
        );
    }

    #[test]
    fn rejects_wrong_hash_flags_assert_and_trailing_instructions() {
        let valid = standard_transfer_script(100);
        let syscall_offset = valid
            .windows(5)
            .position(|window| window[0] == OpCode::SYSCALL as u8)
            .expect("syscall");
        let mut wrong_hash = valid.clone();
        wrong_hash[syscall_offset + 1] ^= 1;
        assert!(Nep17Parser::parse_transfer_script(&wrong_hash).is_err());

        let push15 = valid
            .iter()
            .position(|byte| *byte == OpCode::PUSH15 as u8)
            .expect("call flags");
        let mut wrong_flags = valid.clone();
        wrong_flags[push15] = OpCode::PUSH14 as u8;
        assert!(Nep17Parser::parse_transfer_script(&wrong_flags).is_err());

        let mut missing_assert = valid.clone();
        missing_assert.pop();
        assert!(Nep17Parser::parse_transfer_script(&missing_assert).is_err());
        let mut trailing = valid;
        trailing.push(OpCode::RET as u8);
        assert!(Nep17Parser::parse_transfer_script(&trailing).is_err());
    }

    #[test]
    fn rejects_truncated_and_non_integer_amount_scripts() {
        let valid = standard_transfer_script(1000);
        for end in 0..valid.len() {
            assert!(Nep17Parser::parse_transfer_script(&valid[..end]).is_err());
        }

        let mut builder = crate::ScriptBuilder::new();
        builder.emit_opcode(OpCode::PUSHNULL);
        builder.emit_push(&[100]);
        builder.emit_push(&[2u8; 20]);
        builder.emit_push(&[1u8; 20]);
        builder.emit_push_int(4);
        builder.emit_opcode(OpCode::PACK);
        builder.emit_push_int(15);
        builder.emit_push(b"transfer");
        builder.emit_push(&[3u8; 20]);
        builder.emit_syscall("System.Contract.Call").unwrap();
        builder.emit_opcode(OpCode::ASSERT);
        assert!(Nep17Parser::parse_transfer_script(&builder.to_array()).is_err());
    }
}
