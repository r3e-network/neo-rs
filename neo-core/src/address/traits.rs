use crate::{h160::H160, sign::ToCheckSign};
use neo_base::{
    encoding::ToBase58Check,
    hash::{hash160, sha256},
};
use neo_crypto::ecc256::PublicKey;

use super::{Address, ADDRESS_NEO3};

const SCRIPT_HASH_SIZE: usize = 20;
const SIGN_DATA_SIZE: usize = 4 + 32;

pub trait ToScriptHash {
    fn to_script_hash(&self) -> H160;
}

impl<T: AsRef<[u8]>> ToScriptHash for T {
    #[inline]
    fn to_script_hash(&self) -> H160 {
        H160::from_le_bytes(hash160(self.as_ref()))
    }
}

pub trait ToSignData {
    fn to_sign_data(&self, network: u32) -> [u8; SIGN_DATA_SIZE];
}

impl<T: AsRef<[u8]>> ToSignData for T {
    #[inline]
    fn to_sign_data(&self, network: u32) -> [u8; SIGN_DATA_SIZE] {
        let mut data = [0u8; SIGN_DATA_SIZE];
        let hash = sha256(self.as_ref());
        data[..4].copy_from_slice(&network.to_le_bytes());
        data[4..].copy_from_slice(&hash);
        data
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

        Address::new(ADDRESS_NEO3, addr.to_base58_check())
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
