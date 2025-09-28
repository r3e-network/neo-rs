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
