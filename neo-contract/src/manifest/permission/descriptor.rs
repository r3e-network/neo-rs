use alloc::string::String;
use core::{fmt, str::FromStr};

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    hash::Hash160,
};
use neo_crypto::ecc256::PublicKey;
use serde::{Deserialize, Serialize};

use crate::manifest::ContractGroup;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContractPermissionDescriptor {
    Wildcard,
    Hash(Hash160),
    Group(PublicKey),
}

impl ContractPermissionDescriptor {
    pub fn wildcard() -> Self {
        Self::Wildcard
    }

    pub fn for_hash(hash: Hash160) -> Self {
        Self::Hash(hash)
    }

    pub fn for_group(group: PublicKey) -> Self {
        Self::Group(group)
    }

    pub fn matches_contract(&self, contract_hash: &Hash160, groups: &[ContractGroup]) -> bool {
        match self {
            Self::Wildcard => true,
            Self::Hash(hash) => hash == contract_hash,
            Self::Group(key) => groups.iter().any(|g| &g.public_key == key),
        }
    }
}

impl NeoEncode for ContractPermissionDescriptor {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        match self {
            Self::Wildcard => writer.write_u8(0),
            Self::Hash(hash) => {
                writer.write_u8(1);
                hash.neo_encode(writer);
            }
            Self::Group(group) => {
                writer.write_u8(2);
                group.neo_encode(writer);
            }
        }
    }
}

impl NeoDecode for ContractPermissionDescriptor {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(match reader.read_u8()? {
            0 => Self::Wildcard,
            1 => Self::Hash(Hash160::neo_decode(reader)?),
            2 => Self::Group(PublicKey::neo_decode(reader)?),
            _ => return Err(DecodeError::InvalidValue("ContractPermissionDescriptor")),
        })
    }
}

impl Serialize for ContractPermissionDescriptor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Wildcard => serializer.serialize_str("*"),
            Self::Hash(hash) => serializer.serialize_str(&hash.to_string()),
            Self::Group(group) => serializer.serialize_str(&hex::encode(group.to_compressed())),
        }
    }
}

impl<'de> Deserialize<'de> for ContractPermissionDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == "*" {
            return Ok(Self::Wildcard);
        }
        if let Ok(hash) = Hash160::from_str(&value) {
            return Ok(Self::Hash(hash));
        }
        let bytes = hex::decode(&value).map_err(|e| serde::de::Error::custom(e.to_string()))?;
        let key = PublicKey::from_sec1_bytes(&bytes)
            .map_err(|_| serde::de::Error::custom("invalid group key"))?;
        Ok(Self::Group(key))
    }
}

impl fmt::Display for ContractPermissionDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wildcard => write!(f, "*"),
            Self::Hash(hash) => write!(f, "{hash}"),
            Self::Group(group) => write!(f, "{}", hex::encode(group.to_compressed())),
        }
    }
}
