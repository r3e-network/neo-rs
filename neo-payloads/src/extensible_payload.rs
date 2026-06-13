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

use super::{InventoryType, inventory::Inventory, witness::Witness};
use neo_error::CoreResult;
use neo_io::macros::ValidateLength;
use neo_io::serializable::helper::{get_var_size, get_var_size_bytes, get_var_size_str};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};
use neo_storage::DataCache;
use serde::{Deserialize, Serialize};

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

    /// Returns the cached hash of the payload, computing it if necessary.
    pub fn ensure_hash(&mut self) -> UInt256 {
        match self.try_hash() {
            Ok(hash) => hash,
            Err(err) => {
                tracing::error!("ExtensiblePayload unsigned serialization failed: {err}");
                UInt256::zero()
            }
        }
    }

    /// Convenience accessor mirroring the C# hash property.
    pub fn hash(&mut self) -> UInt256 {
        self.ensure_hash()
    }

    /// Gets the hash of the payload, failing closed if unsigned serialization
    /// fails.
    pub fn try_hash(&mut self) -> CoreResult<UInt256> {
        if let Some(hash) = self._hash {
            return Ok(hash);
        }

        let hash_data = self.try_get_hash_data()?;
        let hash = UInt256::from(neo_crypto::Crypto::sha256(&hash_data));
        self._hash = Some(hash);
        Ok(hash)
    }

    /// Returns the unsigned serialization used for hashing.
    pub fn hash_data(&self) -> Vec<u8> {
        match self.try_get_hash_data() {
            Ok(data) => data,
            Err(err) => {
                tracing::error!("Failed to serialize extensible payload unsigned data: {err}");
                Vec::new()
            }
        }
    }

    /// Returns the unsigned serialization used for hashing, or an error if the
    /// payload cannot be represented on the wire.
    pub fn try_get_hash_data(&self) -> CoreResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)?;
        Ok(writer.into_bytes())
    }

    fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        // Use ValidateLength trait to reduce boilerplate
        self.category
            .validate_max_length(MAX_CATEGORY_LENGTH, "Category")?;
        writer.write_var_string(&self.category)?;

        writer.write_u32(self.valid_block_start)?;
        writer.write_u32(self.valid_block_end)?;
        writer.write_serializable(&self.sender)?;

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

impl neo_primitives::SerializablePayload for ExtensiblePayload {
    fn hash_data(&self) -> Vec<u8> {
        ExtensiblePayload::hash_data(self)
    }

    fn witness_count(&self) -> usize {
        1
    }

    fn invocation_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            self.witness.invocation_script.as_slice()
        } else {
            &[]
        }
    }

    fn verification_script(&self, index: usize) -> &[u8] {
        if index == 0 {
            self.witness.verification_script.as_slice()
        } else {
            &[]
        }
    }
}

impl Inventory for ExtensiblePayload {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Extensible
    }
}

impl crate::VerifiableExt for ExtensiblePayload {
    /// C# `ExtensiblePayload.GetScriptHashesForVerifying`: the single hash to
    /// verify is the payload's `Sender`.
    fn script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        vec![self.sender]
    }

    fn witnesses(&self) -> Vec<&crate::Witness> {
        vec![&self.witness]
    }

    fn witnesses_mut(&mut self) -> Vec<&mut crate::Witness> {
        vec![&mut self.witness]
    }
}

impl Serializable for ExtensiblePayload {
    fn size(&self) -> usize {
        get_var_size_str(&self.category)
            + 4
            + 4
            + UInt160::LENGTH
            + get_var_size_bytes(&self.data)
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
neo_io::impl_default_via_new!(ExtensiblePayload);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_hash_matches_legacy_hash_for_valid_payload() {
        let mut payload = ExtensiblePayload::new();
        payload.category = "oracle".to_string();
        payload.valid_block_start = 1;
        payload.valid_block_end = 2;
        payload.data = vec![1, 2, 3];

        let expected = payload.clone().hash();

        assert_eq!(payload.try_hash().expect("try hash"), expected);
    }

    #[test]
    fn try_hash_rejects_oversized_category_without_caching_zero_hash() {
        let mut payload = ExtensiblePayload::new();
        payload.category = "x".repeat(MAX_CATEGORY_LENGTH + 1);

        assert!(payload.try_hash().is_err());
        assert_eq!(payload.hash(), UInt256::zero());
        assert!(payload._hash.is_none());
    }

    #[test]
    fn iverifiable_extensible_hash_uses_try_hash() {
        let mut payload = ExtensiblePayload::new();
        payload.category = "oracle".to_string();
        payload.valid_block_start = 1;
        payload.valid_block_end = 2;

        let expected = payload.try_hash().expect("try hash");

        assert_eq!(
            neo_primitives::Verifiable::hash(&payload).unwrap(),
            expected
        );
    }
}

impl neo_primitives::Verifiable for ExtensiblePayload {
    fn hash(&self) -> neo_primitives::error::PrimitiveResult<neo_primitives::UInt256> {
        let data = self.try_get_hash_data().map_err(|e| {
            neo_primitives::error::PrimitiveError::invalid_data(format!(
                "extensible payload serialization failed: {e}"
            ))
        })?;
        Ok(neo_primitives::UInt256::from(neo_crypto::Crypto::sha256(
            &data,
        )))
    }
    fn hash_data(&self) -> Vec<u8> {
        let mut writer = neo_io::BinaryWriter::new();
        if self.serialize_unsigned(&mut writer).is_err() {
            return Vec::new();
        }
        writer.into_bytes()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn verify(&self) -> bool {
        true
    }
}
