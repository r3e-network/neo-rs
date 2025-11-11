use alloc::{string::String, vec::Vec};

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use crate::manifest::util::{decode_vec, encode_vec};

use super::ContractParameter;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContractEvent {
    pub name: String,
    #[serde(default)]
    pub parameters: Vec<ContractParameter>,
}

impl NeoEncode for ContractEvent {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.name.neo_encode(writer);
        encode_vec(writer, &self.parameters);
    }
}

impl NeoDecode for ContractEvent {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            name: String::neo_decode(reader)?,
            parameters: decode_vec(reader)?,
        })
    }
}
