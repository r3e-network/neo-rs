// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


pub mod bytes;
pub mod check_sign;
pub mod dbft;
pub mod genesis;

pub mod h160;
pub mod h256;

pub mod script;

pub mod settings;
pub mod verifying;

pub use {
    bytes::*, check_sign::*, dbft::*, genesis::*,
    h160::*, h256::*, script::*, settings::*, verifying::*,
};

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use neo_base::{errors, encoding::{base58::*, bin::*}, hash::{Ripemd160, Sha256}};
use crate::PublicKey;


pub const SCRIPT_HASH_SIZE: usize = H160_SIZE;
pub const ACCOUNT_SIZE: usize = H160_SIZE;
pub const ADDRESS_NEO3: u8 = 0x35;

/// network(u32) + SHA256
pub const SIGN_DATA_SIZE: usize = 4 + H256_SIZE;


pub type Fee = u64;


#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct ScriptHash(pub(crate) [u8; SCRIPT_HASH_SIZE]);


impl AsRef<[u8; SCRIPT_HASH_SIZE]> for ScriptHash {
    #[inline]
    fn as_ref(&self) -> &[u8; SCRIPT_HASH_SIZE] { &self.0 }
}

impl AsRef<[u8]> for ScriptHash {
    #[inline]
    fn as_ref(&self) -> &[u8] { &self.0 }
}

impl From<H160> for ScriptHash {
    #[inline]
    fn from(value: H160) -> Self { Self(value.into()) }
}

impl Into<H160> for ScriptHash {
    #[inline]
    fn into(self) -> H160 { H160::from(self.0) }
}

pub trait ToScriptHash {
    fn to_script_hash(&self) -> ScriptHash;
}

impl ToScriptHash for [u8] {
    #[inline]
    fn to_script_hash(&self) -> ScriptHash { ScriptHash(self.sha256().ripemd160()) }
}

impl ToScriptHash for CheckSign {
    #[inline]
    fn to_script_hash(&self) -> ScriptHash { ScriptHash(self.sha256().ripemd160()) }
}

impl ToScriptHash for PublicKey {
    #[inline]
    fn to_script_hash(&self) -> ScriptHash {
        self.to_compressed().as_slice().to_script_hash()
    }
}


pub struct Address {
    version: u8,
    base58check: String,
}

impl Address {
    #[inline]
    pub fn version(&self) -> u8 { self.version }

    #[inline]
    pub fn as_str(&self) -> &str { self.base58check.as_str() }
}

impl AsRef<str> for Address {
    #[inline]
    fn as_ref(&self) -> &str { self.base58check.as_str() }
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
        let check = Vec::from_base58_check(value, None, None)
            .map_err(Self::Error::from)?;

        if check.len() != 21 {
            return Err(Self::Error::InvalidLength);
        }

        if check[0] != ADDRESS_NEO3 { // NEO2 is not supported at now.
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

        Address {
            version: ADDRESS_NEO3,
            base58check: addr.to_base58_check(None, None),
        }
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


#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum Role {
    StateValidator = 4,
    Oracle = 8,
    NeoFSAlphabet = 16,
    P2pNotary = 32,
}


#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum VmState {
    None = 0,
    Halt = 1,
    Fault = 2,
    Break = 4,
}


#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AccountId {
    version: u8,
    account: [u8; ACCOUNT_SIZE],
}

impl AccountId {
    #[inline]
    pub fn version(&self) -> u8 { self.version }
}

impl AsRef<[u8; ACCOUNT_SIZE]> for AccountId {
    #[inline]
    fn as_ref(&self) -> &[u8; ACCOUNT_SIZE] { &self.account }
}

impl AsRef<[u8]> for AccountId {
    #[inline]
    fn as_ref(&self) -> &[u8] { &self.account }
}

pub type Extra = Option<serde_json::Map<String, serde_json::Value>>;


#[cfg(test)]
mod test {
    use super::*;
    use neo_base::{bytes::ToArray, encoding::{base64::ToBase64, hex::DecodeHex}};


    #[test]
    fn test_script_hash() {
        let script = "61479ab68fd5c2c04b254f382d84ddf2f5c67ced"
            .decode_hex()
            .expect("decode hex should be ok");

        let script = ScriptHash(script.to_array());
        assert_eq!("NUnLWXALK2G6gYa7RadPLRiQYunZHnncxg", script.to_neo3_address().as_str());
        assert_eq!("YUeato/VwsBLJU84LYTd8vXGfO0=", script.to_base64_std());
    }
}
