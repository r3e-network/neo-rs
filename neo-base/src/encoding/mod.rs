// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::String;
use serde::{Deserialize, Deserializer, Serializer, de::Error};

pub mod bin;
pub mod base58;
pub mod base64;
pub mod hex;

pub mod wif;


#[inline]
pub fn encode_hex_u64<S: Serializer>(item: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&alloc::format!("{:016X}", item))
}

#[inline]
pub fn decode_hex_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let hex: String = Deserialize::deserialize(deserializer)?;
    u64::from_str_radix(hex.as_str(), 16)
        .map_err(|err| D::Error::custom(err))
}
