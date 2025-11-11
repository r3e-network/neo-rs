use alloc::vec::Vec;

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::{ContractEvent, ContractMethod};
use crate::manifest::util::{decode_vec, encode_vec};

/// Application binary interface for a contract (mirrors C# `ContractAbi`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContractAbi {
    #[serde(default)]
    pub methods: Vec<ContractMethod>,
    #[serde(default)]
    pub events: Vec<ContractEvent>,
}

impl ContractAbi {
    pub fn find_method(&self, name: &str, parameter_count: usize) -> Option<&ContractMethod> {
        self.methods
            .iter()
            .find(|method| method.name == name && method.parameters.len() == parameter_count)
    }
}

impl NeoEncode for ContractAbi {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        encode_vec(writer, &self.methods);
        encode_vec(writer, &self.events);
    }
}

impl NeoDecode for ContractAbi {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            methods: decode_vec(reader)?,
            events: decode_vec(reader)?,
        })
    }
}
