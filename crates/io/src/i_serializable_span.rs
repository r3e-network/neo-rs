//! ISerializableSpan interface - matches C# Neo.IO.ISerializableSpan exactly

use crate::{IoError, IoResult};

/// Represents NEO objects that can be serialized (matches C# ISerializableSpan)
pub trait ISerializableSpan {
    /// The size of the object in bytes after serialization.
    fn size(&self) -> usize;

    /// Gets a slice that represents the current value.
    /// Requires keeping the data returned by get_span consistent with ISerializable.
    fn get_span(&self) -> &[u8];

    /// Serializes the object using the specified destination slice.
    fn serialize(&self, destination: &mut [u8]) -> IoResult<()> {
        let source = self.get_span();
        if destination.len() < source.len() {
            return Err(IoError::Format);
        }
        destination[..source.len()].copy_from_slice(source);
        Ok(())
    }
}
