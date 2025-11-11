use alloc::{format, string::String};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

use crate::h256::{ToH256Error, H256};

impl Serialize for H256 {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for H256 {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        H256::try_from(value.as_str()).map_err(|err| match err {
            ToH256Error::InvalidLength => D::Error::custom("invalid H256 length"),
            ToH256Error::InvalidChar(c) => D::Error::custom(format!("invalid char {c}")),
        })
    }
}
