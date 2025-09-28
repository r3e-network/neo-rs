// Copyright (C) 2015-2025 The Neo Project.
//
// token_transfer_key.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, io::{ISerializable, MemoryReader, BinaryWriter}};
use serde::{Serialize, Deserialize};

/// Token transfer key implementation.
/// Matches C# TokenTransferKey class exactly
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenTransferKey {
    /// User script hash
    /// Matches C# UserScriptHash property
    pub user_script_hash: UInt160,
    
    /// Timestamp in milliseconds
    /// Matches C# TimestampMS property
    pub timestamp_ms: u64,
    
    /// Asset script hash
    /// Matches C# AssetScriptHash property
    pub asset_script_hash: UInt160,
    
    /// Block transfer notification index
    /// Matches C# BlockXferNotificationIndex property
    pub block_xfer_notification_index: u32,
}

impl TokenTransferKey {
    /// Creates a new TokenTransferKey instance.
    /// Matches C# constructor
    pub fn new(user_script_hash: UInt160, timestamp: u64, asset_script_hash: UInt160, xfer_index: u32) -> Self {
        Self {
            user_script_hash,
            timestamp_ms: timestamp,
            asset_script_hash,
            block_xfer_notification_index: xfer_index,
        }
    }
}

impl ISerializable for TokenTransferKey {
    fn size(&self) -> usize {
        20 + 8 + 20 + 4 // UInt160 + u64 + UInt160 + u32
    }
    
    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        // Write user script hash
        writer.write_all(&self.user_script_hash.to_bytes())?;
        
        // Write timestamp (big endian)
        let timestamp_be = self.timestamp_ms.to_be_bytes();
        writer.write_all(&timestamp_be)?;
        
        // Write asset script hash
        writer.write_all(&self.asset_script_hash.to_bytes())?;
        
        // Write block transfer notification index
        writer.write_all(&self.block_xfer_notification_index.to_le_bytes())?;
        
        Ok(())
    }
    
    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        // Read user script hash
        let mut user_hash_bytes = [0u8; 20];
        reader.read_exact(&mut user_hash_bytes)?;
        self.user_script_hash = UInt160::from_bytes(&user_hash_bytes);
        
        // Read timestamp (big endian)
        let mut timestamp_bytes = [0u8; 8];
        reader.read_exact(&mut timestamp_bytes)?;
        self.timestamp_ms = u64::from_be_bytes(timestamp_bytes);
        
        // Read asset script hash
        let mut asset_hash_bytes = [0u8; 20];
        reader.read_exact(&mut asset_hash_bytes)?;
        self.asset_script_hash = UInt160::from_bytes(&asset_hash_bytes);
        
        // Read block transfer notification index
        let mut index_bytes = [0u8; 4];
        reader.read_exact(&mut index_bytes)?;
        self.block_xfer_notification_index = u32::from_le_bytes(index_bytes);
        
        Ok(())
    }
}

impl Default for TokenTransferKey {
    fn default() -> Self {
        Self {
            user_script_hash: UInt160::zero(),
            timestamp_ms: 0,
            asset_script_hash: UInt160::zero(),
            block_xfer_notification_index: 0,
        }
    }
}