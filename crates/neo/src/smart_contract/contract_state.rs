//! Contract state management for Neo smart contracts.
//!
//! This module provides the ContractState struct which represents the state
//! of a deployed smart contract in the Neo blockchain.

use crate::cryptography::Crypto;
use crate::neo_config::{ADDRESS_SIZE, MAX_SCRIPT_SIZE};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::smart_contract::{helper::Helper, manifest::ContractManifest, CallFlags};
use crate::UInt160;
use std::convert::TryInto;

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

/// Represents a method token in a NEF file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodToken {
    /// The hash of the contract.
    pub hash: UInt160,

    /// The method name.
    pub method: String,

    /// The number of parameters.
    pub parameters_count: u16,

    /// Whether the method has a return value.
    pub has_return_value: bool,

    /// The call flags required for this method.
    pub call_flags: CallFlags,
}

impl Default for MethodToken {
    fn default() -> Self {
        Self {
            hash: UInt160::zero(),
            method: String::new(),
            parameters_count: 0,
            has_return_value: false,
            call_flags: CallFlags::NONE,
        }
    }
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
        let nef = NefFile::new(
            "native".to_string(),
            vec![0x40], // RET opcode - native contracts don't have actual script
        );

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
        let mut writer = BinaryWriter::new();
        self.serialize(&mut writer)
            .expect("ContractState serialization should succeed for size calculation");
        writer.len()
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
}

impl NefFile {
    /// Creates a new NEF file.
    pub fn new(compiler: String, script: Vec<u8>) -> Self {
        let checksum = Self::calculate_checksum(&script);

        Self {
            compiler,
            source: String::new(),
            tokens: Vec::new(),
            script,
            checksum,
        }
    }

    /// Gets the size of the NEF file in bytes.
    pub fn size(&self) -> usize {
        let mut writer = BinaryWriter::new();
        self.serialize(&mut writer)
            .expect("NefFile serialization should succeed for size calculation");
        writer.len()
    }

    /// Calculates the checksum of the script.
    fn calculate_checksum(script: &[u8]) -> u32 {
        let hash = Crypto::sha256(script);
        u32::from_le_bytes(
            hash[..4]
                .try_into()
                .expect("sha256 output shorter than 4 bytes"),
        )
    }

    /// Converts the NEF file to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        Serializable::serialize(self, &mut writer).expect("Operation failed");
        writer.to_bytes()
    }

    /// Parses a NEF file from bytes.
    /// This matches C# NefFile.Parse exactly.
    pub fn parse(data: &[u8]) -> IoResult<Self> {
        let mut reader = MemoryReader::new(data);
        Self::deserialize(&mut reader)
    }
}

impl MethodToken {
    /// Creates a new method token.
    pub fn new(
        hash: UInt160,
        method: String,
        parameters_count: u16,
        has_return_value: bool,
        call_flags: CallFlags,
    ) -> Self {
        Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        }
    }

    /// Gets the size of the method token in bytes.
    pub fn size(&self) -> usize {
        ADDRESS_SIZE +
        self.method.len() + 1 + // method with length prefix
        2 + // parameters_count
        1 + // has_return_value
        4 // call_flags
    }
}

impl Serializable for ContractState {
    fn size(&self) -> usize {
        let mut writer = BinaryWriter::new();
        // Serialization cannot fail for in-memory values; panic on unexpected errors.
        self.serialize(&mut writer)
            .expect("ContractState serialization should succeed for size calculation");
        writer.len()
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
        let mut writer = BinaryWriter::new();
        self.serialize(&mut writer)
            .expect("NefFile serialization should succeed for size calculation");
        writer.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(0x3346454E)?; // NEF magic
        writer.write_var_string(&self.compiler)?;
        writer.write_var_string(&self.source)?;
        writer.write_var_int(self.tokens.len() as u64)?;
        for token in &self.tokens {
            Serializable::serialize(token, writer)?;
        }
        writer.write_var_bytes(&self.script)?;
        writer.write_u32(self.checksum)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let magic = reader.read_u32()?;
        if magic != 0x3346454E {
            return Err(IoError::invalid_data(format!(
                "NEF deserialization magic mismatch: 0x{:08X}",
                magic
            )));
        }

        let compiler = reader.read_var_string(MAX_SCRIPT_SIZE)?; // Max MAX_SCRIPT_SIZE chars for compiler
        let source = reader.read_var_string(MAX_SCRIPT_SIZE)?; // Max MAX_SCRIPT_SIZE chars for source
        let token_count = reader.read_var_int(MAX_SCRIPT_SIZE as u64)? as usize; // Max MAX_SCRIPT_SIZE tokens
        let mut tokens = Vec::with_capacity(token_count);
        for _ in 0..token_count {
            tokens.push(<MethodToken as Serializable>::deserialize(reader)?);
        }
        let script = reader.read_var_bytes(MAX_SCRIPT_SIZE * MAX_SCRIPT_SIZE)?; // Max 1MB script
        let checksum = reader.read_u32()?;

        Ok(NefFile {
            compiler,
            source,
            tokens,
            script,
            checksum,
        })
    }
}

impl Serializable for MethodToken {
    fn size(&self) -> usize {
        let mut writer = BinaryWriter::new();
        self.serialize(&mut writer)
            .expect("MethodToken serialization should succeed for size calculation");
        writer.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_bytes(&self.hash.as_bytes())?;
        writer.write_var_string(&self.method)?;
        writer.write_u16(self.parameters_count)?;
        writer.write_bool(self.has_return_value)?;
        writer.write_u32(u32::from(self.call_flags.bits()))?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let hash_bytes = reader.read_bytes(ADDRESS_SIZE)?;
        let hash =
            UInt160::from_bytes(&hash_bytes).map_err(|e| IoError::invalid_data(e.to_string()))?;
        let method = reader.read_var_string(256)?; // Max 256 chars for method name
        let parameters_count = reader.read_uint16()?;
        let has_return_value = reader.read_boolean()?;
        let call_flags_bits = reader.read_u32()?;
        let call_flags = CallFlags::from_bits_truncate(call_flags_bits as u8);

        Ok(MethodToken {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smart_contract::manifest::ContractManifest;

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
}
