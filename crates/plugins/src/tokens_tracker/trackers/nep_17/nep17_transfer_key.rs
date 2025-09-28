// Copyright (C) 2015-2025 The Neo Project.
//
// nep17_transfer_key.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, io::{ISerializable, MemoryReader, BinaryWriter}};
use super::super::token_transfer_key::TokenTransferKey;
use serde::{Serialize, Deserialize};
use std::cmp::Ordering;

/// NEP-17 transfer key implementation.
/// Matches C# Nep17TransferKey class exactly
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Nep17TransferKey {
    /// Base token transfer key
    /// Matches C# inheritance from TokenTransferKey
    base: TokenTransferKey,
}

impl Nep17TransferKey {
    /// Creates a new Nep17TransferKey instance.
    /// Matches C# constructor
    pub fn new(user_script_hash: UInt160, timestamp: u64, asset_script_hash: UInt160, xfer_index: u32) -> Self {
        Self {
            base: TokenTransferKey::new(user_script_hash, timestamp, asset_script_hash, xfer_index),
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
        self.base.size()
    }
}

impl Default for Nep17TransferKey {
    fn default() -> Self {
        Self {
            base: TokenTransferKey::default(),
        }
    }
}

impl PartialOrd for Nep17TransferKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nep17TransferKey {
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
        self.block_xfer_notification_index().cmp(&other.block_xfer_notification_index())
    }
}

impl ISerializable for Nep17TransferKey {
    fn size(&self) -> usize {
        self.size()
    }
    
    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        // Serialize base class
        self.base.serialize(writer)?;
        Ok(())
    }
    
    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        // Deserialize base class
        self.base.deserialize(reader)?;
        Ok(())
    }
}