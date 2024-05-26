// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


pub mod chain;
pub mod consensus;
pub mod snapshot;


use alloc::vec::Vec;
use neo_base::encoding::bin::*;


/// neo contract id is -5
pub const NEO_CONTRACT_ID: u32 = 0xffff_fffb;

pub const VOTER_REWARD_FACTOR: u64 = 100_000_000;

pub const PREFIX_BLOCK: u8 = 5;
pub const PREFIX_INDEX_TO_HASH: u8 = 9;
pub const PREFIX_TX: u8 = 11;
pub const PREFIX_CURRENT_BLOCK: u8 = 12;

pub const PREFIX_CANDIDATE: u32 = 33;
pub const PREFIX_VOTERS_COUNT: u32 = 1;

pub const PREFIX_GASPER_BLOCK: u32 = 29;
pub const PREFIX_REGISTER_PRICE: u32 = 13;

pub const PREFIX_VOTER_REWARD_PER_COMMITTEE: u64 = 23;

pub const EXEC_BLOCK: u8 = 1;
pub const EXEC_TX: u8 = 2;

pub const NOT_EXISTS: Version = 0;


/// Data Access Layer Settings
#[derive(Debug, Clone)]
pub struct Settings {
    pub network: u32,

    // pub p2p_sign: bool,
    // pub p2p_state_exchange: bool,
    // pub keep_latest_state_only: bool,
    // pub state_root_in_header: bool,
}


pub type Version = u64;


#[derive(Debug, Copy, Clone)]
pub enum Versions {
    Always,
    IfNotExist,
    Expected(Version),
}


pub struct WriteOptions {
    pub version: Versions,
}

impl WriteOptions {
    #[inline]
    pub fn with_if_not_exists() -> Self {
        WriteOptions { version: Versions::IfNotExist }
    }

    #[inline]
    pub fn with_expected(version: Version) -> Self {
        WriteOptions { version: Versions::Expected(version) }
    }

    #[inline]
    pub fn with_always() -> Self {
        WriteOptions { version: Versions::Always }
    }
}

impl Default for WriteOptions {
    #[inline]
    fn default() -> Self { Self { version: Versions::Always } }
}


pub trait ReadOnlyStore: Clone + Sync + Send {
    type ReadError;

    fn get(&self, key: &[u8]) -> Result<(Vec<u8>, Version), Self::ReadError>;

    fn contains(&self, key: &[u8]) -> Result<Version, Self::ReadError>;
}


pub trait Store: ReadOnlyStore {
    type WriteError;

    type WriteBatch: WriteBatch;

    fn delete(&self, key: &[u8], options: &WriteOptions) -> Result<Version, Self::WriteError>;

    fn put(&self, key: Vec<u8>, value: Vec<u8>, options: &WriteOptions) -> Result<Version, Self::WriteError>;

    fn write_batch(&self) -> Self::WriteBatch;
}


pub struct BatchWritten {
    pub deleted: Vec<Version>,
    pub put: Vec<Version>,
}


pub trait WriteBatch {
    type CommitError;

    fn add_delete(&mut self, key: Vec<u8>, options: &WriteOptions);

    fn add_put(&mut self, key: Vec<u8>, value: Vec<u8>, options: &WriteOptions);

    fn commit(self) -> Result<BatchWritten, Self::CommitError>;
}


#[derive(BinEncode)]
pub struct StoreKey<'a, Key: BinEncoder> {
    pub contract_id: u32,
    pub prefix: u8,
    pub key: &'a Key,
}


#[cfg(test)]
mod test {
    use super::*;
    use crate::types::H256;
    use neo_base::{hash::Sha256, encoding::hex::ToHex};


    #[test]
    fn test_store_key() {
        let key = StoreKey::<H256> {
            contract_id: NEO_CONTRACT_ID,
            prefix: PREFIX_TX,
            key: &"Hello".sha256().into(),
        }.to_bin_encoded();
        assert_eq!(&key.to_hex(), "fbffffff0b185f8db32271fe25f561a6fc938b2e264306ec304eda518007d1764826381969");
    }
}