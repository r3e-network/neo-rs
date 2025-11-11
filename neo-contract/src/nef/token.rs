use alloc::string::String;

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    hash::Hash160,
};

use crate::nef::util::{read_limited_string, validate_method_name};

use super::{flags::CallFlags, METHOD_NAME_MAX};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MethodToken {
    pub hash: Hash160,
    pub method: String,
    #[serde(rename = "paramcount")]
    pub parameters_count: u16,
    #[serde(rename = "hasreturnvalue")]
    pub has_return_value: bool,
    #[serde(rename = "callflags")]
    pub call_flags: CallFlags,
}

impl MethodToken {
    pub fn new(
        hash: Hash160,
        method: impl Into<String>,
        parameters_count: u16,
        has_return_value: bool,
        call_flags: CallFlags,
    ) -> Result<Self, DecodeError> {
        let method = method.into();
        validate_method_name(&method)?;
        Ok(Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }
}

impl NeoEncode for MethodToken {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.hash.neo_encode(writer);
        writer.write_var_bytes(self.method.as_bytes());
        writer.write_u16(self.parameters_count);
        self.has_return_value.neo_encode(writer);
        writer.write_u8(self.call_flags.bits());
    }
}

impl NeoDecode for MethodToken {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let hash = Hash160::neo_decode(reader)?;
        let method = read_limited_string(reader, METHOD_NAME_MAX, "MethodToken.method")?;
        validate_method_name(&method)?;
        let parameters_count = reader.read_u16()?;
        let has_return_value = bool::neo_decode(reader)?;
        let flags = reader.read_u8()?;
        let call_flags = CallFlags::from_bits(flags)
            .ok_or(DecodeError::InvalidValue("MethodToken.callflags"))?;
        Ok(Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }
}
