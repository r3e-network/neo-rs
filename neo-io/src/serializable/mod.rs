//! Serializable trait and helpers mirroring C# Neo.IO serialization contracts.

use crate::{binary_writer::BinaryWriter, IoResult, MemoryReader};

pub mod helper;
pub mod primitives;

/// Trait implemented by Neo types that can be serialized and deserialized.
///
/// This follows the behaviour of `Neo.IO.ISerializable` from the C# codebase.
pub trait Serializable: Sized {
    /// Creates an instance from the provided `MemoryReader`.
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self>;

    /// Serializes the current value into the provided `BinaryWriter`.
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()>;

    /// Returns the number of bytes the serialized value will consume.
    fn size(&self) -> usize;
}
