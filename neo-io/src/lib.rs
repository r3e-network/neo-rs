#![warn(missing_docs)]
//! Neo.IO - matches C# Neo.IO exactly
//! This crate provides IO functionality matching C# Neo.IO namespace

pub mod caching;
pub mod compression;
pub mod extensions;

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
pub use compression::{compress_lz4, decompress_lz4, COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD};

// Re-export caching types
pub use caching::{
    cache::{Cache, IoCache},
    fifo_cache::FIFOCache,
    hashset_cache::HashSetCache,
    relay_cache::{InventoryHash, RelayCache},
};
