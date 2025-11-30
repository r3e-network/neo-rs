use super::binary_writer::BinaryWriterExtensions;
use crate::io::{BinaryWriter, IoResult, Serializable};

/// Extension helpers for [`Serializable`] values mirroring
/// `Neo.Extensions.IO.ISerializableExtensions`.
pub trait SerializableExtensions {
    fn to_array(&self) -> IoResult<Vec<u8>>;
}

impl<T: Serializable> SerializableExtensions for T {
    fn to_array(&self) -> IoResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        self.serialize(&mut writer)?;
        Ok(writer.into_bytes())
    }
}

/// Extensions for collections of [`Serializable`] values mirroring C# `CollectionExtensions.ToByteArray`.
pub trait SerializableCollectionExtensions<T: Serializable> {
    fn to_byte_array(&self) -> IoResult<Vec<u8>>;
}

impl<T: Serializable> SerializableCollectionExtensions<T> for [T] {
    fn to_byte_array(&self) -> IoResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        writer.write_serializable_collection(self)?;
        Ok(writer.into_bytes())
    }
}

impl<T: Serializable> SerializableCollectionExtensions<T> for Vec<T> {
    fn to_byte_array(&self) -> IoResult<Vec<u8>> {
        self.as_slice().to_byte_array()
    }
}
