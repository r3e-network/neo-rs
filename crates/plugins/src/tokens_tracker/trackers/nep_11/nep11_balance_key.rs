// Copyright (C) 2015-2025 The Neo Project.
//
// nep11_balance_key.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, ByteString, io::{ISerializable, MemoryReader, BinaryWriter}};
use serde::{Serialize, Deserialize};
use std::cmp::Ordering;
use std::num::BigInt;

/// NEP-11 balance key implementation.
/// Matches C# Nep11BalanceKey class exactly
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nep11BalanceKey {
    /// User script hash
    /// Matches C# UserScriptHash field
    pub user_script_hash: UInt160,
    
    /// Asset script hash
    /// Matches C# AssetScriptHash field
    pub asset_script_hash: UInt160,
    
    /// Token ID
    /// Matches C# Token field
    pub token: ByteString,
}

impl Nep11BalanceKey {
    /// Creates a new Nep11BalanceKey instance.
    /// Matches C# constructor
    pub fn new(user_script_hash: UInt160, asset_script_hash: UInt160, token_id: ByteString) -> Self {
        Self {
            user_script_hash,
            asset_script_hash,
            token: token_id,
        }
    }
    
    /// Gets the size of the serialized data.
    /// Matches C# Size property
    pub fn size(&self) -> usize {
        20 + 20 + self.token.get_var_size() // UInt160 + UInt160 + Token
    }
}

impl Default for Nep11BalanceKey {
    fn default() -> Self {
        Self {
            user_script_hash: UInt160::zero(),
            asset_script_hash: UInt160::zero(),
            token: ByteString::new(),
        }
    }
}

impl PartialOrd for Nep11BalanceKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nep11BalanceKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare user script hash first
        let user_cmp = self.user_script_hash.cmp(&other.user_script_hash);
        if user_cmp != Ordering::Equal {
            return user_cmp;
        }
        
        // Compare asset script hash second
        let asset_cmp = self.asset_script_hash.cmp(&other.asset_script_hash);
        if asset_cmp != Ordering::Equal {
            return asset_cmp;
        }
        
        // Compare token by integer value
        let self_token_int = self.token.get_integer();
        let other_token_int = other.token.get_integer();
        (self_token_int - other_token_int).signum().cmp(&0)
    }
}

impl ISerializable for Nep11BalanceKey {
    fn size(&self) -> usize {
        self.size()
    }
    
    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        // Write user script hash
        writer.write_all(&self.user_script_hash.to_bytes())?;
        
        // Write asset script hash
        writer.write_all(&self.asset_script_hash.to_bytes())?;
        
        // Write token with var size
        self.write_var_bytes(writer, &self.token.get_bytes())?;
        
        Ok(())
    }
    
    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        // Read user script hash
        let mut user_hash_bytes = [0u8; 20];
        reader.read_exact(&mut user_hash_bytes)?;
        self.user_script_hash = UInt160::from_bytes(&user_hash_bytes);
        
        // Read asset script hash
        let mut asset_hash_bytes = [0u8; 20];
        reader.read_exact(&mut asset_hash_bytes)?;
        self.asset_script_hash = UInt160::from_bytes(&asset_hash_bytes);
        
        // Read token with var size
        let token_bytes = self.read_var_bytes(reader)?;
        self.token = ByteString::from_bytes(&token_bytes);
        
        Ok(())
    }
}

impl Nep11BalanceKey {
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
            },
            0xFE => {
                let mut bytes = [0u8; 4];
                reader.read_exact(&mut bytes)?;
                u32::from_le_bytes(bytes) as usize
            },
            0xFF => {
                let mut bytes = [0u8; 8];
                reader.read_exact(&mut bytes)?;
                u64::from_le_bytes(bytes) as usize
            },
            _ => return Err("Invalid var length prefix".to_string()),
        };
        
        let mut data = vec![0u8; length];
        reader.read_exact(&mut data)?;
        Ok(data)
    }
}