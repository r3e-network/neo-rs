use std::io::{Read, Write};
use NeoRust::prelude::StringExt;
use neo_json::jtoken::JToken;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::call_flags::CallFlags;
use neo_type::H160;

/// Represents the methods that a contract will call statically.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MethodToken {
    /// The hash of the contract to be called.
    pub hash: H160,

    /// The name of the method to be called.
    pub method: String,

    /// The number of parameters of the method to be called.
    #[serde(rename = "paramcount")]
    pub parameters_count: u16,

    /// Indicates whether the method to be called has a return value.
    #[serde(rename = "hasreturnvalue")]
    pub has_return_value: bool,

    /// The CallFlags to be used to call the contract.
    #[serde(rename = "callflags")]
    pub call_flags: CallFlags,
}

impl JsonConvertibleTrait for MethodToken {

    /// Converts the token to a JSON object.
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "hash": self.hash.to_string(),
            "method": self.method,
            "paramcount": self.parameters_count,
            "hasreturnvalue": self.has_return_value,
            "callflags": self.call_flags
        })
    }

    fn from_json(json: &serde_json::Value) -> Result<Self, JsonError> {
        let hash = json["hash"].as_str()
            .ok_or(JsonError::InvalidFormat)?;
        let hash = H160::from_str(hash).map_err(|_| JsonError::InvalidFormat)?;

        let method = json["method"].as_str()
            .ok_or(JsonError::InvalidFormat)?;
        
        let parameters_count = json["paramcount"].as_u64()
            .ok_or(JsonError::InvalidFormat)? as u16;
        
        let has_return_value = json["hasreturnvalue"].as_bool()
            .ok_or(JsonError::InvalidFormat)?;
        
        let call_flags = CallFlags::from_bits(json["callflags"].as_u64()
            .ok_or(JsonError::InvalidFormat)? as u8)
            .ok_or(JsonError::InvalidFormat)?;

        Ok(Self {
            hash,
            method: method.to_string(),
            parameters_count,
            has_return_value,
            call_flags,
        })
    }
}

impl SerializableTrait for MethodToken {
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
