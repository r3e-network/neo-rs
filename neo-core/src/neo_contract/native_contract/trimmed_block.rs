use alloc::rc::Rc;
use std::cell::RefCell;
use NeoRust::codec::VarSizeTrait;
use neo_vm::References;
use neo_vm::StackItem;
use crate::block::Header;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::native_contract::native_contract_error::NativeContractError;
use neo_type::H256;

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
}

impl SerializableTrait for TrimmedBlock {
    fn size(&self) -> usize {
        self.header.size() + self.hashes.var_size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> std::io::Result<()> {
        self.header.serialize(writer)?;
        writer.write_var_vec(&self.hashes)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> std::io::Result<Self> {
        let header = Header::deserialize(reader)?;
        let hashes = reader.read_var_vec::<H256>(u16::MAX as usize)?;
        Ok(Self { header, hashes })
    }
}

impl Default for TrimmedBlock {
    fn default() -> Self {
        todo!()
    }
}

impl IInteroperable for TrimmedBlock {
    type Error = std::io::Error;

    fn from_stack_item(_item: &StackItem) -> Result<Self, Self::Error> {
        Err(std::io::Error::new(std::io::ErrorKind::Other, "Not supported"))
    }

    fn to_stack_item(&self, reference_counter: &mut Rc<RefCell<References>>) -> Result<StackItem, Self::Error> {
        Ok(StackItem::new_array(reference_counter,
            vec![
                // Computed properties
                StackItem::ByteString(self.header.hash().to_vec()),

                // BlockBase properties
                StackItem::Integer(self.header.version().into()),
                StackItem::ByteString(self.header.prev_hash().to_vec()),
                StackItem::ByteString(self.header.merkle_root().to_vec()),
                StackItem::Integer(self.header.timestamp().into()),
                StackItem::Integer(self.header.nonce().into()),
                StackItem::Integer(self.header.index().into()),
                StackItem::Integer(self.header.primary().into()),
                StackItem::ByteString(self.header.next_consensus().to_vec()),

                // Block properties
                StackItem::Integer(self.hashes.len().into()),
            ]
        ))
    }

    fn clone(&self) -> Box<dyn IInteroperable<Error=NativeContractError>> {
        Box::new(TrimmedBlock {
            header: self.header.clone(),
            hashes: self.hashes.clone(),
        })
    }
}
