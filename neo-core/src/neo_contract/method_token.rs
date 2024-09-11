use std::io::{Read, Write};
use NeoRust::prelude::StringExt;
use neo_json::jtoken::JToken;
use crate::io::iserializable::ISerializable;
use crate::neo_contract::call_flags::CallFlags;
use crate::uint160::UInt160;

/// Represents the methods that a contract will call statically.
pub struct MethodToken {
    /// The hash of the contract to be called.
    pub hash: UInt160,

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
    pub fn size(&self) -> usize {
        UInt160::LEN +
        self.method.var_size() +
        std::mem::size_of::<u16>() +
        std::mem::size_of::<bool>() +
        std::mem::size_of::<CallFlags>()
    }

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
    fn deserialize<R: Read>(&mut self, reader: &mut R) -> Result<(), std::io::Error> {
        self.hash = UInt160::deserialize(reader)?;
        self.method = reader.read_var_string(32)?;
        if self.method.starts_with('_') {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Method name cannot start with underscore"));
        }
        self.parameters_count = reader.read_u16()?;
        self.has_return_value = reader.read_bool()?;
        self.call_flags = CallFlags::from_u8(reader.read_u8()?);
        if (self.call_flags & !CallFlags::ALL) != CallFlags::empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid CallFlags"));
        }
        Ok(())
    }

    fn serialize<W: Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        self.hash.serialize(writer)?;
        writer.write_var_string(&self.method)?;
        writer.write_u16(self.parameters_count)?;
        writer.write_bool(self.has_return_value)?;
        writer.write_u8(self.call_flags.bits())?;
        Ok(())
    }

    fn size(&self) -> usize {
        todo!()
    }
}
