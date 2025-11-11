use alloc::{format, string::String};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

use crate::h160::{ToH160Error, H160};

impl Serialize for H160 {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for H160 {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        H160::try_from(value.as_str()).map_err(|err| match err {
            ToH160Error::InvalidLength => D::Error::custom("invalid H160 length"),
            ToH160Error::InvalidChar(c) => D::Error::custom(format!("invalid char {c}")),
        })
    }
}
