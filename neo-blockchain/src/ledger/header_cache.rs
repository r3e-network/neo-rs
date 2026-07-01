//! In-memory header cache used by the blockchain service.
//!
//! The service is the single owner of the canonical tip, so this cache lives
//! on the service state rather than in a process-wide singleton.

use neo_payloads::Header;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::collections::VecDeque;

/// Maximum number of headers retained in the cache (C# `HeaderCache.MaxHeaders`).
pub const MAX_HEADERS: usize = 10_000;

/// Thread-safe cache that stores headers which have arrived before
/// their corresponding blocks. Mirrors the behaviour of the C#
/// `HeaderCache`.
#[derive(Default)]
pub struct HeaderCache {
    headers: RwLock<VecDeque<Header>>,
}

impl HeaderCache {
    /// Construct an empty cache with the default capacity.
    pub fn new() -> Self {
        Self {
            headers: RwLock::new(VecDeque::with_capacity(MAX_HEADERS)),
        }
    }

    /// Returns the number of headers currently buffered.
    #[inline]
    pub fn count(&self) -> usize {
        self.read().len()
    }

    /// Returns `true` when the cache reached `MAX_HEADERS` entries.
    #[inline]
    pub fn full(&self) -> bool {
        self.count() >= MAX_HEADERS
    }

    /// Returns the latest header stored in the cache, if any.
    #[inline]
    pub fn last(&self) -> Option<Header> {
        self.read().back().cloned()
    }

    /// Returns the first header index stored in the cache.
    #[inline]
    pub fn first_index(&self) -> Option<u32> {
        self.read().front().map(|header| header.index())
    }

    /// Look up a header by its index.
    pub fn get(&self, index: u32) -> Option<Header> {
        self.read().iter().find(|h| h.index() == index).cloned()
    }

    /// Append a header to the cache after the current tip. Returns
    /// `false` when the cache was full and the header was rejected.
    pub fn add(&self, header: Header) -> bool {
        let mut guard = self.write();
        if guard.len() >= MAX_HEADERS {
            return false;
        }
        guard.push_back(header);
        true
    }

    /// Removes every header whose index is `<= up_to_index`.
    pub fn remove_up_to(&self, up_to_index: u32) -> usize {
        let mut guard = self.write();
        let before = guard.len();
        while let Some(front) = guard.front() {
            if front.index() <= up_to_index {
                guard.pop_front();
            } else {
                break;
            }
        }
        before - guard.len()
    }

    #[inline]
    fn read(&self) -> RwLockReadGuard<'_, VecDeque<Header>> {
        self.headers.read()
    }

    #[inline]
    fn write(&self) -> RwLockWriteGuard<'_, VecDeque<Header>> {
        self.headers.write()
    }
}

#[cfg(test)]
#[path = "../tests/ledger/header_cache.rs"]
mod tests;
