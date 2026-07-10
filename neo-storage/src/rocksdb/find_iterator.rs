//! Concrete RocksDB prefix-scan iterators.
//!
//! `ReadOnlyStoreGeneric::find` is a hot storage boundary, so RocksDB keeps a
//! concrete enum over overlay and DB-cursor scan modes instead of boxing an
//! iterator per scan.

use rocksdb::{DB, DBIteratorWithThreadMode};
use tracing::warn;

use crate::types::{StorageItem, StorageKey};

enum RocksDbFindInner<'a> {
    Overlay(std::vec::IntoIter<(Vec<u8>, Vec<u8>)>),
    Cursor(DBIteratorWithThreadMode<'a, DB>),
}

/// Concrete raw byte iterator for RocksDB prefix scans.
pub struct RocksDbRawFindIterator<'a> {
    inner: RocksDbFindInner<'a>,
    prefix: Option<Vec<u8>>,
}

impl<'a> RocksDbRawFindIterator<'a> {
    pub(crate) fn overlay(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        Self {
            inner: RocksDbFindInner::Overlay(entries.into_iter()),
            prefix: None,
        }
    }

    pub(crate) fn cursor(
        cursor: DBIteratorWithThreadMode<'a, DB>,
        prefix: Option<Vec<u8>>,
    ) -> Self {
        Self {
            inner: RocksDbFindInner::Cursor(cursor),
            prefix,
        }
    }
}

impl Iterator for RocksDbRawFindIterator<'_> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            RocksDbFindInner::Overlay(iter) => iter.next(),
            RocksDbFindInner::Cursor(iter) => loop {
                let (key, value) = match iter.next()? {
                    Ok(entry) => entry,
                    Err(err) => {
                        warn!(target: "neo", error = %err, "rocksdb iterator error");
                        continue;
                    }
                };
                let key = key.to_vec();
                if self
                    .prefix
                    .as_ref()
                    .is_some_and(|prefix| !key.starts_with(prefix.as_slice()))
                {
                    return None;
                }
                return Some((key, value.to_vec()));
            },
        }
    }
}

/// Concrete typed storage iterator for RocksDB prefix scans.
pub struct RocksDbStorageFindIterator<'a> {
    inner: RocksDbRawFindIterator<'a>,
}

impl<'a> RocksDbStorageFindIterator<'a> {
    pub(crate) fn new(inner: RocksDbRawFindIterator<'a>) -> Self {
        Self { inner }
    }
}

impl Iterator for RocksDbStorageFindIterator<'_> {
    type Item = (StorageKey, StorageItem);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|(key, value)| (StorageKey::from_bytes(&key), StorageItem::from_bytes(value)))
    }
}
