use neo_protocol::payloads::Header;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::collections::VecDeque;
use std::iter::FromIterator;

pub const MAX_HEADERS: usize = 50_000;

#[derive(Default)]
pub struct HeaderCache {
    headers: RwLock<VecDeque<Header>>,
}

impl HeaderCache {
    pub fn new() -> Self {
        Self {
            headers: RwLock::new(VecDeque::with_capacity(MAX_HEADERS)),
        }
    }

    #[inline]
    pub fn count(&self) -> usize {
        self.read().len()
    }

    #[inline]
    pub fn full(&self) -> bool {
        self.count() >= MAX_HEADERS
    }

    #[inline]
    pub fn last(&self) -> Option<Header> {
        self.read().back().cloned()
    }

    #[inline]
    pub fn first_index(&self) -> Option<u32> {
        self.read().front().map(|header| header.index())
    }

    pub fn iter(&self) -> HeaderCacheIter {
        let snapshot = self.read().iter().cloned().collect::<Vec<_>>();
        HeaderCacheIter {
            inner: snapshot.into_iter(),
        }
    }

    #[inline]
    pub fn get(&self, index: u32) -> Option<Header> {
        let headers = self.read();
        let first = headers.front()?;
        if index < first.index() {
            return None;
        }
        let offset = index.checked_sub(first.index())? as usize;
        headers.get(offset).cloned()
    }

    pub fn add(&self, header: Header) -> bool {
        let mut headers = self.write();
        if headers.len() >= MAX_HEADERS {
            return false;
        }
        headers.push_back(header);
        true
    }

    pub fn try_remove_first(&self) -> Option<Header> {
        self.write().pop_front()
    }

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
