use alloc::{string::String, vec::Vec};

use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

use neo_base::encoding::{FromBase64, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToBase64};

pub const MAX_SCRIPT_LENGTH: usize = 1024 * 512;

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct Script {
    script: Vec<u8>,
}

impl Script {
    pub fn new(script: Vec<u8>) -> Self {
        Self { script }
    }

    pub fn len(&self) -> usize {
        self.script.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.script.as_slice()
    }
}

impl Serialize for Script {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.script.to_base64_std())
    }
}

impl<'de> Deserialize<'de> for Script {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Vec::from_base64_std(&value.as_bytes())
            .map(Script::new)
            .map_err(D::Error::custom)
    }
}

impl NeoEncode for Script {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(&self.script);
    }
}

impl NeoDecode for Script {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, neo_base::encoding::DecodeError> {
        let bytes = reader.read_var_bytes(MAX_SCRIPT_LENGTH as u64)?;
        Ok(Self::new(bytes))
    }
}
