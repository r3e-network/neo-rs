// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

mod base58;
mod base64;
mod bin;
mod hex;
mod wif;

pub use base58::*;
pub use base64::*;
pub use bin::*;
pub use hex::*;
pub use wif::*;

use alloc::string::String;

use alloc::format;
use serde::{de::Error, Deserialize, Deserializer, Serializer};

#[inline]
pub fn encode_hex_u64<S: Serializer>(item: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{:016X}", item))
}

#[inline]
pub fn decode_hex_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let hex: String = Deserialize::deserialize(deserializer)?;
    u64::from_str_radix(hex.as_str(), 16).map_err(|err| D::Error::custom(err))
}
