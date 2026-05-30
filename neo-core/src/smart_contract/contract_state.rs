//! Contract state management for Neo smart contracts.
//!
//! This module provides the ContractState struct which represents the state
//! of a deployed smart contract in the Neo blockchain.

use crate::error::{CoreError, CoreResult};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::smart_contract::{
    helper::Helper, interoperable::Interoperable, manifest::ContractManifest, nef_file::NefFile,
};
use crate::neo_vm::StackItem;
use crate::UInt160;
use neo_vm_rs::{OpCode, StackValue};
use num_traits::ToPrimitive;
use serde_json::{json, Value};

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
    match value {
        StackValue::Integer(value) => Ok(num_bigint::BigInt::from(*value)),
        StackValue::Boolean(value) => Ok(num_bigint::BigInt::from(i32::from(*value))),
        StackValue::BigInteger(bytes) => Ok(num_bigint::BigInt::from_signed_bytes_le(bytes)),
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() <= 32 => {
            Ok(num_bigint::BigInt::from_signed_bytes_le(bytes))
        }
        _ => Err(CoreError::invalid_format(
            "ContractState field must be Integer-compatible",
        )),
    }
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
}

impl Interoperable for ContractState {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), crate::neo_vm::VmError> {
        let sv = StackValue::try_from(stack_item).map_err(|error| {
            crate::neo_vm::VmError::invalid_operation_msg(format!(
                "ContractState expects Array/Struct stack item: {error}"
            ))
        })?;
        self.from_stack_value(sv).map_err(|e| crate::neo_vm::VmError::invalid_operation_msg(e.to_string()))
    }

    fn to_stack_item(&self) -> Result<StackItem, crate::neo_vm::VmError> {
        StackItem::try_from(self.to_stack_value()).map_err(|error| {
            crate::neo_vm::VmError::invalid_operation_msg(format!(
                "ContractState StackValue conversion failed: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

impl ContractState {
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::Integer(self.id as i64),
            StackValue::Integer(i64::from(self.update_counter)),
            StackValue::ByteString(self.hash.to_bytes().to_vec()),
            StackValue::ByteString(self.nef.to_bytes()),
            self.manifest.to_stack_value(),
        ])
    }

    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let items = match stack_value {
            StackValue::Array(items) | StackValue::Struct(items) => items,
            other => {
                return Err(CoreError::invalid_format(format!(
                    "ContractState expects Array/Struct stack value, found {:?}",
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
            .unwrap_or_default();
        let update_counter = stack_value_to_bigint(&items[1])?
            .to_u16()
            .unwrap_or_default();
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
mod tests {
    use super::*;
    use crate::smart_contract::manifest::ContractManifest;
    use neo_vm_rs::StackValue;

    #[test]
    fn contract_state_roundtrip_matches_signed_id() {
        let nef = NefFile::new("compiler".to_string(), vec![1, 2, 3]);
        let manifest = ContractManifest::new("test".to_string());
        let state = ContractState::new(-1, UInt160::zero(), nef.clone(), manifest.clone());

        let mut writer = BinaryWriter::new();
        state.serialize(&mut writer).expect("serialize");
        let bytes = writer.into_bytes();

        let mut reader = MemoryReader::new(&bytes);
        let parsed = ContractState::deserialize(&mut reader).expect("deserialize");

        assert_eq!(parsed.id, state.id);
        assert_eq!(parsed.update_counter, state.update_counter);
        assert_eq!(parsed.hash, state.hash);
        assert_eq!(parsed.nef.script, nef.script);
        assert_eq!(parsed.manifest.name, manifest.name);
        assert_eq!(bytes.len(), state.size());
    }

    #[test]
    fn contract_state_projects_to_stack_value() {
        let hash = UInt160::from_bytes(&[1u8; 20]).expect("hash");
        let nef = NefFile::new("compiler".to_string(), vec![1, 2, 3]);
        let manifest = ContractManifest::new("test".to_string());
        let mut state = ContractState::new(-7, hash, nef.clone(), manifest.clone());
        state.update_counter = 9;

        assert_eq!(
            state.to_stack_value(),
            StackValue::Array(vec![
                StackValue::Integer(-7),
                StackValue::Integer(9),
                StackValue::ByteString(hash.to_bytes()),
                StackValue::ByteString(nef.to_bytes()),
                manifest.to_stack_value(),
            ])
        );
    }

    #[test]
    fn contract_state_reads_stack_value() {
        let hash = UInt160::from_bytes(&[2u8; 20]).expect("hash");
        let nef = NefFile::new("compiler".to_string(), vec![4, 5, 6]);
        let manifest = ContractManifest::new("parsed".to_string());

        let mut state = ContractState::default();
        state
            .from_stack_value(StackValue::Array(vec![
                StackValue::Integer(11),
                StackValue::Integer(3),
                StackValue::ByteString(hash.to_bytes()),
                StackValue::ByteString(nef.to_bytes()),
                manifest.to_stack_value(),
            ]))
            .expect("contract state from stack value");

        assert_eq!(state.id, 11);
        assert_eq!(state.update_counter, 3);
        assert_eq!(state.hash, hash);
        assert_eq!(state.nef.script, nef.script);
        assert_eq!(state.manifest.name, manifest.name);
    }

    #[test]
    fn contract_state_stack_item_projection_matches_stack_value_projection() {
        let hash = UInt160::from_bytes(&[3u8; 20]).expect("hash");
        let nef = NefFile::new("compiler".to_string(), vec![7, 8, 9]);
        let manifest = ContractManifest::new("adapter".to_string());
        let state = ContractState::new(4, hash, nef, manifest);
        let expected = StackItem::try_from(state.to_stack_value()).unwrap();

        assert_eq!(state.to_stack_item().unwrap(), expected);
    }
}
