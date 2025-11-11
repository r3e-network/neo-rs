use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};
use serde::{Deserialize, Serialize};

/// Matches C# `ContractFeatures`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractFeatures {
    #[serde(default)]
    pub storage: bool,
    #[serde(default)]
    pub payable: bool,
}

impl NeoEncode for ContractFeatures {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.storage.neo_encode(writer);
        self.payable.neo_encode(writer);
    }
}

impl NeoDecode for ContractFeatures {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            storage: bool::neo_decode(reader)?,
            payable: bool::neo_decode(reader)?,
        })
    }
}
