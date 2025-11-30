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

use super::{i_inventory::IInventory, inventory_type::InventoryType, witness::Witness};
use crate::macros::ValidateLength;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use crate::{CoreResult, UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;

const MAX_CATEGORY_LENGTH: usize = 32;
const MAX_DATA_LENGTH: usize = 0x0100_0000; // 16 MB, matches C# ReadVarMemory upper bound

/// Represents an extensible message that can be relayed.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        snapshot: &DataCache,
        extensible_witness_white_list: &HashSet<UInt160>,
    ) -> bool {
        let height = LedgerContract::new().current_index(snapshot).unwrap_or(0);

        // Check if within valid block range
        if height < self.valid_block_start || height >= self.valid_block_end {
            return false;
        }

        // Check if sender is in whitelist
        if !extensible_witness_white_list.contains(&self.sender) {
            return false;
        }

        // Verify witness with max gas of 0.06 GAS
        crate::IVerifiable::verify_witnesses(self, settings, snapshot, 6_000_000)
    }

    /// Returns the cached hash of the payload, computing it if necessary.
    pub fn ensure_hash(&mut self) -> UInt256 {
        if let Some(hash) = self._hash {
            return hash;
        }

        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)
            .expect("extensible payload serialization should not fail");
        let hash = UInt256::from(crate::neo_crypto::sha256(&writer.into_bytes()));
        self._hash = Some(hash);
        hash
    }

    /// Convenience accessor mirroring the C# hash property.
    pub fn hash(&mut self) -> UInt256 {
        self.ensure_hash()
    }

    fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        // Use ValidateLength trait to reduce boilerplate
        self.category
            .validate_max_length(MAX_CATEGORY_LENGTH, "Category")?;
        writer.write_var_string(&self.category)?;

        writer.write_u32(self.valid_block_start)?;
        writer.write_u32(self.valid_block_end)?;
        let sender = self.sender.as_bytes();
        writer.write_bytes(&sender)?;

        self.data.validate_max_length(MAX_DATA_LENGTH, "Data")?;
        writer.write_var_bytes(&self.data)?;

        Ok(())
    }

    fn deserialize_unsigned(reader: &mut MemoryReader) -> IoResult<Self> {
        let category = reader.read_var_string(MAX_CATEGORY_LENGTH)?;
        let valid_block_start = reader.read_u32()?;
        let valid_block_end = reader.read_u32()?;

        if valid_block_start >= valid_block_end {
            return Err(IoError::invalid_data(
                "Invalid valid block range: start must be less than end",
            ));
        }

        let sender = <UInt160 as Serializable>::deserialize(reader)?;
        let data = reader.read_var_bytes(MAX_DATA_LENGTH)?;

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
        self.ensure_hash()
    }
}

impl crate::IVerifiable for ExtensiblePayload {
    fn get_script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        vec![self.sender]
    }

    fn get_witnesses(&self) -> Vec<&Witness> {
        vec![&self.witness]
    }

    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness> {
        vec![&mut self.witness]
    }

    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> CoreResult<UInt256> {
        let mut clone = self.clone();
        Ok(ExtensiblePayload::hash(&mut clone))
    }

    fn get_hash_data(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)
            .expect("extensible payload unsigned serialization should succeed");
        writer.into_bytes()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Serializable for ExtensiblePayload {
    fn size(&self) -> usize {
        get_var_size(self.category.len() as u64)
            + self.category.len()
            + 4
            + 4
            + UInt160::LENGTH
            + get_var_size(self.data.len() as u64)
            + self.data.len()
            + get_var_size(1)
            + self.witness.size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.serialize_unsigned(writer)?;
        // Write witness count (always 1)
        writer.write_var_uint(1)?;
        writer.write_serializable(&self.witness)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let mut payload = Self::deserialize_unsigned(reader)?;

        // Read witness count (must be 1)
        let count = reader.read_var_uint()?;
        if count != 1 {
            return Err(IoError::invalid_data("Expected 1 witness"));
        }

        payload.witness = <Witness as Serializable>::deserialize(reader)?;
        Ok(payload)
    }
}

// Use macro to reduce boilerplate
crate::impl_default_via_new!(ExtensiblePayload);
