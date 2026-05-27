//! IO module for Neo blockchain
//!
//! This module mirrors the C# Neo.IO namespace by re-exporting the shared `neo-io` crate.

#[allow(unused_imports)]
pub use neo_io_crate::{
    serializable::{self, helper},
    BinaryWriter, IoError, IoResult, MemoryReader, Serializable,
};
