//! Header cache implementation.
//!
//! This module provides header caching functionality exactly matching C# Neo HeaderCache.

// Matches C# using directives exactly:
// using Neo.IO.Caching;
// using Neo.Network.P2P.Payloads;
// using System;
// using System.Collections;
// using System.Collections.Generic;
// using System.Diagnostics.CodeAnalysis;
// using System.Threading;

use std::sync::RwLock;

/// namespace Neo.Ledger -> public sealed class HeaderCache : IDisposable, IEnumerable<Header>

/// Maximum number of headers that can be cached (matches C# public const int MaxHeaders = 10_000)
pub const MAX_HEADERS: usize = 10_000;

/// Used to cache the headers of the blocks that have not been received.
pub struct HeaderCache {
    // private readonly IndexedQueue<Header> _headers = new();
    headers: crate::io::caching::IndexedQueue<crate::network::p2p::payloads::Header>,

    // private readonly ReaderWriterLockSlim _readerWriterLock = new();
    reader_writer_lock: RwLock<()>,
}

impl HeaderCache {
    /// Constructor (implicit in C#)
    pub fn new() -> Self {
        Self {
            headers: crate::io::caching::IndexedQueue::new(),
            reader_writer_lock: RwLock::new(()),
        }
    }

    /// Gets the Header at the specified index in the cache.
    /// public Header? this[uint index]
    pub fn get(&self, index: u32) -> Option<crate::network::p2p::payloads::Header> {
        let _guard = self.reader_writer_lock.read().unwrap();

        if self.headers.count() == 0 {
            return None;
        }

        let first_index = self.headers.get(0)?.index;
        if index < first_index {
            return None;
        }

        let index = index - first_index;
        if index >= self.headers.count() as u32 {
            return None;
        }

        self.headers.get(index as usize).cloned()
    }

    /// Gets the number of elements in the cache.
    /// public int Count
    pub fn count(&self) -> usize {
        let _guard = self.reader_writer_lock.read().unwrap();
        self.headers.count()
    }

    /// Indicates whether the cache is full.
    /// public bool Full => Count >= MaxHeaders;
    pub fn full(&self) -> bool {
        self.count() >= MAX_HEADERS
    }

    /// Gets the last Header in the cache. Or null if the cache is empty.
    /// public Header? Last
    pub fn last(&self) -> Option<crate::network::p2p::payloads::Header> {
        let _guard = self.reader_writer_lock.read().unwrap();

        if self.headers.count() == 0 {
            return None;
        }

        self.headers.last().cloned()
    }

    /// internal bool Add(Header header)
    pub(crate) fn add(&mut self, header: crate::network::p2p::payloads::Header) -> bool {
        let _guard = self.reader_writer_lock.write().unwrap();

        // Enforce the cache limit when Full
        if self.headers.count() >= MAX_HEADERS {
            return false;
        }

        self.headers.enqueue(header);
        true
    }

    /// internal bool TryRemoveFirst([NotNullWhen(true)] out Header? header)
    pub(crate) fn try_remove_first(&mut self) -> Option<crate::network::p2p::payloads::Header> {
        let _guard = self.reader_writer_lock.write().unwrap();
        self.headers.try_dequeue()
    }

    /// GetEnumerator implementation
    pub fn iter(&self) -> impl Iterator<Item = crate::network::p2p::payloads::Header> + '_ {
        let _guard = self.reader_writer_lock.read().unwrap();
        self.headers.iter().cloned().collect::<Vec<_>>().into_iter()
    }
}

// IDisposable implementation - in Rust this would be Drop trait
impl Drop for HeaderCache {
    fn drop(&mut self) {
        // In C#: _readerWriterLock.Dispose();
        // In Rust, RwLock is automatically cleaned up
    }
}

// IEnumerable<Header> implementation
impl IntoIterator for HeaderCache {
    type Item = crate::network::p2p::payloads::Header;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let _guard = self.reader_writer_lock.read().unwrap();
        self.headers.into_iter().collect::<Vec<_>>().into_iter()
    }
}
