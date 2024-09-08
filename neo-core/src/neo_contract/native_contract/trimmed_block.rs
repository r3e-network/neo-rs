use neo_vm::stack_item::StackItem;
use crate::block::Header;
/// Represents a block which the transactions are trimmed.
#[derive(Clone)]
pub struct TrimmedBlock {
    /// The header of the block.
    pub header: Header,

    /// The hashes of the transactions of the block.
    pub hashes: Vec<H256>,
}

impl TrimmedBlock {
    /// The hash of the block.
    pub fn hash(&self) -> H256 {
        self.header.hash()
    }

    /// The index of the block.
    pub fn index(&self) -> u32 {
        self.header.index()
    }

    pub fn size(&self) -> usize {
        self.header.size() + self.hashes.var_size()
    }
}

impl ISerializable for TrimmedBlock {
    fn deserialize(reader: &mut Reader) -> Result<Self, Error> {
        let header = Header::deserialize(reader)?;
        let hashes = reader.read_var_vec::<H256>(u16::MAX as usize)?;
        Ok(Self { header, hashes })
    }

    fn serialize(&self, writer: &mut Writer) -> Result<(), Error> {
        self.header.serialize(writer)?;
        writer.write_var_vec(&self.hashes)?;
        Ok(())
    }
}

impl IInteroperable for TrimmedBlock {
    fn from_interface_object(_object: InterfaceObject) -> Result<Self, Error> {
        Err(Error::NotSupported)
    }

    fn to_interface_object(&self) -> InterfaceObject {
        let mut array = Vec::with_capacity(10);
        array.push(StackItem::ByteString(self.header.hash().to_vec()));
        array.push(StackItem::Integer(self.header.version().into()));
        array.push(StackItem::ByteString(self.header.prev_hash().to_vec()));
        array.push(StackItem::ByteString(self.header.merkle_root().to_vec()));
        array.push(StackItem::Integer(self.header.timestamp().into()));
        array.push(StackItem::Integer(self.header.nonce().into()));
        array.push(StackItem::Integer(self.header.index().into()));
        array.push(StackItem::Integer(self.header.primary_index().into()));
        array.push(StackItem::ByteString(self.header.next_consensus().to_vec()));
        array.push(StackItem::Integer(self.hashes.len().into()));
        InterfaceObject::Array(array)
    }
}
