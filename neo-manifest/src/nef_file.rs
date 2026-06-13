//! `NefFile` — NEF (Neo Executable Format) wire container
//! (matches C# `Neo.SmartContract.NefFile`).
//!
//! ## Layering
//!
//! Pure data type in **Layer 1 (protocol)**. Depends only on
//! `MethodToken` from the same crate and `neo-primitives` /
//! `neo-crypto` / `neo-io`. The `Serializable` impl lives here
//! too because the on-wire encoding is a pure data concern.

use crate::method_token::MethodToken;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neo_crypto::Crypto;
use neo_io::extensions::memory_reader::MemoryReaderExtensions;
use neo_io::serializable::helper::{
    get_var_size_bytes, get_var_size_serializable_slice, get_var_size_str,
};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use serde_json::{Value, json};

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

impl NefFile {
    /// The NEF magic number: `'N', 'E', 'F', 3` in little-endian.
    pub const MAGIC: u32 = 0x3346_454E;
    /// The fixed compiler string length (zero-padded to 64 bytes on the wire).
    pub const COMPILER_LENGTH: usize = 64;
    /// The maximum source string length in bytes.
    pub const MAX_SOURCE_LENGTH: usize = 256;
    /// The maximum number of tokens in a NEF file.
    pub const MAX_TOKENS: usize = 128;

    /// Creates a new NEF file with the computed checksum.
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

    /// Gets the size of the NEF file in bytes (matches the
    /// on-wire serialised length).
    pub fn size(&self) -> usize {
        4 + // Magic (u32)
        Self::COMPILER_LENGTH + // Compiler fixed string (64 bytes)
        get_var_size_str(&self.source) + // Source var string
        1 + // Reserved byte
        get_var_size_serializable_slice(&self.tokens) + // Tokens array
        2 + // Reserved bytes (u16)
        get_var_size_bytes(&self.script) + // Script var bytes
        4 // Checksum (u32)
    }

    /// Computes the NEF checksum using the C# algorithm:
    /// `Hash256(nef_bytes_without_checksum)[..4]` interpreted as
    /// little-endian u32.
    pub fn compute_checksum(nef: &Self) -> u32 {
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
        writer.write_u32(Self::MAGIC).expect("writer");

        // Compiler as fixed-length string
        let compiler_bytes = self.compiler.as_bytes();
        let mut fixed = [0u8; Self::COMPILER_LENGTH];
        let len = compiler_bytes.len().min(Self::COMPILER_LENGTH);
        fixed[..len].copy_from_slice(&compiler_bytes[..len]);
        writer.write_bytes(&fixed).expect("writer");

        writer.write_var_string(&self.source).expect("writer");
        writer.write_u8(0).expect("writer");
        writer.write_serializable_vec(&self.tokens).expect("writer");
        writer.write_u16(0).expect("writer");
        writer.write_var_bytes(&self.script).expect("writer");
        writer.write_u32(self.checksum).expect("writer");

        writer.into_bytes()
    }

    /// Parses a NEF file from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = MemoryReader::new(bytes);
        Self::deserialize(&mut reader).map_err(|e| e.to_string())
    }

    /// Parses a NEF file from bytes (alias for [`Self::from_bytes`]).
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        Self::from_bytes(bytes)
    }

    /// Converts the NEF file to base64-encoded bytes.
    pub fn to_base64(&self) -> String {
        BASE64_STANDARD.encode(self.to_bytes())
    }

    /// Parses a NEF file from base64-encoded bytes.
    pub fn from_base64(base64: &str) -> Result<Self, String> {
        let bytes = BASE64_STANDARD.decode(base64).map_err(|e| e.to_string())?;
        Self::from_bytes(&bytes)
    }

    /// Converts to JSON representation.
    pub fn to_json(&self) -> Value {
        json!({
            "magic": Self::MAGIC,
            "compiler": self.compiler,
            "source": self.source,
            "tokens": self.tokens,
            "script": BASE64_STANDARD.encode(&self.script),
            "checksum": self.checksum,
        })
    }
}

impl Serializable for NefFile {
    fn size(&self) -> usize {
        self.size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        if self.compiler.len() > Self::COMPILER_LENGTH {
            return Err(IoError::invalid_data(format!(
                "Compiler name too long: {} > {}",
                self.compiler.len(),
                Self::COMPILER_LENGTH
            )));
        }
        if self.source.len() > Self::MAX_SOURCE_LENGTH {
            return Err(IoError::invalid_data(format!(
                "Source too long: {} > {}",
                self.source.len(),
                Self::MAX_SOURCE_LENGTH
            )));
        }
        if self.tokens.len() > Self::MAX_TOKENS {
            return Err(IoError::invalid_data(format!(
                "Too many tokens: {} > {}",
                self.tokens.len(),
                Self::MAX_TOKENS
            )));
        }
        // Check script length is within the var-int encoding range
        // (matches the on-wire NEF3 format).
        if self.script.len() > u32::MAX as usize {
            return Err(IoError::invalid_data("Script too long for NEF format"));
        }
        // Also enforce the Neo VM item-size cap (matches
        // neo-vm-rs::ExecutionEngineLimits::max_item_size).
        let max_item_size = neo_vm_rs::ExecutionEngineLimits::DEFAULT.max_item_size as usize;
        if self.script.len() > max_item_size {
            return Err(IoError::invalid_data(format!(
                "Script exceeds max item size: {} > {}",
                self.script.len(),
                max_item_size
            )));
        }
        writer.write_u32(Self::MAGIC)?;
        let compiler_bytes = self.compiler.as_bytes();
        let mut fixed = [0u8; Self::COMPILER_LENGTH];
        let len = compiler_bytes.len().min(Self::COMPILER_LENGTH);
        fixed[..len].copy_from_slice(&compiler_bytes[..len]);
        writer.write_bytes(&fixed)?;

        writer.write_var_string(&self.source)?;
        writer.write_u8(0)?; // reserved
        writer.write_serializable_vec(&self.tokens)?;
        writer.write_u16(0)?; // reserved
        writer.write_var_bytes(&self.script)?;
        writer.write_u32(self.checksum)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let magic = reader.read_u32()?;
        if magic != Self::MAGIC {
            return Err(IoError::invalid_data(format!(
                "Bad magic: {magic:#x}, expected {:#x}",
                Self::MAGIC
            )));
        }

        let compiler_bytes = reader.read_bytes(Self::COMPILER_LENGTH)?;
        let compiler_end = compiler_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(compiler_bytes.len());
        let compiler = String::from_utf8(compiler_bytes[..compiler_end].to_vec())
            .map_err(|e| IoError::invalid_data(format!("Invalid compiler UTF-8: {e}")))?;

        let source = reader.read_var_string(Self::MAX_SOURCE_LENGTH)?;

        let _reserved = reader.read_u8()?;
        let tokens: Vec<MethodToken> = reader
            .read_serializable_array(Self::MAX_TOKENS)
            .map_err(|e| IoError::invalid_data(e.to_string()))?;
        let _reserved2 = reader.read_u16()?;
        let script = reader.read_var_bytes(u32::MAX as usize)?;
        let checksum = reader.read_u32()?;

        let nef = NefFile {
            compiler,
            source,
            tokens,
            script,
            checksum,
        };

        // Validate the on-wire checksum (matches C# NEF verifier).
        let expected = Self::compute_checksum(&nef);
        if expected != nef.checksum {
            return Err(IoError::invalid_data(format!(
                "Bad checksum: {:#x}, expected {:#x}",
                nef.checksum, expected
            )));
        }

        Ok(nef)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_constant_matches_neo_spec() {
        // 0x3346454E = 'N','E','F',3 in little-endian
        assert_eq!(NefFile::MAGIC, 0x3346_454E);
    }

    #[test]
    fn new_computes_checksum() {
        let nef = NefFile::new("neo-core-v0.0.0".to_string(), vec![0x40]); // RET
        assert_ne!(nef.checksum, 0);
    }

    #[test]
    fn default_has_zero_checksum() {
        let nef = NefFile::default();
        assert_eq!(nef.checksum, 0);
    }

    #[test]
    fn new_constructor_stores_fields() {
        let nef = NefFile::new("compiler".to_string(), vec![1, 2, 3, 4]);
        assert_eq!(nef.compiler, "compiler");
        assert_eq!(nef.script, vec![1, 2, 3, 4]);
        assert!(nef.tokens.is_empty());
        assert!(nef.source.is_empty());
    }

    #[test]
    fn size_includes_all_fields() {
        let nef = NefFile::new("c".to_string(), vec![0; 100]);
        // 4 (magic) + 64 (compiler fixed) + 1 (source var int) + 1 (reserved)
        // + 1 (tokens var int) + 2 (reserved u16) + 2 (script var int) + 4 (checksum)
        // + the actual bytes
        let size = nef.size();
        assert!(size > 4 + 64 + 1 + 1 + 1 + 2 + 2 + 4);
    }

    #[test]
    fn from_bytes_rejects_bad_magic() {
        let bytes = vec![0xFF; 100];
        let result = NefFile::from_bytes(&bytes);
        assert!(result.is_err());
    }
}
