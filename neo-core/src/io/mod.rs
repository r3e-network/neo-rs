//! IO module for Neo blockchain
//!
//! This module mirrors the C# Neo.IO namespace by re-exporting the shared `neo-io` crate.

#[allow(dead_code)]
mod binary_reader;

#[allow(unused_imports)]
pub use binary_reader::BinaryReader;
#[allow(unused_imports)]
pub use neo_io_crate::{
    serializable::{self, helper},
    BinaryWriter, IoError, IoResult, MemoryReader, Serializable,
};

// Compatibility alias matching historical naming.
#[allow(unused_imports)]
pub use Serializable as ISerializable;
