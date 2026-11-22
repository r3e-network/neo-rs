// Copyright (c) 2024 Neo-RS Project
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Reusable container utilities for reducing allocation overhead.
//!
//! This module provides specialized container types designed to eliminate heap
//! reallocations during typical transaction processing:
//! - `PooledVec`: Fixed-capacity vector that maintains allocated memory across clears
//! - `PooledHashMap`: Open-addressing hash map with pre-allocated buckets
//!
//! ## Performance Benefits
//!
//! Every transaction typically allocates:
//! - ~16 event notifications (Vec grows: 0→8→16 allocations)
//! - ~64 storage operations (HashMap insertions)
//! - Multiple temporary collections during script execution
//!
//! At 10k TPS → hundreds of thousands of allocations/sec → fragmentation + GC pressure.
//! These pooled containers maintain capacity, eliminating repeated malloc/free cycles.
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use neo_vm::memory::{PooledVec, PooledHashMap};
//!
//! // Create a pooled vector for notifications
//! let mut notifications = PooledVec::<String>::with_capacity(64);
//!
//! // First pass: allocates capacity 64
//! for i in 0..50 {
//!     notifications.push(format!("Event {}", i));
//! }
//! assert_eq!(notifications.len(), 50);
//!
//! // Second pass: ZERO allocations! Just resets length
//! notifications.clear_keep_capacity();
//! for i in 0..30 {
//!     notifications.push(format!("New Event {}", i));
//! }
//!
//! // PooledHashMap for storage operations
//! let mut storage_cache = PooledHashMap::<u64, Vec<u8>>::new(256);
//!
//! // Insert without bucket reallocations
//! storage_cache.insert_no_alloc(12345, vec![1, 2, 3]);
//! storage_cache.insert_no_alloc(67890, vec![4, 5, 6]);
//!
//! // Clear but keep all bucket slots available
//! storage_cache.clear_no_dealloc();
//! ```
//!
//! ## Design Tradeoffs
//!
//! ### PooledVec vs Vec
//!
//! - **Pro**: No reallocations up to max capacity
//! - **Con**: Cannot exceed capacity (panics instead of reallocating)
//! - **Best for**: Predictable-length collections (notifications, events)
//!
//! ### PooledHashMap vs HashMap
//!
//! - **Pro**: Fixed bucket array, no rehashing, cache-friendly layout
//! - **Con**: Lower collision tolerance (fixed size)
//! - **Best for**: Known key ranges (storage keys by prefix, syscall handlers)
//!
//! ## Thread Safety
//!
//! Both types are `!Sync` by design - intended for single-threaded transaction execution.
//! For cross-thread sharing, wrap in `Arc<Mutex<_>>` or use message passing.

use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};

/// FNV-1a hash function for fast integer hashing.
///
/// FNV-1a provides excellent distribution characteristics and is extremely fast.
/// Uses the prime offset basis 14695981039346656037 and prime multiplier 1099511628211.
///
/// # Arguments
///
/// * `value` - The u64 value to hash
///
/// # Returns
///
/// A hashed u64 value with good bit distribution
#[inline]
fn fnv1a_hash_u64(value: u64) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;

    let hash = FNV_OFFSET_BASIS ^ value;
    hash.wrapping_mul(FNV_PRIME)
}

/// Hashes an arbitrary `Hash` key to a `u64` using the standard library's
/// writer, so `PooledHashMap` works with any key type (not just integers).
#[inline]
fn hash_key<K: Hash>(key: &K) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

mod hash_map;
#[cfg(test)]
mod tests;
mod vec;

pub use hash_map::PooledHashMap;
pub use vec::PooledVec;
