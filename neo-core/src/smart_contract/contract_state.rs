//! Contract state management for Neo smart contracts.
//!
//! This module provides the ContractState struct which represents the state
//! of a deployed smart contract in the Neo blockchain.

use neo_crypto::Crypto;
use crate::error::{CoreError, CoreResult};
use crate::neo_io::serializable::helper::{
    get_var_size_bytes, get_var_size_serializable_slice, get_var_size_str,
};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::smart_contract::{
    helper::Helper, interoperable::Interoperable, manifest::ContractManifest,
    MethodToken,
};
use crate::neo_vm::StackItem;
use crate::UInt160;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
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

/// Represents a NEF (Neo Executable Format) file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NefFile {
    /// The compiler used to compile the contract.
    pub compiler: String,

    /// The source code information.
    pub source: String,

    /// The tokens used in the contract.
    pub tokens: Vec<MethodToken>,

    /// The script of the contract.
    pub script: Vec<u8>,

    /// The checksum of the NEF file.
    pub checksum: u32,
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

impl NefFile {
    pub const MAGIC: u32 = 0x3346_454E;
    const COMPILER_LENGTH: usize = 64;
    const MAX_SOURCE_LENGTH: usize = 256;
    const MAX_TOKENS: usize = 128;

    /// Creates a new NEF file.
    pub fn new(compiler: String, script: Vec<u8>) -> Self {
        let mut nef = Self {
            compiler,
            source: String::new(),
            tokens: Vec::new(),
            script,
            checksum: 0,
        };
        nef.checksum = Self::compute_checksum(&nef);
        nef
    }

    /// Gets the size of the NEF file in bytes.
    pub fn size(&self) -> usize {
        4 + // Magic (u32)
        Self::COMPILER_LENGTH + // Compiler fixed string (64 bytes)
        get_var_size_str(&self.source) + // Source var string
        1 + // Reserved byte
        get_var_size_serializable_slice(&self.tokens) + // Tokens array (var length + items)
        2 + // Reserved bytes (u16)
        get_var_size_bytes(&self.script) + // Script var bytes
        4 // Checksum (u32)
    }

    /// Computes the NEF checksum using the C# algorithm:
    /// `Hash256(nef_bytes_without_checksum)[..4]` interpreted as little-endian u32.
    fn compute_checksum(nef: &Self) -> u32 {
        let mut writer = BinaryWriter::new();

        // Serialize all fields except checksum in NEF3 format.
        writer.write_u32(Self::MAGIC).expect("writer");

        let compiler_bytes = nef.compiler.as_bytes();
        let mut fixed = [0u8; Self::COMPILER_LENGTH];
        let len = compiler_bytes.len().min(Self::COMPILER_LENGTH);
        fixed[..len].copy_from_slice(&compiler_bytes[..len]);
        writer.write_bytes(&fixed).expect("writer");

        writer.write_var_string(&nef.source).expect("writer");

        writer.write_u8(0).expect("writer"); // reserved
        writer.write_serializable_vec(&nef.tokens).expect("writer");
        writer.write_u16(0).expect("writer"); // reserved
        writer.write_var_bytes(&nef.script).expect("writer");

        let bytes = writer.into_bytes();
        let hash = Crypto::hash256(&bytes);
        u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]])
    }

    /// Recomputes and updates the checksum in-place.
    pub fn update_checksum(&mut self) {
        self.checksum = Self::compute_checksum(self);
    }

    /// Converts the NEF file to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if let Err(err) = Serializable::serialize(self, &mut writer) {
            tracing::error!("NEF serialization failed: {err}");
            return Vec::new();
        }
        writer.into_bytes()
    }

    /// Parses a NEF file from bytes.
    /// This matches C# NefFile.Parse exactly.
    pub fn parse(data: &[u8]) -> IoResult<Self> {
        let mut reader = MemoryReader::new(data);
        Self::deserialize(&mut reader)
    }

    /// Converts the NEF file to JSON (matches C# NefFile.ToJson).
    pub fn to_json(&self) -> Value {
        json!({
            "magic": Self::MAGIC,
            "compiler": self.compiler,
            "source": self.source,
            "tokens": self.tokens.iter().map(|t| t.to_json()).collect::<Vec<_>>(),
            "script": BASE64_STANDARD.encode(&self.script),
            "checksum": self.checksum,
        })
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

impl Serializable for NefFile {
    fn size(&self) -> usize {
        self.size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        use neo_vm_rs::ExecutionEngineLimits;

        writer.write_u32(Self::MAGIC)?;

        // Compiler fixed string (64 bytes)
        let compiler_bytes = self.compiler.as_bytes();
        if compiler_bytes.len() > Self::COMPILER_LENGTH {
            return Err(IoError::invalid_data(format!(
                "Compiler length {} exceeds {} bytes",
                compiler_bytes.len(),
                Self::COMPILER_LENGTH
            )));
        }
        writer.write_bytes(compiler_bytes)?;
        if compiler_bytes.len() < Self::COMPILER_LENGTH {
            let padding = vec![0u8; Self::COMPILER_LENGTH - compiler_bytes.len()];
            writer.write_bytes(&padding)?;
        }

        // Source var string (max 256 bytes)
        if self.source.len() > Self::MAX_SOURCE_LENGTH {
            return Err(IoError::invalid_data(format!(
                "Source length exceeds {} bytes",
                Self::MAX_SOURCE_LENGTH
            )));
        }
        writer.write_var_string(&self.source)?;

        writer.write_u8(0)?; // reserved

        if self.tokens.len() > Self::MAX_TOKENS {
            return Err(IoError::invalid_data(format!(
                "Token count {} exceeds maximum {}",
                self.tokens.len(),
                Self::MAX_TOKENS
            )));
        }
        writer.write_serializable_vec(&self.tokens)?;

        writer.write_u16(0)?; // reserved

        if self.script.is_empty() {
            return Err(IoError::invalid_data("Script cannot be empty"));
        }
        let max_item_size = ExecutionEngineLimits::default().max_item_size as usize;
        if self.script.len() > max_item_size {
            return Err(IoError::invalid_data(format!(
                "Script size {} exceeds MaxItemSize {}",
                self.script.len(),
                max_item_size
            )));
        }
        writer.write_var_bytes(&self.script)?;

        writer.write_u32(self.checksum)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        use neo_vm_rs::ExecutionEngineLimits;

        let start_position = reader.position();

        let magic = reader.read_u32()?;
        if magic != Self::MAGIC {
            return Err(IoError::invalid_data(format!(
                "NEF deserialization magic mismatch: 0x{:08X}",
                magic
            )));
        }

        let compiler = reader.read_fixed_string(Self::COMPILER_LENGTH)?;
        let source = reader.read_var_string(Self::MAX_SOURCE_LENGTH)?;

        let reserved = reader.read_byte()?;
        if reserved != 0 {
            return Err(IoError::invalid_data("Reserved byte must be 0"));
        }

        let token_count = reader.read_var_int(Self::MAX_TOKENS as u64)? as usize;
        let mut tokens = Vec::with_capacity(token_count);
        for _ in 0..token_count {
            tokens.push(<MethodToken as Serializable>::deserialize(reader)?);
        }

        let reserved2 = reader.read_uint16()?;
        if reserved2 != 0 {
            return Err(IoError::invalid_data(
                "Reserved bytes must be 0".to_string(),
            ));
        }

        let max_item_size = ExecutionEngineLimits::default().max_item_size as usize;
        let script = reader.read_var_bytes(max_item_size)?;
        if script.is_empty() {
            return Err(IoError::invalid_data("Script cannot be empty"));
        }

        let checksum = reader.read_u32()?;

        let nef = NefFile {
            compiler,
            source,
            tokens,
            script,
            checksum,
        };

        let calculated = Self::compute_checksum(&nef);
        if calculated != checksum {
            return Err(IoError::invalid_data("CRC verification fail"));
        }

        let size = reader.position().saturating_sub(start_position);
        if size > max_item_size {
            return Err(IoError::invalid_data("Max vm item size exceed"));
        }

        Ok(nef)
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
