use crate::network::p2p::payloads::Header;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::collections::VecDeque;
use std::iter::FromIterator;

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

    /// Returns the first header index stored in the cache.
    pub fn first_index(&self) -> Option<u32> {
        self.read().front().map(|header| header.index())
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
        if index < first.index() {
            return None;
        }
        let offset = index.checked_sub(first.index())? as usize;
        headers.get(offset).cloned()
    }

    /// Attempts to enqueue a new header. Returns false when capacity is exceeded.
    pub fn add(&self, header: Header) -> bool {
        let mut headers = self.write();
        if headers.len() >= MAX_HEADERS {
            return false;
        }
        headers.push_back(header);
        true
    }

    /// Removes the first header when present.
    pub fn try_remove_first(&self) -> Option<Header> {
        self.write().pop_front()
    }

    /// Removes all headers with index less than or equal to `up_to_index`.
    pub fn remove_up_to(&self, up_to_index: u32) -> usize {
        let mut headers = self.write();
        let mut removed = 0;
        while let Some(front) = headers.front() {
            if front.index() > up_to_index {
                break;
            }
            headers.pop_front();
            removed += 1;
        }
        removed
    }

    fn read(&self) -> RwLockReadGuard<'_, VecDeque<Header>> {
        self.headers.read()
    }

    fn write(&self) -> RwLockWriteGuard<'_, VecDeque<Header>> {
        self.headers.write()
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

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

    fn make_header(index: u32) -> Header {
        let mut header = Header::new();
        header.set_index(index);
        header
    }

    #[test]
    fn refuses_to_add_when_full() {
        let cache = HeaderCache::new();
        for index in 0..(MAX_HEADERS as u32) {
            assert!(cache.add(make_header(index)));
        }

        assert!(cache.full());
        assert_eq!(cache.last().unwrap().index(), (MAX_HEADERS - 1) as u32);
        assert!(!cache.add(make_header(MAX_HEADERS as u32)));
        assert_eq!(cache.count(), MAX_HEADERS);
        assert_eq!(cache.last().unwrap().index(), (MAX_HEADERS - 1) as u32);
        assert!(cache.get(MAX_HEADERS as u32).is_none());
    }

    #[test]
    fn remove_up_to_discards_lower_indices() {
        let cache = HeaderCache::new();
        for index in 0..5 {
            cache.add(make_header(index));
        }

        let removed = cache.remove_up_to(2);
        assert_eq!(removed, 3);
        assert_eq!(cache.first_index(), Some(3));
    }
}

impl FromIterator<Header> for HeaderCache {
    fn from_iter<T: IntoIterator<Item = Header>>(iter: T) -> Self {
        Self {
            headers: RwLock::new(iter.into_iter().collect()),
        }
    }
}
