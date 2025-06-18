//! IO operations and data structures for the Neo blockchain.
//!
//! This crate provides IO functionality for the Neo blockchain, including binary serialization,
//! caching, and other IO-related utilities.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod binary_reader;
pub mod binary_writer;
pub mod memory_reader;
pub mod serializable;
pub mod caching;

#[cfg(feature = "async")]
pub mod actors;

// Re-exports for commonly used types
pub use binary_reader::BinaryReader;
pub use binary_writer::BinaryWriter;
pub use memory_reader::MemoryReader;
pub use serializable::Serializable;

/// Error types for IO operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Buffer overflow")]
    BufferOverflow,

    #[error("End of stream")]
    EndOfStream,

    #[error("Format exception")]
    FormatException,

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

/// Result type for IO operations
pub type Result<T> = std::result::Result<T, Error>;
