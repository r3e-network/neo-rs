use super::{BinaryWriter, IoResult, MemoryReader};

pub trait Serializable: Sized {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self>;
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()>;
    fn size(&self) -> usize;
}
