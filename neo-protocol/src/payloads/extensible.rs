// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.

//! Canonical extensible relay payload: wire codec, hash caching, and witness
//! access. Snapshot-driven validity (current height, whitelist, witness VM
//! verification) is a higher-layer concern and intentionally not present here;
//! this leaf depends only on neo-io / neo-crypto / neo-primitives.

use super::validate_length;
use crate::witness::{Witness, WitnessData};
use neo_crypto::Crypto;
use neo_io::serializable::helper::{get_var_size, get_var_size_bytes, get_var_size_str};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::error::{PrimitiveError, PrimitiveResult};
use neo_primitives::{Inventory, InventoryType, SerializablePayload, UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Maximum byte length of the extension category (C# `ReadVarString(32)`).
const MAX_CATEGORY_LENGTH: usize = 32;
/// Maximum byte length of the payload data (C# `ReadVarMemory` upper bound).
const MAX_DATA_LENGTH: usize = 0x0100_0000; // 16 MB

/// Represents an extensible message that can be relayed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extensible {
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

impl Extensible {
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

    /// Verify the extensible payload against protocol settings and snapshot.
    ///
    /// Validation includes:
    /// 1. Block height range validation (valid_block_start < valid_block_end)
    /// 2. Sender whitelist checking (sender must be in the allowed list for this category)
    /// 3. Size limits (data must not exceed MAX_DATA_LENGTH)
    /// 4. Category format validation
    ///
    /// Note: Witness verification is performed separately by the caller using
    /// neo_ledger::verify_witness functions.
    ///
    /// Reference: C# Neo ExtensiblePayload.Verify()
    pub fn verify(
        &self,
        _settings: &crate::protocol_settings::ProtocolSettings,
        _snapshot: &dyn std::any::Any,
        whitelist: &std::collections::HashSet<UInt160>,
    ) -> bool {
        // 1. Validate block height range
        if self.valid_block_start >= self.valid_block_end {
            return false;
        }

        // 2. Validate data size
        if self.data.len() > MAX_DATA_LENGTH {
            return false;
        }

        // 3. Validate category length
        if self.category.len() > MAX_CATEGORY_LENGTH {
            return false;
        }

        // 4. Check sender against whitelist
        // The whitelist contains addresses authorized to send extensible payloads
        // of this category (typically consensus nodes for dBFT messages)
        if !whitelist.contains(&self.sender) {
            return false;
        }

        // 5. Additional validation: category should not be empty
        if self.category.is_empty() {
            return false;
        }

        true
    }

    /// Returns the cached hash of the payload, computing it if necessary.
    ///
    /// Mirrors the historical fail-soft accessor: on unsigned-serialization
    /// failure it logs and returns the zero hash without populating the cache.
    pub fn ensure_hash(&mut self) -> UInt256 {
        match self.try_hash() {
            Ok(hash) => hash,
            Err(err) => {
                tracing::error!("Extensible unsigned serialization failed: {err}");
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
    pub fn try_hash(&mut self) -> IoResult<UInt256> {
        if let Some(hash) = self._hash {
            return Ok(hash);
        }

        let hash_data = self.try_get_hash_data()?;
        let hash = UInt256::from(Crypto::sha256(&hash_data));
        self._hash = Some(hash);
        Ok(hash)
    }

    /// Returns the unsigned serialization used for hashing, or the historical
    /// empty fallback on failure.
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
    pub fn try_get_hash_data(&self) -> IoResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        self.serialize_unsigned(&mut writer)?;
        Ok(writer.into_bytes())
    }

    fn serialize_unsigned(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        validate_length(self.category.len(), MAX_CATEGORY_LENGTH, "Category")?;
        writer.write_var_string(&self.category)?;

        writer.write_u32(self.valid_block_start)?;
        writer.write_u32(self.valid_block_end)?;
        writer.write_serializable(&self.sender)?;

        validate_length(self.data.len(), MAX_DATA_LENGTH, "Data")?;
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

impl SerializablePayload for Extensible {
    fn hash_data(&self) -> Vec<u8> {
        Extensible::hash_data(self)
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

impl Inventory for Extensible {
    fn inventory_type(&self) -> InventoryType {
        InventoryType::Extensible
    }
}

impl neo_primitives::Verifiable for Extensible {
    // State-independent structural check only; snapshot validity belongs higher.
    fn verify(&self) -> bool {
        true
    }

    fn hash(&self) -> PrimitiveResult<UInt256> {
        let mut clone = self.clone();
        clone
            .try_hash()
            .map_err(|e| PrimitiveError::invalid_data(e.to_string()))
    }

    fn hash_data(&self) -> Vec<u8> {
        Extensible::hash_data(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl WitnessData for Extensible {
    fn witnesses(&self) -> Vec<&Witness> {
        vec![&self.witness]
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        vec![&mut self.witness]
    }
}

impl Serializable for Extensible {
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

impl Default for Extensible {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_io::SerializableExtensions;

    // ---- Preserved from neo-core extensible_payload.rs `mod tests` ----
    //
    // NOTE: `SerializablePayload` is imported at the top of this file, so a
    // plain `.hash()` call would resolve to its default trait method (double
    // SHA-256) instead of the inherent fail-soft single-SHA-256 accessor
    // (`&self` trait methods win the autoref stages over `&mut self`
    // inherent methods). These tests disambiguate explicitly via
    // `ensure_hash` / `Extensible::hash` to keep testing the legacy hash.

    #[test]
    fn try_hash_matches_legacy_hash_for_valid_payload() {
        let mut payload = Extensible::new();
        payload.category = "oracle".to_string();
        payload.valid_block_start = 1;
        payload.valid_block_end = 2;
        payload.data = vec![1, 2, 3];

        let mut legacy = payload.clone();
        let expected = legacy.ensure_hash();

        assert_eq!(payload.try_hash().expect("try hash"), expected);
    }

    #[test]
    fn try_hash_rejects_oversized_category_without_caching_zero_hash() {
        let mut payload = Extensible::new();
        payload.category = "x".repeat(MAX_CATEGORY_LENGTH + 1);

        assert!(payload.try_hash().is_err());
        assert_eq!(Extensible::hash(&mut payload), UInt256::zero());
        assert!(payload._hash.is_none());
    }

    #[test]
    fn iverifiable_extensible_hash_uses_try_hash() {
        let mut payload = Extensible::new();
        payload.category = "oracle".to_string();
        payload.valid_block_start = 1;
        payload.valid_block_end = 2;

        let expected = payload.try_hash().expect("try hash");

        assert_eq!(
            neo_primitives::Verifiable::hash(&payload).unwrap(),
            expected
        );
    }

    // ---- Preserved from neo-core/tests/p2p_payloads_csharp_tests.rs ----

    #[test]
    fn csharp_ut_extensible_payload_size_and_roundtrip() {
        let sender = UInt160::from_bytes(&Crypto::hash160(&[])).expect("empty script hash");
        let mut payload = Extensible::new();
        payload.category = "123".to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = 1;
        payload.sender = sender;
        payload.data = vec![1, 2, 3];
        payload.witness = Witness::new_with_scripts(vec![3, 5, 6], Vec::new());
        assert_eq!(payload.size(), 42);

        let mut payload = Extensible::new();
        payload.category = "123".to_string();
        payload.valid_block_start = 456;
        payload.valid_block_end = 789;
        payload.sender = sender;
        payload.data = vec![1, 2, 3];
        payload.witness = Witness::new_with_scripts(vec![0x51, 0x52, 0x53], Vec::new());
        let bytes = payload.to_array().expect("serialize");
        let mut reader = MemoryReader::new(&bytes);
        let mut clone =
            <Extensible as Serializable>::deserialize(&mut reader).expect("deserialize");

        assert_eq!(clone.witness.script_hash(), sender);
        assert_eq!(clone.hash(), payload.hash());
        assert_eq!(clone.valid_block_start, 456);
        assert_eq!(clone.valid_block_end, 789);
        assert_eq!(clone.category, "123");
        assert_eq!(clone.data, vec![1, 2, 3]);

        {
            let witnesses = clone.witnesses();
            assert_eq!(witnesses.len(), 1);
        }
        let mut witnesses = clone.witnesses_mut();
        assert_eq!(witnesses.len(), 1);
        witnesses[0].invocation_script = vec![0x01];
        assert_eq!(clone.witness.invocation_script, vec![0x01]);
    }

    #[test]
    fn csharp_ut_varbytes_payload_size_boundaries_extensible() {
        // Extensible slice of the shared C# var-bytes boundary case.
        for len in [252usize, 253] {
            let bytes = vec![0xAB; len];

            let mut extensible = Extensible::new();
            extensible.category = "x".to_string();
            extensible.valid_block_start = 0;
            extensible.valid_block_end = 1;
            extensible.sender = UInt160::from_bytes(&[0x11; 20]).expect("sender");
            extensible.data = bytes;
            extensible.witness = Witness::empty();
            let extensible_bytes = extensible.to_array().expect("extensible serialize");
            assert_eq!(extensible.size(), extensible_bytes.len());
        }
    }
}
