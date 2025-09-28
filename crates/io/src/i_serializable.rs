//! ISerializable interface - matches C# Neo.IO.ISerializable exactly

use crate::{IoResult, MemoryReader};
use std::io::Write;

/// Represents NEO objects that can be serialized (matches C# ISerializable)
pub trait ISerializable {
    /// The size of the object in bytes after serialization.
    fn size(&self) -> usize;

    /// Serializes the object using the specified writer (C# uses BinaryWriter).
    fn serialize<W: Write>(&self, writer: &mut W) -> IoResult<()>;

    /// Deserializes the object using the specified MemoryReader.
    fn deserialize(&mut self, reader: &mut MemoryReader) -> IoResult<()>;
}
