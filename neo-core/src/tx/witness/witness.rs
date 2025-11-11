use crate::script::Script;
use neo_base::encoding::{NeoDecode, NeoEncode, NeoRead, NeoWrite};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Witness {
    pub invocation_script: Script,
    pub verification_script: Script,
}

impl Witness {
    pub fn new(invocation: Script, verification: Script) -> Self {
        Self {
            invocation_script: invocation,
            verification_script: verification,
        }
    }
}

impl NeoEncode for Witness {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.invocation_script.neo_encode(writer);
        self.verification_script.neo_encode(writer);
    }
}

impl NeoDecode for Witness {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, neo_base::encoding::DecodeError> {
        let invocation = Script::neo_decode(reader)?;
        let verification = Script::neo_decode(reader)?;
        Ok(Self::new(invocation, verification))
    }
}
