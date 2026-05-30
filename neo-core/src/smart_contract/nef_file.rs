//! NEF (Neo Executable Format) file representation.
//!
//! A NEF file packages a contract's compiled script together with its
//! compiler metadata, method tokens and a checksum. The on-wire format and
//! checksum algorithm match C# `NefFile`.

use crate::neo_io::serializable::helper::{
    get_var_size_bytes, get_var_size_serializable_slice, get_var_size_str,
};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::smart_contract::MethodToken;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use neo_crypto::Crypto;
use serde_json::{json, Value};

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
