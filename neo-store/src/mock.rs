// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::{collections::BTreeMap, sync::{Arc, Mutex}};

use neo_base::errors;
use neo_core::store::{self, *, chain::{PutError, GetError}};


struct Item {
    pub version: Version,
    pub value: Vec<u8>,
}


#[derive(Clone)]
pub struct MockStore {
    inner: Arc<Mutex<MockInner>>,
}

impl MockStore {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(MockInner::new())) }
    }
}


#[derive(Clone)]
pub struct WriteBatch {
    deletes: Vec<(Vec<u8>, Versions)>,
    puts: Vec<(Vec<u8>, Vec<u8>, Versions)>,
    inner: Arc<Mutex<MockInner>>,
}

impl store::WriteBatch for WriteBatch {
    type CommitError = WriteError;

    fn add_delete(&mut self, key: Vec<u8>, options: &WriteOptions) {
        self.deletes.push((key, options.version));
    }

    fn add_put(&mut self, key: Vec<u8>, value: Vec<u8>, options: &WriteOptions) {
        self.puts.push((key, value, options.version));
    }

    fn commit(self) -> Result<BatchWritten, Self::CommitError> {
        let mut inner = self.inner.lock().unwrap();
        for (key, version) in self.deletes.iter() {
            if !inner.can_delete(key, *version) {
                return Err(WriteError::Conflict);
            }
        }

        for (key, _, version) in self.puts.iter() {
            if !inner.can_put(key, *version) {
                return Err(WriteError::Conflict);
            }
        }

        let deleted = self.deletes.iter()
            .map(|(key, _)| inner.store.remove(key)
                .map(|v| v.version)
                .unwrap_or(NOT_EXISTS))
            .collect();

        let put = self.puts.into_iter()
            .map(|(key, value, _)| {
                let version = inner.next_version();
                inner.store.insert(key, Item { version, value });
                version
            })
            .collect();

        Ok(BatchWritten { deleted, put })
    }
}


struct MockInner {
    version: u64,
    store: BTreeMap<Vec<u8>, Item>,
}

impl MockInner {
    fn new() -> Self {
        Self { version: 0, store: BTreeMap::new() }
    }

    fn next_version(&mut self) -> Version {
        self.version += 1;
        self.version
    }

    fn can_put(&self, key: &[u8], version: Versions) -> bool {
        if let Versions::Expected(expected) = version {
            self.store.get(key)
                .map(|v| v.version == expected)
                .unwrap_or(false)
        } else if let Versions::IfNotExist = version {
            self.store.get(key).is_none()
        } else {
            true
        }
    }

    fn can_delete(&self, key: &[u8], version: Versions) -> bool {
        if let Versions::Expected(expected) = version {
            self.store.get(key)
                .map(|v| v.version == expected)
                .unwrap_or(false)
        } else {
            true
        }
    }
}

#[derive(Debug, Eq, PartialEq, errors::Error)]
pub enum ReadError {
    #[error("read-error: no such key")]
    NoSuchKey,
}


impl Into<GetError> for ReadError {
    fn into(self) -> GetError {
        match self { ReadError::NoSuchKey => GetError::NoSuchKey }
    }
}

impl ReadOnlyStore for MockStore {
    type ReadError = ReadError;

    fn get(&self, key: &[u8]) -> Result<(Vec<u8>, Version), Self::ReadError> {
        let inner = self.inner.lock().unwrap();
        inner.store.get(key)
            .map(|m| (m.value.clone(), m.version))
            .ok_or_else(|| ReadError::NoSuchKey)
    }

    fn contains(&self, key: &[u8]) -> Result<Version, Self::ReadError> {
        let inner = self.inner.lock().unwrap();
        inner.store.get(key)
            .map(|m| m.version)
            .ok_or_else(|| ReadError::NoSuchKey)
    }
}

#[derive(Debug, errors::Error)]
pub enum WriteError {
    #[error("write-error: conflict")]
    Conflict,
}

impl Into<PutError> for WriteError {
    fn into(self) -> PutError {
        match self { WriteError::Conflict => PutError::AlreadyExists }
    }
}

impl Store for MockStore {
    type WriteError = WriteError;

    type WriteBatch = WriteBatch;

    fn delete(&self, key: &[u8], options: &WriteOptions) -> Result<Version, Self::WriteError> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.can_delete(key, options.version) {
            return Err(WriteError::Conflict);
        }

        let v = inner.store.remove(key)
            .map(|v| v.version)
            .unwrap_or(NOT_EXISTS);
        Ok(v)
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>, options: &WriteOptions) -> Result<Version, Self::WriteError> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.can_put(&key, options.version) {
            return Err(WriteError::Conflict);
        }

        let version = inner.next_version();
        inner.store.insert(key, Item { version, value });
        Ok(version)
    }

    fn write_batch(&self) -> WriteBatch {
        WriteBatch { deletes: Vec::new(), puts: Vec::new(), inner: self.inner.clone() }
    }
}