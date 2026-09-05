#![warn(missing_docs)]
//! Neo.IO - matches C# Neo.IO exactly
//! This crate provides IO functionality matching C# Neo.IO namespace

/// In-memory cache structures used by the network and storage layers.
pub mod caching;
/// Compression helpers for network payloads.
pub mod compression;
/// Extension traits for readers and writers.
pub mod extensions;
/// VM method-token payload type.
pub mod method_token;
/// Conditional witness-rule evaluation (matches C# `WitnessCondition`/`WitnessRule`).
pub mod witness_rule;

mod binary_writer;
// Core interfaces
mod memory_reader;
pub mod serializable;
pub mod var_int;

pub use binary_writer::BinaryWriter;
pub use memory_reader::{IoError, IoResult, MemoryReader};
pub use serializable::Serializable;

// Extension traits
pub use extensions::{
    binary_reader::BinaryReaderExtensions,
    binary_writer::BinaryWriterExtensions,
    memory_reader::MemoryReaderExtensions,
    serializable::{SerializableCollectionExtensions, SerializableExtensions},
};

// Re-export compression types
pub use compression::{COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD, compress_lz4, decompress_lz4};

// Re-export method token
pub use method_token::MethodToken;

// Re-export witness rule types
pub use witness_rule::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};

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
