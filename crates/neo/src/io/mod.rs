//! IO module for Neo blockchain
//!
//! This module provides IO functionality matching the C# Neo.IO namespace.

mod binary_writer;
mod error;
mod memory_reader;
mod serializable;

pub use binary_writer::BinaryWriter;
pub use error::{IoError, IoResult};
pub use memory_reader::MemoryReader;
pub use serializable::Serializable;

// Compatibility aliases matching historical naming.
pub use serializable::Serializable as ISerializable;
