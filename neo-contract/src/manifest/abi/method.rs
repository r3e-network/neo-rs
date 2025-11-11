use alloc::{string::String, vec::Vec};

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use crate::manifest::util::{decode_vec, encode_vec};

use super::{ContractParameter, ParameterKind};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContractMethod {
    pub name: String,
    #[serde(default)]
    pub parameters: Vec<ContractParameter>,
    pub return_type: ParameterKind,
    pub offset: u16,
    pub safe: bool,
}

impl ContractMethod {
    pub fn parameter_count(&self) -> usize {
        self.parameters.len()
    }
}

impl NeoEncode for ContractMethod {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.name.neo_encode(writer);
        encode_vec(writer, &self.parameters);
        self.return_type.neo_encode(writer);
        writer.write_u16(self.offset);
        self.safe.neo_encode(writer);
    }
}

impl NeoDecode for ContractMethod {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            name: String::neo_decode(reader)?,
            parameters: decode_vec(reader)?,
            return_type: ParameterKind::neo_decode(reader)?,
            offset: reader.read_u16()?,
            safe: bool::neo_decode(reader)?,
        })
    }
}
