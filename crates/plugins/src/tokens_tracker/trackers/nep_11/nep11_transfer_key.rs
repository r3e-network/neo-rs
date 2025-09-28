// Copyright (C) 2015-2025 The Neo Project.
//
// nep11_transfer_key.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, ByteString, io::{ISerializable, MemoryReader, BinaryWriter}};
use super::super::token_transfer_key::TokenTransferKey;
use serde::{Serialize, Deserialize};
use std::cmp::Ordering;
use std::num::BigInt;

/// NEP-11 transfer key implementation.
/// Matches C# Nep11TransferKey class exactly
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nep11TransferKey {
    /// Base token transfer key
    /// Matches C# inheritance from TokenTransferKey
    base: TokenTransferKey,
    
    /// Token ID
    /// Matches C# Token field
    pub token: ByteString,
}

impl Nep11TransferKey {
    /// Creates a new Nep11TransferKey instance.
    /// Matches C# constructor
    pub fn new(user_script_hash: UInt160, timestamp: u64, asset_script_hash: UInt160, token_id: ByteString, xfer_index: u32) -> Self {
        Self {
            base: TokenTransferKey::new(user_script_hash, timestamp, asset_script_hash, xfer_index),
            token: token_id,
        }
    }
    
    /// Gets the user script hash.
    /// Matches C# UserScriptHash property
    pub fn user_script_hash(&self) -> UInt160 {
        self.base.user_script_hash
    }
    
    /// Gets the timestamp in milliseconds.
    /// Matches C# TimestampMS property
    pub fn timestamp_ms(&self) -> u64 {
        self.base.timestamp_ms
    }
    
    /// Gets the asset script hash.
    /// Matches C# AssetScriptHash property
    pub fn asset_script_hash(&self) -> UInt160 {
        self.base.asset_script_hash
    }
    
    /// Gets the block transfer notification index.
    /// Matches C# BlockXferNotificationIndex property
    pub fn block_xfer_notification_index(&self) -> u32 {
        self.base.block_xfer_notification_index
    }
    
    /// Gets the size of the serialized data.
    /// Matches C# Size property
    pub fn size(&self) -> usize {
        self.base.size() + self.token.get_var_size()
    }
}

impl Default for Nep11TransferKey {
    fn default() -> Self {
        Self {
            base: TokenTransferKey::default(),
            token: ByteString::new(),
        }
    }
}

impl PartialOrd for Nep11TransferKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nep11TransferKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare user script hash first
        let user_cmp = self.user_script_hash().cmp(&other.user_script_hash());
        if user_cmp != Ordering::Equal {
            return user_cmp;
        }
        
        // Compare timestamp second
        let timestamp_cmp = self.timestamp_ms().cmp(&other.timestamp_ms());
        if timestamp_cmp != Ordering::Equal {
            return timestamp_cmp;
        }
        
        // Compare asset script hash third
        let asset_cmp = self.asset_script_hash().cmp(&other.asset_script_hash());
        if asset_cmp != Ordering::Equal {
            return asset_cmp;
        }
        
        // Compare block transfer notification index fourth
        let index_cmp = self.block_xfer_notification_index().cmp(&other.block_xfer_notification_index());
        if index_cmp != Ordering::Equal {
            return index_cmp;
        }
        
        // Compare token by integer value last
        let self_token_int = self.token.get_integer();
        let other_token_int = other.token.get_integer();
        (self_token_int - other_token_int).signum().cmp(&0)
    }
}

impl ISerializable for Nep11TransferKey {
    fn size(&self) -> usize {
        self.size()
    }
    
    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        // Serialize base class
        self.base.serialize(writer)?;
        
        // Serialize token with var size
        self.write_var_bytes(writer, &self.token.get_bytes())?;
        
        Ok(())
    }
    
    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        // Deserialize base class
        self.base.deserialize(reader)?;
        
        // Deserialize token with var size
        let token_bytes = self.read_var_bytes(reader)?;
        self.token = ByteString::from_bytes(&token_bytes);
        
        Ok(())
    }
}

impl Nep11TransferKey {
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