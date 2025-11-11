use alloc::{string::String, vec::Vec};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

use crate::encoding::{FromBase64, ToBase64};

use super::Bytes;

impl Serialize for Bytes {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_base64_std())
    }
}

impl<'de> Deserialize<'de> for Bytes {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        Vec::from_base64_std(&encoded)
            .map(Bytes)
            .map_err(D::Error::custom)
    }
}
