//! IO module for Neo blockchain
//!
//! This module mirrors the C# Neo.IO namespace by re-exporting the shared `neo-io` crate.

mod binary_reader;

pub use binary_reader::BinaryReader;
pub use neo_io_crate::{
    serializable::{self, helper},
    BinaryWriter, IoError, IoResult, MemoryReader, Serializable,
};

// Compatibility alias matching historical naming.
pub use Serializable as ISerializable;
