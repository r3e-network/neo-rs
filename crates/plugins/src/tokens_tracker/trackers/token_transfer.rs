// Copyright (C) 2015-2025 The Neo Project.
//
// token_transfer.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{
    io::{BinaryWriter, ISerializable, MemoryReader},
    UInt160, UInt256,
};
use serde::{Deserialize, Serialize};
use std::num::BigInt;

/// Token transfer implementation.
/// Matches C# TokenTransfer class exactly
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenTransfer {
    /// User script hash
    /// Matches C# UserScriptHash field
    pub user_script_hash: UInt160,

    /// Block index
    /// Matches C# BlockIndex field
    pub block_index: u32,

    /// Transaction hash
    /// Matches C# TxHash field
    pub tx_hash: UInt256,

    /// Amount
    /// Matches C# Amount field
    pub amount: BigInt,
}

impl TokenTransfer {
    /// Creates a new TokenTransfer instance.
    pub fn new() -> Self {
        Self {
            user_script_hash: UInt160::zero(),
            block_index: 0,
            tx_hash: UInt256::zero(),
            amount: BigInt::from(0),
        }
    }

    /// Creates a new TokenTransfer with specified parameters.
    pub fn new_with_params(
        user_script_hash: UInt160,
        block_index: u32,
        tx_hash: UInt256,
        amount: BigInt,
    ) -> Self {
        Self {
            user_script_hash,
            block_index,
            tx_hash,
            amount,
        }
    }
}

impl ISerializable for TokenTransfer {
    fn size(&self) -> usize {
        20 + 4 + 32 + self.amount.get_var_size() // UInt160 + u32 + UInt256 + Amount
    }

    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        // Write user script hash
        writer.write_all(&self.user_script_hash.to_bytes())?;

        // Write block index
        writer.write_all(&self.block_index.to_le_bytes())?;

        // Write transaction hash
        writer.write_all(&self.tx_hash.to_bytes())?;

        // Write amount with var size
        self.write_var_bytes(writer, &self.amount.to_bytes_be().1)?;

        Ok(())
    }

    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        // Read user script hash
        let mut user_hash_bytes = [0u8; 20];
        reader.read_exact(&mut user_hash_bytes)?;
        self.user_script_hash = UInt160::from_bytes(&user_hash_bytes);

        // Read block index
        let mut block_bytes = [0u8; 4];
        reader.read_exact(&mut block_bytes)?;
        self.block_index = u32::from_le_bytes(block_bytes);

        // Read transaction hash
        let mut tx_hash_bytes = [0u8; 32];
        reader.read_exact(&mut tx_hash_bytes)?;
        self.tx_hash = UInt256::from_bytes(&tx_hash_bytes);

        // Read amount with var size
        let amount_bytes = self.read_var_bytes(reader)?;
        self.amount = BigInt::from_bytes_be(num_bigint::Sign::Plus, &amount_bytes);

        Ok(())
    }
}

impl TokenTransfer {
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

impl Default for TokenTransfer {
    fn default() -> Self {
        Self::new()
    }
}
