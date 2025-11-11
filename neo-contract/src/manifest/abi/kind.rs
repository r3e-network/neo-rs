use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

/// Matches C# `ContractParameterType`.
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum ParameterKind {
    Signature = 0x00,
    Boolean = 0x01,
    Integer = 0x02,
    Hash160 = 0x03,
    Hash256 = 0x04,
    ByteArray = 0x05,
    PublicKey = 0x06,
    String = 0x07,
    Array = 0x10,
    Map = 0x12,
    InteropInterface = 0x40,
    Any = 0xfe,
    Void = 0xff,
}

impl ParameterKind {
    pub fn from_byte(value: u8) -> Result<Self, DecodeError> {
        Ok(match value {
            0x00 => ParameterKind::Signature,
            0x01 => ParameterKind::Boolean,
            0x02 => ParameterKind::Integer,
            0x03 => ParameterKind::Hash160,
            0x04 => ParameterKind::Hash256,
            0x05 => ParameterKind::ByteArray,
            0x06 => ParameterKind::PublicKey,
            0x07 => ParameterKind::String,
            0x10 => ParameterKind::Array,
            0x12 => ParameterKind::Map,
            0x40 => ParameterKind::InteropInterface,
            0xfe => ParameterKind::Any,
            0xff => ParameterKind::Void,
            _ => return Err(DecodeError::InvalidValue("ParameterKind")),
        })
    }
}

impl NeoEncode for ParameterKind {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(*self as u8);
    }
}

impl NeoDecode for ParameterKind {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Self::from_byte(reader.read_u8()?)
    }
}
