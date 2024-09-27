// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;
use core::fmt::Debug;

use neo_base::encoding::bin::*;
use neo_base::errors;

pub use {chain::*, contract::*, dbft::*, policy::*, snapshot::*, states::*};

pub mod chain;
pub mod contract;
pub mod dbft;
pub mod policy;
pub mod snapshot;
pub mod states;

pub const VOTER_REWARD_FACTOR: u64 = 100_000_000;

pub const PREFIX_GASPER_BLOCK: u32 = 29;

pub const EXEC_BLOCK: u8 = 1;
pub const EXEC_TX: u8 = 2;

pub const NOT_EXISTS: Version = 0;

/// Data Access Layer Config
#[derive(Debug, Clone)]
pub struct DalConfig {
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
    pub fn with_if_not_exists() -> Self { WriteOptions { version: Versions::IfNotExist } }

    #[inline]
    pub fn with_expected(version: Version) -> Self {
        WriteOptions { version: Versions::Expected(version) }
    }

    #[inline]
    pub fn with_always() -> Self { WriteOptions { version: Versions::Always } }
}

impl Default for WriteOptions {
    #[inline]
    fn default() -> Self { Self { version: Versions::Always } }
}

#[derive(Debug, Clone, errors::Error)]
pub enum ReadError {
    #[error("store-read: no-such-key")]
    NoSuchKey,
}

impl ReadError {
    pub fn is_not_found(&self) -> bool { matches!(self, Self::NoSuchKey) }
}

pub trait ReadOnlyStore: Clone + Sync + Send {
    fn get(&self, key: &[u8]) -> Result<(Vec<u8>, Version), ReadError>;

    fn contains(&self, key: &[u8]) -> Result<Version, ReadError>;
}

#[derive(Debug, Clone, errors::Error)]
pub enum WriteError {
    #[error("store-write: conflicted")]
    Conflicted,
}

pub trait Store: ReadOnlyStore {
    type WriteBatch: WriteBatch;

    fn delete(&self, key: &[u8], options: &WriteOptions) -> Result<Version, WriteError>;

    fn put(&self, key: Vec<u8>, value: Vec<u8>, options: &WriteOptions) -> Result<Version, WriteError>;

    fn write_batch(&self) -> Self::WriteBatch;
}

pub struct BatchWritten {
    pub deleted: Vec<Version>,
    pub put: Vec<Version>,
}

#[derive(Debug, Clone, errors::Error)]
pub enum CommitError {
    #[error("store-write: conflicted")]
    Conflicted,
}

pub trait WriteBatch {
    fn add_delete(&mut self, key: Vec<u8>, options: &WriteOptions);

    fn add_put(&mut self, key: Vec<u8>, value: Vec<u8>, options: &WriteOptions);

    fn commit(self) -> Result<BatchWritten, CommitError>;
}

#[derive(Debug, errors::Error)]
pub enum BinWriteError {
    #[error("chain-write: already exists")]
    AlreadyExists,
}

impl From<WriteError> for BinWriteError {
    #[inline]
    fn from(value: WriteError) -> Self {
        match value {
            WriteError::Conflicted => Self::AlreadyExists,
        }
    }
}

#[derive(Debug, errors::Error)]
pub enum BinReadError {
    #[error("bin-read: no-such-key")]
    NoSuchKey,

    #[error("bin-read: bin-decode-error: {0}")]
    BinDecodeError(BinDecodeError),
}

impl BinReadError {
    pub fn is_not_found(&self) -> bool { matches!(self, Self::NoSuchKey) }
}

impl From<ReadError> for BinReadError {
    #[inline]
    fn from(value: ReadError) -> Self {
        match value {
            ReadError::NoSuchKey => Self::NoSuchKey,
        }
    }
}

pub trait GetBinEncoded {
    fn get_bin_encoded<T: BinDecoder>(&self, key: &[u8]) -> Result<T, BinReadError>;
}

impl<Store: ReadOnlyStore> GetBinEncoded for Store {
    fn get_bin_encoded<T: BinDecoder>(&self, key: &[u8]) -> Result<T, BinReadError> {
        let (data, _version) = self.get(key)
            .map_err(|err| BinReadError::from(err))?;

        let mut rb = RefBuffer::from(data.as_slice());
        BinDecoder::decode_bin(&mut rb)
            .map_err(|err| BinReadError::BinDecodeError(err))
    }
}

#[derive(Debug, BinEncode)]
pub struct StoreKey<'a, Key: BinEncoder + Debug> {
    pub contract_id: u32,
    pub prefix: u8,
    pub key: &'a Key,
}

impl<'a, Key: BinEncoder + Debug> StoreKey<'a, Key> {
    #[inline]
    pub fn new(contract_id: u32, prefix: u8, key: &'a Key) -> Self {
        Self { contract_id, prefix, key }
    }
}

#[cfg(test)]
mod test {
    use neo_base::{encoding::hex::ToHex, hash::Sha256};

    use super::*;
    use neo_type::H256;

    #[test]
    fn test_store_key() {
        let key =
            StoreKey::new(0xffff_fffb, PREFIX_TX, &H256::from("Hello".sha256())).to_bin_encoded();
        assert_eq!(
            &key.to_hex(),
            "fbffffff0b185f8db32271fe25f561a6fc938b2e264306ec304eda518007d1764826381969"
        );
    }
}
