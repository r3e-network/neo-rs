// Copyright (C) 2015-2025 The Neo Project.
//
// token_balance.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::io::{BinaryWriter, ISerializable, MemoryReader};
use serde::{Deserialize, Serialize};
use std::num::BigInt;

/// Token balance implementation.
/// Matches C# TokenBalance class exactly
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenBalance {
    /// Balance amount
    /// Matches C# Balance field
    pub balance: BigInt,

    /// Last updated block
    /// Matches C# LastUpdatedBlock field
    pub last_updated_block: u32,
}

impl TokenBalance {
    /// Creates a new TokenBalance instance.
    pub fn new() -> Self {
        Self {
            balance: BigInt::from(0),
            last_updated_block: 0,
        }
    }

    /// Creates a new TokenBalance with specified parameters.
    pub fn new_with_params(balance: BigInt, last_updated_block: u32) -> Self {
        Self {
            balance,
            last_updated_block,
        }
    }
}

impl ISerializable for TokenBalance {
    fn size(&self) -> usize {
        self.balance.get_var_size() + 4 // Balance + LastUpdatedBlock
    }

    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        // Write balance with var size
        self.write_var_bytes(writer, &self.balance.to_bytes_be().1)?;

        // Write last updated block
        writer.write_all(&self.last_updated_block.to_le_bytes())?;

        Ok(())
    }

    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        // Read balance with var size
        let balance_bytes = self.read_var_bytes(reader)?;
        self.balance = BigInt::from_bytes_be(num_bigint::Sign::Plus, &balance_bytes);

        // Read last updated block
        let mut block_bytes = [0u8; 4];
        reader.read_exact(&mut block_bytes)?;
        self.last_updated_block = u32::from_le_bytes(block_bytes);

        Ok(())
    }
}

impl TokenBalance {
    /// Writes variable-length bytes to writer.
    /// Matches C# WriteVarBytes method
    fn write_var_bytes(&self, writer: &mut dyn std::io::Write, data: &[u8]) -> Result<(), String> {
        if data.len() < 0xFD {
            writer.write_all(&[data.len() as u8])?;
        } else if data.len() <= 0xFFFF {
            writer.write_all(&[0xFD])?;
            writer.write_all(&(data.len() as u16).to_le_bytes())?;
        } else if data.len() <= 0xFFFFFFFF {
            writer.write_all(&[0xFE])?;
            writer.write_all(&(data.len() as u32).to_le_bytes())?;
        } else {
            writer.write_all(&[0xFF])?;
            writer.write_all(&(data.len() as u64).to_le_bytes())?;
        }
        writer.write_all(data)?;
        Ok(())
    }

    /// Reads variable-length bytes from reader.
    /// Matches C# ReadVarMemory method
    fn read_var_bytes(&self, reader: &mut dyn std::io::Read) -> Result<Vec<u8>, String> {
        let mut length_byte = [0u8; 1];
        reader.read_exact(&mut length_byte)?;

        let length = match length_byte[0] {
            len if len < 0xFD => len as usize,
            0xFD => {
                let mut bytes = [0u8; 2];
                reader.read_exact(&mut bytes)?;
                u16::from_le_bytes(bytes) as usize
            }
            0xFE => {
                let mut bytes = [0u8; 4];
                reader.read_exact(&mut bytes)?;
                u32::from_le_bytes(bytes) as usize
            }
            0xFF => {
                let mut bytes = [0u8; 8];
                reader.read_exact(&mut bytes)?;
                u64::from_le_bytes(bytes) as usize
            }
            _ => return Err("Invalid var length prefix".to_string()),
        };

        let mut data = vec![0u8; length];
        reader.read_exact(&mut data)?;
        Ok(data)
    }
}

impl Default for TokenBalance {
    fn default() -> Self {
        Self::new()
    }
}
