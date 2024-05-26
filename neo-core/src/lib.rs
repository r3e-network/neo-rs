// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(feature = "std", feature = "enclave"))]
compile_error!("feature 'std' and 'enclave' cannot be enabled both");

extern crate alloc;
extern crate core;

pub mod block;
pub mod h160;
pub mod h256;
pub mod script;
pub mod sign;
pub mod tx;
pub mod wallet;

use alloc::string::String;

use crate::h160::H160;
use crate::sign::ToCheckSign;
use neo_base::encoding::ToBase58Check;
use neo_base::hash::{Ripemd160, Sha256};
use neo_crypto::ecc256::PublicKey;

const SCRIPT_HASH_SIZE: usize = 20;
const SIGN_DATA_SIZE: usize = 4 + 32;
const ADDRESS_NEO3: u8 = 0x35;

pub trait ToScriptHash {
    fn to_script_hash(&self) -> H160;
}

impl<T: AsRef<[u8]>> ToScriptHash for T {
    #[inline]
    fn to_script_hash(&self) -> H160 {
        H160::from_le_bytes(self.as_ref().sha256().ripemd160())
    }
}

pub trait ToSignData {
    fn to_sign_data(&self, network: u32) -> [u8; SIGN_DATA_SIZE];
}

impl<T: AsRef<[u8]>> ToSignData for T {
    #[inline]
    fn to_sign_data(&self, network: u32) -> [u8; SIGN_DATA_SIZE] {
        let mut data = [0u8; SIGN_DATA_SIZE];
        let hash = self.as_ref().sha256();
        data[..4].copy_from_slice(&network.to_le_bytes());
        data[4..].copy_from_slice(&hash);
        data
    }
}

pub struct Address {
    version: u8,
    base58check: String,
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

pub trait ToNeo3Address {
    fn to_neo3_address(&self) -> Address;
}

impl ToNeo3Address for H160 {
    #[inline]
    fn to_neo3_address(&self) -> Address {
        let mut addr = [0u8; 1 + SCRIPT_HASH_SIZE];
        addr[0] = ADDRESS_NEO3;
        addr[1..].copy_from_slice(self.as_le_bytes());

        Address {
            version: ADDRESS_NEO3,
            base58check: addr.to_base58_check(),
        }
    }
}

impl ToNeo3Address for PublicKey {
    #[inline]
    fn to_neo3_address(&self) -> Address {
        self.to_check_sign()
            .as_bytes()
            .to_script_hash()
            .to_neo3_address()
    }
}

impl Into<String> for Address {
    fn into(self) -> String {
        self.base58check
    }
}
