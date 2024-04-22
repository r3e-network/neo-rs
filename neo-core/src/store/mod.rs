// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


pub mod chain;
pub mod dal;


use alloc::vec::Vec;


pub type Version = u64;


#[derive(Debug, Copy, Clone)]
pub enum Versions {
    IfNotExist,
    AnyVersion,
    Expected(Version),
}


pub struct WriteOptions {
    pub version: Versions,
}

impl Default for WriteOptions {
    fn default() -> Self { Self { version: Versions::AnyVersion } }
}


pub trait ReadOnlyStore: Clone + Sync + Send {
    type ReadError;

    fn get(&self, key: &[u8]) -> Result<(Vec<u8>, Version), Self::ReadError>;

    fn contains(&self, key: &[u8]) -> Result<Version, Self::ReadError>;
}


pub trait Store: ReadOnlyStore {
    type WriteError;

    fn delete(&self, key: &[u8], options: &WriteOptions) -> Result<(), Self::WriteError>;

    fn put(&self, key: &[u8], value: &[u8], options: &WriteOptions) -> Result<Version, Self::WriteError>;

    // fn write_batch<W: WriteBatch>(&self) -> W;
}


pub struct BatchWritten {
    pub deleted: Vec<Version>,
    pub put: Vec<Version>,
}


pub trait WriteBatch {
    type CommitError;

    // fn version(&self) -> Version;

    fn add_delete(&mut self, key: &[u8], options: &WriteOptions);

    fn add_put(&mut self, key: &[u8], value: &[u8], options: &WriteOptions);

    fn commit(&mut self) -> Result<BatchWritten, Self::CommitError>;
}
