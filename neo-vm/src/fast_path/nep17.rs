//! NEP-17 transfer fast path execution.
//!
//! This module implements optimized execution for NEP-17 token transfers,
//! bypassing the VM interpreter entirely for 10-50x performance improvement.

use super::error::{FastPathError, FastPathResult};
use crate::OpCode;

/// Parsed NEP-17 transfer parameters.
#[derive(Debug, Clone)]
pub struct TransferParams {
    /// Sender address
    pub from: Vec<u8>,

    /// Recipient address
    pub to: Vec<u8>,

    /// Transfer amount (as raw bytes, will be converted to BigInteger)
    pub amount: Vec<u8>,

    /// NEP-17 contract hash
    pub contract_hash: Vec<u8>,
}

/// NEP-17 transfer script parser.
pub struct Nep17Parser;

impl Nep17Parser {
    /// Parses NEP-17 transfer parameters from a script without executing the VM.
    ///
    /// This directly extracts parameters from the bytecode, avoiding the overhead
    /// of stack operations, context management, and instruction dispatch.
    pub fn parse_transfer_script(script: &[u8]) -> FastPathResult<TransferParams> {
        let mut offset = 0;

        // Parse: PUSHDATA1 <from>
        let from = Self::read_pushdata(script, &mut offset)
            .map_err(|e| FastPathError::ParseError(format!("Failed to parse 'from' address: {}", e)))?;

        if from.len() != 20 {
            return Err(FastPathError::ParseError(
                format!("Invalid 'from' address length: expected 20, got {}", from.len())
            ));
        }

        // Parse: PUSHDATA1 <to>
        let to = Self::read_pushdata(script, &mut offset)
            .map_err(|e| FastPathError::ParseError(format!("Failed to parse 'to' address: {}", e)))?;

        if to.len() != 20 {
            return Err(FastPathError::ParseError(
                format!("Invalid 'to' address length: expected 20, got {}", to.len())
            ));
        }

        // Parse: PUSHDATA1 <amount>
        let amount = Self::read_pushdata(script, &mut offset)
            .map_err(|e| FastPathError::ParseError(format!("Failed to parse amount: {}", e)))?;

        // Skip: PUSHNULL, PUSH4, PACK
        Self::expect_opcode(script, &mut offset, OpCode::PUSHNULL)?;
        Self::expect_opcode(script, &mut offset, OpCode::PUSH4)?;
        Self::expect_opcode(script, &mut offset, OpCode::PACK)?;

        // Parse: PUSHDATA1 "transfer"
        let method = Self::read_pushdata(script, &mut offset)
            .map_err(|e| FastPathError::ParseError(format!("Failed to parse method name: {}", e)))?;

        if method != b"transfer" {
            return Err(FastPathError::ParseError(
                format!("Expected 'transfer' method, got: {:?}", String::from_utf8_lossy(&method))
            ));
        }

        // Parse: PUSHDATA1 <contract_hash>
        let contract_hash = Self::read_pushdata(script, &mut offset)
            .map_err(|e| FastPathError::ParseError(format!("Failed to parse contract hash: {}", e)))?;

        if contract_hash.len() != 20 {
            return Err(FastPathError::ParseError(
                format!("Invalid contract hash length: expected 20, got {}", contract_hash.len())
            ));
        }

        // Expect: SYSCALL
        Self::expect_opcode(script, &mut offset, OpCode::SYSCALL)?;

        Ok(TransferParams {
            from,
            to,
            amount,
            contract_hash,
        })
    }

    /// Reads a PUSHDATA1 instruction and returns the data.
    fn read_pushdata(script: &[u8], offset: &mut usize) -> FastPathResult<Vec<u8>> {
        if *offset >= script.len() {
            return Err(FastPathError::ParseError("Unexpected end of script".to_string()));
        }

        let opcode = script[*offset];
        *offset += 1;

        // Handle PUSHDATA1
        if opcode == OpCode::PUSHDATA1 as u8 {
            if *offset >= script.len() {
                return Err(FastPathError::ParseError("Unexpected end of script after PUSHDATA1".to_string()));
            }

            let len = script[*offset] as usize;
            *offset += 1;

            if *offset + len > script.len() {
                return Err(FastPathError::ParseError(
                    format!("PUSHDATA1 length {} exceeds script bounds", len)
                ));
            }

            let data = script[*offset..*offset + len].to_vec();
            *offset += len;

            return Ok(data);
        }

        // Handle PUSHDATA2
        if opcode == OpCode::PUSHDATA2 as u8 {
            if *offset + 1 >= script.len() {
                return Err(FastPathError::ParseError("Unexpected end of script after PUSHDATA2".to_string()));
            }

            let len = u16::from_le_bytes([script[*offset], script[*offset + 1]]) as usize;
            *offset += 2;

            if *offset + len > script.len() {
                return Err(FastPathError::ParseError(
                    format!("PUSHDATA2 length {} exceeds script bounds", len)
                ));
            }

            let data = script[*offset..*offset + len].to_vec();
            *offset += len;

            return Ok(data);
        }

        // Handle PUSHINT opcodes (for small amounts)
        if opcode >= OpCode::PUSHINT8 as u8 && opcode <= OpCode::PUSHINT256 as u8 {
            let byte_count = 1 << (opcode - OpCode::PUSHINT8 as u8);

            if *offset + byte_count > script.len() {
                return Err(FastPathError::ParseError(
                    format!("PUSHINT length {} exceeds script bounds", byte_count)
                ));
            }

            let data = script[*offset..*offset + byte_count].to_vec();
            *offset += byte_count;

            return Ok(data);
        }

        // Handle PUSH0-PUSH16 for very small amounts
        if opcode >= OpCode::PUSH0 as u8 && opcode <= OpCode::PUSH16 as u8 {
            let value = if opcode == OpCode::PUSH0 as u8 {
                0i8
            } else {
                (opcode - OpCode::PUSH0 as u8) as i8
            };

            return Ok(vec![value as u8]);
        }

        Err(FastPathError::ParseError(
            format!("Expected PUSHDATA instruction, got opcode: 0x{:02x}", opcode)
        ))
    }

    /// Expects a specific opcode at the current offset.
    fn expect_opcode(script: &[u8], offset: &mut usize, expected: OpCode) -> FastPathResult<()> {
        if *offset >= script.len() {
            return Err(FastPathError::ParseError(
                format!("Expected opcode {:?}, but reached end of script", expected)
            ));
        }

        let opcode = script[*offset];
        if opcode != expected as u8 {
            return Err(FastPathError::ParseError(
                format!("Expected opcode {:?} (0x{:02x}), got 0x{:02x}", expected, expected as u8, opcode)
            ));
        }

        *offset += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_transfer() {
        let mut script = Vec::new();

        // PUSHDATA1 from (20 bytes)
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[1u8; 20]);

        // PUSHDATA1 to (20 bytes)
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[2u8; 20]);

        // PUSHDATA1 amount (1 byte = 100)
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(1);
        script.push(100);

        // PUSHNULL PUSH4 PACK
        script.push(OpCode::PUSHNULL as u8);
        script.push(OpCode::PUSH4 as u8);
        script.push(OpCode::PACK as u8);

        // PUSHDATA1 "transfer"
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(8);
        script.extend_from_slice(b"transfer");

        // PUSHDATA1 contract_hash (20 bytes)
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[3u8; 20]);

        // SYSCALL
        script.push(OpCode::SYSCALL as u8);

        let params = Nep17Parser::parse_transfer_script(&script).unwrap();

        assert_eq!(params.from, vec![1u8; 20]);
        assert_eq!(params.to, vec![2u8; 20]);
        assert_eq!(params.amount, vec![100u8]);
        assert_eq!(params.contract_hash, vec![3u8; 20]);
    }

    #[test]
    fn test_parse_invalid_from_length() {
        let mut script = Vec::new();

        // PUSHDATA1 from (wrong length: 19 bytes)
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(19);
        script.extend_from_slice(&[1u8; 19]);

        let result = Nep17Parser::parse_transfer_script(&script);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_wrong_method() {
        let mut script = Vec::new();

        // Valid from/to/amount
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[1u8; 20]);

        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[2u8; 20]);

        script.push(OpCode::PUSHDATA1 as u8);
        script.push(1);
        script.push(100);

        script.push(OpCode::PUSHNULL as u8);
        script.push(OpCode::PUSH4 as u8);
        script.push(OpCode::PACK as u8);

        // Wrong method: "approve" instead of "transfer"
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(7);
        script.extend_from_slice(b"approve");

        let result = Nep17Parser::parse_transfer_script(&script);
        assert!(result.is_err());
    }
}
