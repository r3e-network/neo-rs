// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

mod builder;
mod opcode;

pub use builder::*;
pub use opcode::*;

use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

use neo_base::encoding::{FromBase64, ToBase64};

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
            .map(|v| Self::new(v))
            .map_err(D::Error::custom)
    }
}
