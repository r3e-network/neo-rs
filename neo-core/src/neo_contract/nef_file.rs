use std::convert::TryInto;
use std::io::{Cursor, Read, Write};
use std::mem::size_of;
use byteorder::LittleEndian;
use NeoRust::prelude::{StringExt, VarSizeTrait};
use neo_base::encoding::base64;
use neo_json::jtoken::JToken;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use crate::io::iserializable::ISerializable;
use crate::neo_contract::method_token::MethodToken;

/// Represents the structure of NEO Executable Format.
#[derive(Default)]
pub struct NefFile {
    /// The name and version of the compiler that generated this nef file.
    pub compiler: String,

    /// The url of the source files.
    pub source: String,

    /// The methods that to be called statically.
    pub tokens: Vec<MethodToken>,

    /// The script of the contract.
    pub script: Vec<u8>,

    /// The checksum of the nef file.
    pub checksum: u32,
}

impl NefFile {
    /// NEO Executable Format 3 (NEF3)
    const MAGIC: u32 = 0x3346454E;

    const HEADER_SIZE: usize = size_of::<u32>() + 64;

    pub fn size(&self) -> usize {
        Self::HEADER_SIZE
            + self.source.var_size()
            + 1
            + self.tokens.var_size()
            + 2
            + self.script.var_size()
            + size_of::<u32>()
    }

    /// Parse NefFile from bytes
    pub fn parse(data: &[u8], verify: bool) -> Result<Self, std::io::Error> {
        let mut reader = Cursor::new(data);
        let mut nef = NefFile::default();
        nef.deserialize(&mut reader, verify)?;
        Ok(nef)
    }

    /// Computes the checksum for the specified nef file.
    pub fn compute_checksum(&self) -> u32 {
        let data = self.to_bytes();
        let hash = Crypto::hash256(&data[..data.len() - 4]);
        u32::from_le_bytes(hash[..4].try_into().unwrap())
    }

    /// Converts the nef file to a JSON object.
    pub fn to_json(&self) -> JToken {
        JToken::new_object()
            .insert("magic".to_string(), Self::MAGIC.into())
            .unwrap()
            .insert("compiler".to_string(), self.compiler.clone().into())
            .unwrap()
            .insert("source".to_string(), self.source.clone().into())
            .unwrap()
            .insert("tokens".to_string(), self.tokens.iter().map(|t| t.to_json()).collect::<Vec<_>>().into())
            .unwrap()
            .insert("script".to_string(), base64::encode(&self.script).into())
            .unwrap()
            .insert("checksum".to_string(), self.checksum.into())
    }
}

impl ISerializable for NefFile {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize<W: Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        writer.write_u32::<LittleEndian>(Self::MAGIC)?;
        writer.write_fixed_string(&self.compiler, 64)?;
        writer.write_var_string(&self.source)?;
        writer.write_u8(0)?;
        writer.write_var_array(&self.tokens)?;
        writer.write_u16::<LittleEndian>(0)?;
        writer.write_var_bytes(&self.script)?;
        writer.write_u32::<LittleEndian>(self.checksum)?;
        Ok(())
    }

    fn deserialize<R: Read>(&mut self, reader: &mut R, verify: bool) -> Result<(), std::io::Error> {
        let start_position = reader.stream_position()?;
        if reader.read_u32::<LittleEndian>()? != Self::MAGIC {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Wrong magic"));
        }
        self.compiler = reader.read_fixed_string(64)?;
        self.source = reader.read_var_string(256)?;
        if reader.read_u8()? != 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Reserved bytes must be 0"));
        }
        self.tokens = reader.read_var_array(128)?;
        if reader.read_u16::<LittleEndian>()? != 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Reserved bytes must be 0"));
        }
        self.script = reader.read_var_bytes(ExecutionEngineLimits::default().max_item_size as usize)?;
        if self.script.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Script can't be empty"));
        }
        self.checksum = reader.read_u32::<LittleEndian>()?;
        if verify {
            if self.checksum != self.compute_checksum() {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "CRC verification fail"));
            }
            if reader.stream_position()? - start_position > ExecutionEngineLimits::default().max_item_size as u64 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Max vm item size exceed"));
            }
        }
        Ok(())
    }
}
