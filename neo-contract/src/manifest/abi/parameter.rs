use alloc::string::String;

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::ParameterKind;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContractParameter {
    pub name: String,
    pub kind: ParameterKind,
}

impl NeoEncode for ContractParameter {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.name.neo_encode(writer);
        self.kind.neo_encode(writer);
    }
}

impl NeoDecode for ContractParameter {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            name: String::neo_decode(reader)?,
            kind: ParameterKind::neo_decode(reader)?,
        })
    }
}
