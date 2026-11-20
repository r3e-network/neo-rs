#![warn(missing_docs)]
//! Neo.IO - matches C# Neo.IO exactly
//! This crate provides IO functionality matching C# Neo.IO namespace

/// In-memory cache structures used by the network and storage layers.
pub mod caching;
/// Owned errors for general serialization and compression codecs.
pub mod codec_error;
/// Compression helpers for network payloads.
pub mod compression;
/// Extension traits for readers and writers.
pub mod extensions;
/// Conditional witness-rule evaluation (matches C# `WitnessCondition`/`WitnessRule`).
pub mod witness_rule;

mod binary_writer;
// Core interfaces
mod memory_reader;
pub mod serializable;
/// General binary, JSON, and Neo-native serialization codecs.
pub mod serialization;
pub mod var_int;

pub use binary_writer::BinaryWriter;
pub use memory_reader::{IoError, IoResult, MemoryReader};
pub use serializable::Serializable;

// Extension traits
pub use extensions::{
    binary_reader::BinaryReaderExtensions,
    binary_writer::BinaryWriterExtensions,
    byte::ByteLz4Extensions,
    memory::ReadOnlyMemoryExtensions,
    memory_reader::MemoryReaderExtensions,
    serializable::{SerializableCollectionExtensions, SerializableExtensions},
    span::SpanExtensions,
    validation::{OptionExt, ValidateLength},
};

// Re-export compression types
pub use compression::{
    COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD, DEFAULT_MAX_DECOMPRESSED_SIZE, compress_lz4,
    decompress_lz4,
};

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
