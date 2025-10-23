use crate::network::p2p::payloads::Header;
use std::collections::VecDeque;
use std::iter::FromIterator;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Maximum number of headers retained in the cache. Matches the C# constant.
pub const MAX_HEADERS: usize = 10_000;

/// Thread-safe cache that stores headers which have arrived before their
/// corresponding blocks. Mirrors the behaviour of the C# `HeaderCache`.
#[derive(Default)]
pub struct HeaderCache {
    headers: RwLock<VecDeque<Header>>,
}

impl HeaderCache {
    pub fn new() -> Self {
        Self {
            headers: RwLock::new(VecDeque::new()),
        }
    }

    /// Returns the number of headers currently buffered.
    pub fn count(&self) -> usize {
        self.read().len()
    }

    /// Returns true when the cache reached `MAX_HEADERS` entries.
    pub fn full(&self) -> bool {
        self.count() >= MAX_HEADERS
    }

    /// Returns the latest header stored in the cache, if any.
    pub fn last(&self) -> Option<Header> {
        self.read().back().cloned()
    }

    /// Returns an iterator over a snapshot of the cached headers.
    pub fn iter(&self) -> HeaderCacheIter {
        let snapshot = self.read().iter().cloned().collect::<Vec<_>>();
        HeaderCacheIter {
            inner: snapshot.into_iter(),
        }
    }

    /// Retrieves a header by its blockchain index if present in the cache.
    pub fn get(&self, index: u32) -> Option<Header> {
        let headers = self.read();
        let first = headers.front()?;
        let offset = index.checked_sub(first.index())? as usize;
        headers.get(offset).cloned()
    }

    /// Attempts to enqueue a new header. Returns `false` when the cache is full.
    pub fn add(&self, header: Header) -> bool {
        let mut headers = self.write();
        if headers.len() >= MAX_HEADERS {
            return false;
        }
        headers.push_back(header);
        true
    }

    /// Removes and returns the oldest header, if present.
    pub fn try_remove_first(&self) -> Option<Header> {
        self.write().pop_front()
    }

    fn read(&self) -> RwLockReadGuard<VecDeque<Header>> {
        self.headers
            .read()
            .expect("header cache read lock poisoned")
    }

    fn write(&self) -> RwLockWriteGuard<VecDeque<Header>> {
        self.headers
            .write()
            .expect("header cache write lock poisoned")
    }
}

/// Iterator over a snapshot of the cached headers.
pub struct HeaderCacheIter {
    inner: std::vec::IntoIter<Header>,
}

impl Iterator for HeaderCacheIter {
    type Item = Header;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl ExactSizeIterator for HeaderCacheIter {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl FromIterator<Header> for HeaderCache {
    fn from_iter<T: IntoIterator<Item = Header>>(iter: T) -> Self {
        Self {
            headers: RwLock::new(iter.into_iter().collect()),
        }
    }
}
