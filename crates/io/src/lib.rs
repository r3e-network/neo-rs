//! Neo.IO - matches C# Neo.IO exactly
//! This crate provides IO functionality matching C# Neo.IO namespace

pub mod actors;
pub mod caching;

mod binary_writer;
// Core interfaces
mod i_serializable;
mod i_serializable_span;
mod memory_reader;
pub mod serializable;

// Re-export core types matching C# namespace structure
pub use binary_writer::BinaryWriter;
pub use i_serializable::ISerializable;
pub use i_serializable_span::ISerializableSpan;
pub use memory_reader::{IoError, IoResult, MemoryReader};
pub use serializable::Serializable;

// Re-export actors
pub use actors::idle::Idle;

// Re-export caching types
pub use caching::{
    cache::Cache,
    ec_point_cache::{ECPointCache, EncodablePoint},
    ecdsa_cache::{ECDsaCache, ECDsaCacheItem},
    fifo_cache::FIFOCache,
    hashset_cache::HashSetCache,
    indexed_queue::IndexedQueue,
    keyed_collection_slim::KeyedCollectionSlim,
    lru_cache::LRUCache,
    reflection_cache_attribute::ReflectionCacheAttribute,
    relay_cache::{InventoryHash, RelayCache},
};
