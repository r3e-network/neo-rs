//! Contract state management for Neo smart contracts.
//!
//! This module provides the [`ContractState`] struct, which represents the
//! state of a deployed smart contract in the Neo blockchain (the Rust
//! counterpart of C# `Neo.SmartContract.ContractState`).
//!
//! # Why this lives in `neo-execution`, not `neo-native-contracts`
//!
//! `ContractState` is read and written by the `ContractManagement` native
//! contract (in `neo-native-contracts`), so at first glance it looks like it
//! belongs next to that contract. It cannot move there, however, because it is
//! deeply coupled to the execution engine: it depends on [`crate::helper::Helper`],
//! [`crate::interoperable::Interoperable`], and the VM stack-item types
//! ([`neo_vm::StackItem`] / [`neo_vm_rs::StackValue`]) for its (de)serialization
//! to/from the on-chain `Struct` representation. `neo-native-contracts` depends
//! on `neo-execution` (for the `NativeContract` trait and the engine), so
//! moving `ContractState` the other way would create a dependency cycle.
//!
//! The C# reference confirms this placement: `ContractState` lives in the
//! `Neo.SmartContract` namespace (the execution layer), not in
//! `Neo.SmartContract.Native` (where `ContractManagement` lives). The Rust
//! layering mirrors that split.

use crate::helper::Helper;
use crate::interoperable::Interoperable;
use neo_error::{CoreError, CoreResult};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_manifest::{ContractManifest, NefFile};
use neo_primitives::UInt160;
use neo_vm_rs::{OpCode, StackValue};
use num_traits::ToPrimitive;
use serde_json::{Value, json};

/// Represents the state of a deployed smart contract.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContractState {
    /// The unique identifier of the contract.
    pub id: i32,

    /// The update counter of the contract.
    pub update_counter: u16,

    /// The hash of the contract.
    pub hash: UInt160,

    /// The NEF (Neo Executable Format) file of the contract.
    pub nef: NefFile,

    /// The manifest of the contract.
    pub manifest: ContractManifest,
}

fn stack_value_to_bigint(value: &StackValue) -> Result<num_bigint::BigInt, CoreError> {
    neo_vm::stack_value_as_bigint(value)
        .map_err(|_| CoreError::invalid_format("ContractState field must be Integer-compatible"))
}

fn stack_value_to_bytes(value: &StackValue) -> Option<Vec<u8>> {
    value.to_byte_string_bytes()
}

impl ContractState {
    /// Creates a new contract state.
    pub fn new(id: i32, hash: UInt160, nef: NefFile, manifest: ContractManifest) -> Self {
        Self {
            id,
            update_counter: 0,
            hash,
            nef,
            manifest,
        }
    }

    /// Creates a new native contract state.
    pub fn new_native(id: i32, hash: UInt160, name: String) -> Self {
        let nef = NefFile::new("native".to_string(), vec![OpCode::RET.byte()]);

        let manifest = ContractManifest::new_native(name);

        Self {
            id,
            update_counter: 0,
            hash,
            nef,
            manifest,
        }
    }

    /// Gets the size of the contract state in bytes.
    pub fn size(&self) -> usize {
        4 + // Id (i32)
        2 + // UpdateCounter (u16)
        UInt160::LENGTH + // Hash (UInt160)
        self.nef.size() + // NefFile
        self.manifest.size() // ContractManifest
    }

    /// Calculates the hash of the contract from its NEF and manifest.
    pub fn calculate_hash(sender: &UInt160, nef_checksum: u32, manifest_name: &str) -> UInt160 {
        Helper::get_contract_hash(sender, nef_checksum, manifest_name)
    }

    /// Serializes the contract state to bytes.
    pub fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_i32(self.id)?;
        writer.write_u16(self.update_counter)?;
        Serializable::serialize(&self.hash, writer)?;
        Serializable::serialize(&self.nef, writer)?;

        Serializable::serialize(&self.manifest, writer)
            .map_err(|e| IoError::invalid_data(format!("Manifest serialization failed: {e}")))?;

        Ok(())
    }

    /// Deserializes the contract state from bytes.
    pub fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let id = reader.read_i32()?;
        let update_counter = reader.read_uint16()?;
        let hash = <UInt160 as Serializable>::deserialize(reader)?;
        let nef = <NefFile as Serializable>::deserialize(reader)?;

        let manifest = match ContractManifest::deserialize(reader) {
            Ok(manifest) => manifest,
            Err(e) => {
                return Err(IoError::invalid_data(format!(
                    "Manifest deserialization failed: {e}"
                )));
            }
        };

        Ok(Self {
            id,
            update_counter,
            hash,
            nef,
            manifest,
        })
    }

    /// Converts the contract state to JSON (matches C# ContractState.ToJson).
    pub fn to_json(&self) -> CoreResult<Value> {
        let manifest = self.manifest.to_json()?;
        Ok(json!({
            "id": self.id,
            "updatecounter": self.update_counter,
            "hash": self.hash.to_string(),
            "nef": self.nef.to_json(),
            "manifest": manifest,
        }))
    }

    /// Serializes this contract state into the per-contract storage record
    /// bytes (the `Prefix_Contract(8)` value under `ContractManagement`).
    ///
    /// Matches C# exactly: `StorageItem.Value` for an `IInteroperable` is
    /// `BinarySerializer.Serialize(contract.ToStackItem(null),
    /// ExecutionEngineLimits.Default)`, and `ContractState.ToStackItem` is
    /// `Array [Integer(Id), Integer(UpdateCounter), ByteString(Hash),
    /// ByteString(Nef.ToArray()), Manifest.ToStackItem()]` — NOT the raw
    /// `neo_io` field encoding (which remains available via [`Serializable`]
    /// for non-storage purposes).
    pub fn serialize_contract_record(&self) -> CoreResult<Vec<u8>> {
        neo_serialization::BinarySerializer::serialize_stack_value(
            &self.to_stack_value(),
            &neo_vm_rs::ExecutionEngineLimits::default(),
        )
        .map_err(|e| CoreError::serialization(format!("ContractState record: {e}")))
    }

    /// Decodes a per-contract storage record produced by
    /// [`Self::serialize_contract_record`].
    ///
    /// Matches C# `StorageItem.GetInteroperable<ContractState>()`:
    /// `BinarySerializer.Deserialize(value, ExecutionEngineLimits.Default)`
    /// followed by `ContractState.FromStackItem`.
    pub fn deserialize_contract_record(bytes: &[u8]) -> CoreResult<Self> {
        let limits = neo_vm_rs::ExecutionEngineLimits::default();
        let value = neo_serialization::BinarySerializer::deserialize_stack_value_with_limits(
            bytes,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("ContractState record: {e}")))?;
        let mut state = Self::default();
        state.from_stack_value(value)?;
        Ok(state)
    }
}

impl Interoperable for ContractState {
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), neo_vm::InteroperableError> {
        self.from_stack_value(value)
            .map_err(|e| neo_vm::InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_value(&self) -> Result<StackValue, neo_vm::InteroperableError> {
        Ok(self.to_stack_value())
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

impl ContractState {
    /// Converts the contract state into the persisted VM stack-value shape.
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::Integer(self.id as i64),
            StackValue::Integer(i64::from(self.update_counter)),
            StackValue::ByteString(self.hash.to_bytes().to_vec()),
            StackValue::ByteString(self.nef.to_bytes()),
            self.manifest.to_stack_value(),
        ])
    }

    /// Updates this contract state from the persisted VM stack-value shape.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let items = match stack_value {
            StackValue::Array(items) => items,
            other => {
                return Err(CoreError::invalid_format(format!(
                    "ContractState expects Array stack value, found {:?}",
                    other.compact_type_tag()
                )));
            }
        };

        if items.len() < 5 {
            return Err(CoreError::invalid_format(format!(
                "ContractState stack value must contain 5 elements, found {}",
                items.len()
            )));
        }

        let id = stack_value_to_bigint(&items[0])?
            .to_i32()
            .ok_or_else(|| CoreError::invalid_format("ContractState id must fit Int32"))?;
        let update_counter = stack_value_to_bigint(&items[1])?.to_u16().ok_or_else(|| {
            CoreError::invalid_format("ContractState update counter must fit UInt16")
        })?;
        let hash_bytes = stack_value_to_bytes(&items[2])
            .ok_or_else(|| CoreError::invalid_format("ContractState hash must be ByteString"))?;
        let hash = UInt160::from_bytes(&hash_bytes).map_err(|_| {
            CoreError::invalid_format("ContractState hash must be valid UInt160 bytes")
        })?;
        let nef_bytes = stack_value_to_bytes(&items[3])
            .ok_or_else(|| CoreError::invalid_format("ContractState NEF must be ByteString"))?;
        let nef = NefFile::parse(&nef_bytes)
            .map_err(|_| CoreError::invalid_format("ContractState NEF bytes failed to parse"))?;

        let mut manifest = ContractManifest::new(String::new());
        manifest.from_stack_value(items[4].clone())?;

        self.id = id;
        self.update_counter = update_counter;
        self.hash = hash;
        self.nef = nef;
        self.manifest = manifest;
        Ok(())
    }
}

impl Serializable for ContractState {
    fn size(&self) -> usize {
        self.size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_i32(self.id)?;
        writer.write_u16(self.update_counter)?;
        writer.write_bytes(&self.hash.as_bytes())?;
        Serializable::serialize(&self.nef, writer)?;
        Serializable::serialize(&self.manifest, writer)
            .map_err(|e| IoError::invalid_data(format!("Manifest serialization failed: {e}")))?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let id = reader.read_i32()?;
        let update_counter = reader.read_u16()?;
        let hash = <UInt160 as Serializable>::deserialize(reader)?;
        let nef = <NefFile as Serializable>::deserialize(reader)?;

        let manifest = match ContractManifest::deserialize(reader) {
            Ok(manifest) => manifest,
            Err(e) => {
                return Err(IoError::invalid_data(format!(
                    "Manifest deserialization failed: {e}"
                )));
            }
        };

        Ok(Self {
            id,
            update_counter,
            hash,
            nef,
            manifest,
        })
    }
}

#[cfg(test)]
#[path = "../tests/contracts/contract_state.rs"]
mod tests;
