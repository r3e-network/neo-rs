//! Strict parser for the standard single-call NEP-17 transfer script.
//!
//! Parsing only proves that the script has the exact wallet-emitted shape. The
//! host must still check native-contract eligibility and execute all consensus
//! fees, witness checks, events, and failure semantics.
//!
//! Instruction decoding itself is delegated to
//! [`neo_vm::fast_path::InstructionCursor`]; this module owns only the NEP-17
//! interpretation of the decoded operands.

use neo_vm::fast_path::{EntryInstruction, FastPathError, FastPathResult, InstructionCursor};
use neo_vm::{OpCode, StackItem, interop_hash};
use num_bigint::BigInt;

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
        let mut parser = InstructionCursor::new(script);
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
        if !parser.is_exhausted() {
            return Err(parse_error(format!(
                "Trailing instruction or bytes at offset {}",
                parser.offset()
            )));
        }
        let entry_end_offset = parser.offset();
        Ok(TransferParams {
            from,
            to,
            amount,
            contract_hash,
            entry_instructions: parser.into_instructions(),
            entry_end_offset,
        })
    }
}

fn parse_error(message: String) -> FastPathError {
    FastPathError::ParseError(message)
}

#[cfg(test)]
pub(crate) fn standard_transfer_script(amount: i64) -> Vec<u8> {
    use neo_vm::ScriptBuilder;

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
            let script = standard_transfer_script(amount);
            let params = Nep17Parser::parse_transfer_script(&script)
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
            assert_eq!(params.entry_end_offset, script.len());
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

        let mut builder = neo_vm::ScriptBuilder::new();
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
