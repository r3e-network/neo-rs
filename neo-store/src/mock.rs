// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::{collections::BTreeMap, sync::{Arc, Mutex}};

use neo_base::errors;
use neo_core::store::{*, chain::{PutError, GetError}};


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

    fn delete(&self, key: &[u8], options: &WriteOptions) -> Result<(), Self::WriteError> {
        let mut inner = self.inner.lock().unwrap();
        if let Versions::Expected(expected) = options.version {
            if let Some(existed) = inner.store.get(key) {
                if existed.version == expected {
                    inner.store.remove(key);
                    return Ok(());
                }
            }
            return Err(WriteError::Conflict);
        }

        inner.store.remove(key);
        Ok(())
    }

    fn put(&self, key: &[u8], value: &[u8], options: &WriteOptions) -> Result<Version, Self::WriteError> {
        let mut inner = self.inner.lock().unwrap();
        if let Versions::Expected(expected) = options.version {
            if let Some(existed) = inner.store.get(key) {
                if existed.version == expected {
                    let version = inner.next_version();
                    inner.store.insert(key.to_vec(), Item { version, value: value.to_vec() });
                    return Ok(version);
                }
            }
            return Err(WriteError::Conflict);
        } else if let Versions::IfNotExist = options.version {
            if inner.store.get(key).is_some() {
                return Err(WriteError::Conflict);
            }
        }

        let version = inner.next_version();
        inner.store.insert(key.to_vec(), Item { version, value: value.to_vec() });
        Ok(version)
    }

    // fn write_batch<W: crate::WriteBatch>(&mut self) -> W {}
}