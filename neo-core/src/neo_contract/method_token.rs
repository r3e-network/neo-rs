use std::io::{Read, Write};
use NeoRust::prelude::StringExt;
use neo_json::jtoken::JToken;
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::call_flags::CallFlags;
use neo_type::H160;

/// Represents the methods that a contract will call statically.
pub struct MethodToken {
    /// The hash of the contract to be called.
    pub hash: H160,

    /// The name of the method to be called.
    pub method: String,

    /// The number of parameters of the method to be called.
    pub parameters_count: u16,

    /// Indicates whether the method to be called has a return value.
    pub has_return_value: bool,

    /// The CallFlags to be used to call the contract.
    pub call_flags: CallFlags,
}

impl MethodToken {


    /// Converts the token to a JSON object.
    pub fn to_json(&self) -> JToken {
        JToken::new_object()
            .insert("hash".to_string(), self.hash.to_string())
            .unwrap()
            .insert("method".to_string(), self.method.clone())
            .unwrap()
            .insert("paramcount".to_string(), self.parameters_count)
            .unwrap()
            .insert("hasreturnvalue".to_string(), self.has_return_value)
            .unwrap()
            .insert("callflags".to_string(), self.call_flags)
            .unwrap()


    }
}

impl ISerializable for MethodToken {
    fn size(&self) -> usize {
        H160::LEN +
            self.method.var_size() +
            std::mem::size_of::<u16>() +
            std::mem::size_of::<bool>() +
            std::mem::size_of::<CallFlags>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        self.hash.serialize(writer)?;
        writer.write_var_string(&self.method)?;
        writer.write_u16(self.parameters_count)?;
        writer.write_bool(self.has_return_value)?;
        writer.write_u8(self.call_flags.bits())?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let hash = H160::deserialize(reader)?;
        let method = reader.read_var_string(32)?;
        if method.starts_with('_') {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Method name cannot start with underscore"));
        }
        let parameters_count = reader.read_u16()?;
        let has_return_value = reader.read_bool()?;
        let call_flags = CallFlags::from_bits(reader.read_u8()?).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid CallFlags")
        })?;

        Ok(Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }
}
