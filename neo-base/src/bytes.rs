// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

use crate::encoding::*;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Bytes(pub(crate) Vec<u8>);

impl Bytes {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self {
        Bytes(value)
    }
}

impl Into<Vec<u8>> for Bytes {
    fn into(self) -> Vec<u8> {
        self.0
    }
}

impl Default for Bytes {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl BinEncoder for Bytes {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write_varint(self.0.len() as u64);
        w.write(&self.0);
    }
}

impl BinDecoder for Bytes {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let len = r.read_varint_le()?;
        let mut buf = Vec::with_capacity(len as usize);
        r.read_full(buf.as_mut_slice())?;
        Ok(Bytes(buf))
    }
}

impl Serialize for Bytes {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_base64_std())
    }
}

impl<'de> Deserialize<'de> for Bytes {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Vec::from_base64_std(&value)
            .map(|v| v.into())
            .map_err(D::Error::custom)
    }
}
