use getset::{Getters, Setters};
use neo_base::encoding::base58::{FromBase58Check, FromBase58CheckError, ToBase58Check};
use neo_base::errors;
use neo_crypto::secp256r1::PublicKey;
use serde::{Deserialize, Serialize};

use crate::{
    ADDRESS_NEO3, CheckSign, MultiCheckSign, SCRIPT_HASH_SIZE, ScriptHash, ToCheckSign,
    ToScriptHash,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Getters, Setters)]
pub struct Address {
    #[getset(get = "pub", set = "pub")]
    pub version:     u8,
    #[getset(get = "pub", set = "pub")]
    pub base58check: String,
}

impl Address {
    #[inline]
    pub fn version(&self) -> u8 {
        self.version
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        self.base58check.as_str()
    }
}

impl AsRef<str> for Address {
    #[inline]
    fn as_ref(&self) -> &str {
        self.base58check.as_str()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, errors::Error)]
pub enum ToAddressError {
    #[error("to-address: invalid character '{0}'")]
    InvalidChar(char),

    #[error("to-address: invalid length")]
    InvalidLength,

    #[error("to-address: invalid checksum")]
    InvalidChecksum,

    #[error("to-address: invalid address")]
    InvalidAddress,

    #[error("to-address: invalid version '0x{0:x}'")]
    InvalidVersion(u8),
}

impl From<FromBase58CheckError> for ToAddressError {
    #[inline]
    fn from(value: FromBase58CheckError) -> Self {
        use FromBase58CheckError as Error;
        match value {
            Error::InvalidChar(ch) => ToAddressError::InvalidChar(ch),
            Error::InvalidLength => ToAddressError::InvalidLength,
            Error::InvalidChecksum => ToAddressError::InvalidChecksum,
            _ => ToAddressError::InvalidAddress,
        }
    }
}

impl TryFrom<&str> for Address {
    type Error = ToAddressError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let check = Vec::from_base58_check(value, None, None).map_err(Self::Error::from)?;

        if check.len() != 21 {
            return Err(Self::Error::InvalidLength);
        }

        if check[0] != ADDRESS_NEO3 {
            // NEO2 is not supported at now.
            return Err(Self::Error::InvalidVersion(check[0]));
        }

        Ok(Self { version: check[0], base58check: value.into() })
    }
}

pub trait ToNeo3Address {
    fn to_neo3_address(&self) -> Address;
}

impl ToNeo3Address for ScriptHash {
    #[inline]
    fn to_neo3_address(&self) -> Address {
        let mut addr = [0u8; 1 + SCRIPT_HASH_SIZE];
        addr[0] = ADDRESS_NEO3;
        addr[1..].copy_from_slice(self.0.as_ref());

        Address { version: ADDRESS_NEO3, base58check: addr.to_base58_check(None, None) }
    }
}

impl ToNeo3Address for CheckSign {
    #[inline]
    fn to_neo3_address(&self) -> Address {
        self.to_script_hash().to_neo3_address()
    }
}

impl ToNeo3Address for MultiCheckSign {
    #[inline]
    fn to_neo3_address(&self) -> Address {
        self.to_script_hash().to_neo3_address()
    }
}

impl ToNeo3Address for PublicKey {
    #[inline]
    fn to_neo3_address(&self) -> Address {
        self.to_check_sign().to_script_hash().to_neo3_address()
    }
}
