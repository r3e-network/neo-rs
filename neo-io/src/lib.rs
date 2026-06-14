//! Neo.IO - matches C# Neo.IO exactly
//! This crate provides IO functionality matching C# Neo.IO namespace

pub mod caching;
pub mod compression;
/// Extension traits that mirror the C# `Neo.Extensions.IO` helpers.
pub mod extensions;

mod binary_writer;
/// Generic derive-style macros and IO helper traits (OptionExt, ValidateLength).
/// Relocated from neo-core so layered crates (e.g. neo-p2p chain types) can use
/// them without depending on neo-core.
#[macro_use]
pub mod macros;
// Core interfaces
mod memory_reader;
pub mod serializable;
pub mod var_int;

pub use binary_writer::BinaryWriter;
pub use macros::{OptionExt, ValidateLength};
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
pub use compression::{COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD, Lz4};

// Re-export caching types
pub use caching::{
    cache::{Cache, IoCache},
    fifo_cache::FIFOCache,
    hashset_cache::HashSetCache,
    relay_cache::{InventoryHash, RelayCache},
};
