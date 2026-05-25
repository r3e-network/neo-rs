#![warn(missing_docs)]
//! Neo.IO - matches C# Neo.IO exactly
//! This crate provides IO functionality matching C# Neo.IO namespace

pub mod caching;

mod binary_writer;
// Core interfaces
mod memory_reader;
pub mod serializable;

pub use binary_writer::BinaryWriter;
pub use memory_reader::{IoError, IoResult, MemoryReader};
pub use serializable::Serializable;

// Re-export caching types
pub use caching::{
    cache::{Cache, IoCache},
    ec_point_cache::{ECPointCache, EncodablePoint},
    ecdsa_cache::{ECDsaCache, ECDsaCacheItem},
    fifo_cache::FIFOCache,
    hashset_cache::HashSetCache,
    lru_cache::LRUCache,
    relay_cache::{InventoryHash, RelayCache},
};
