// Copyright (C) 2015-2025 The Neo Project.
//
// extensible_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    i_inventory::IInventory, i_verifiable::IVerifiable, inventory_type::InventoryType,
    witness::Witness,
};
use crate::neo_io::{MemoryReader, Serializable};
use crate::{neo_system::ProtocolSettings, persistence::DataCache, UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, Write};

/// Represents an extensible message that can be relayed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensiblePayload {
    /// The category of the extension.
    pub category: String,

    /// Indicates that the payload is only valid when the block height is greater than or equal to this value.
    pub valid_block_start: u32,

    /// Indicates that the payload is only valid when the block height is less than this value.
    pub valid_block_end: u32,

    /// The sender of the payload.
    pub sender: UInt160,

    /// The data of the payload.
    pub data: Vec<u8>,

    /// The witness of the payload. It must match the sender.
    pub witness: Witness,

    #[serde(skip)]
    _hash: Option<UInt256>,
}

impl ExtensiblePayload {
    /// Creates a new extensible payload.
    pub fn new() -> Self {
        Self {
            category: String::new(),
            valid_block_start: 0,
            valid_block_end: 0,
            sender: UInt160::default(),
            data: Vec::new(),
            witness: Witness::new(),
            _hash: None,
        }
    }

    /// Verify the payload against protocol settings and snapshot.
    pub fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &dyn DataCache,
        extensible_witness_white_list: &HashSet<UInt160>,
    ) -> bool {
        // Get current block height from ledger
        let height = snapshot.get_current_block_height();

        // Check if within valid block range
        if height < self.valid_block_start || height >= self.valid_block_end {
            return false;
        }

        // Check if sender is in whitelist
        if !extensible_witness_white_list.contains(&self.sender) {
            return false;
        }

        // Verify witness with max gas of 0.06 GAS
        self.verify_witnesses(settings, snapshot, 6_000_000)
    }

    fn serialize_unsigned(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Write category as var string (max 32 bytes)
        let category_bytes = self.category.as_bytes();
        if category_bytes.len() > 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Category too long",
            ));
        }
        writer.write_all(&[category_bytes.len() as u8])?;
        writer.write_all(category_bytes)?;

        writer.write_all(&self.valid_block_start.to_le_bytes())?;
        writer.write_all(&self.valid_block_end.to_le_bytes())?;
        writer.write_all(self.sender.as_bytes())?;

        // Write data as var bytes
        if self.data.len() > 0xFFFFFF {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Data too long"));
        }
        writer.write_all(&[self.data.len() as u8])?;
        writer.write_all(&self.data)?;

        Ok(())
    }

    fn deserialize_unsigned(reader: &mut MemoryReader) -> Result<Self, String> {
        let category = reader.read_var_string(32).map_err(|e| e.to_string())?;
        let valid_block_start = reader.read_u32().map_err(|e| e.to_string())?;
        let valid_block_end = reader.read_u32().map_err(|e| e.to_string())?;

        if valid_block_start >= valid_block_end {
            return Err(format!(
                "Invalid valid block range: {} >= {}",
                valid_block_start, valid_block_end
            ));
        }

        let sender = UInt160::deserialize(reader)?;
        let data = reader.read_var_bytes().map_err(|e| e.to_string())?;

        Ok(Self {
            category,
            valid_block_start,
            valid_block_end,
            sender,
            data,
            witness: Witness::new(),
            _hash: None,
        })
    }
}

impl IInventory for ExtensiblePayload {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Extensible
    }

    fn hash(&mut self) -> UInt256 {
        if let Some(hash) = self._hash {
            return hash;
        }

        // Calculate hash from serialized unsigned data
        let mut data = Vec::new();
        self.serialize_unsigned(&mut data).unwrap();
        let hash = UInt256::from(neo_crypto::sha256(&data));
        self._hash = Some(hash);
        hash
    }
}

impl IVerifiable for ExtensiblePayload {
    fn get_script_hashes_for_verifying(&self, _snapshot: &dyn DataCache) -> Vec<UInt160> {
        vec![self.sender]
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        vec![&self.witness]
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        vec![&mut self.witness]
    }
}

impl Serializable for ExtensiblePayload {
    fn size(&self) -> usize {
        1 + self.category.len() + // Category with var length prefix
        4 + // ValidBlockStart
        4 + // ValidBlockEnd
        20 + // Sender (UInt160)
        1 + self.data.len() + // Data with var length prefix
        1 + self.witness.size() // Witness with count prefix (always 1)
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        self.serialize_unsigned(writer)?;
        // Write witness count (always 1)
        writer.write_all(&[1u8])?;
        self.witness.serialize(writer)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let mut payload = Self::deserialize_unsigned(reader)?;

        // Read witness count (must be 1)
        let count = reader.read_u8().map_err(|e| e.to_string())?;
        if count != 1 {
            return Err(format!("Expected 1 witness, got {}", count));
        }

        payload.witness = Witness::deserialize(reader)?;
        Ok(payload)
    }
}

impl Default for ExtensiblePayload {
    fn default() -> Self {
        Self::new()
    }
}
