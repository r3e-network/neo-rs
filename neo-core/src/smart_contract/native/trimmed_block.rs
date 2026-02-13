use crate::error::CoreError;
use crate::extensions::io::memory_reader::MemoryReaderExtensions;
use crate::ledger::{block_header::BlockHeader, Block};
use crate::neo_io::{
    serializable::helper::get_var_size, BinaryWriter, IoResult, MemoryReader, Serializable,
};
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::UInt256;
use neo_vm::StackItem;

/// A trimmed block containing only the header and transaction hashes (matches C# TrimmedBlock)
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TrimmedBlock {
    pub header: BlockHeader,
    pub hashes: Vec<UInt256>,
}

impl TrimmedBlock {
    /// Creates a trimmed block from a block header and list of transaction hashes.
    pub fn create(header: BlockHeader, hashes: Vec<UInt256>) -> Self {
        Self { header, hashes }
    }

    /// Creates a trimmed block from a full block.
    pub fn from_block(block: &Block) -> Self {
        Self::create(
            block.header.clone(),
            block.transactions.iter().map(|tx| tx.hash()).collect(),
        )
    }

    /// Returns the block hash.
    pub fn hash(&self) -> UInt256 {
        self.header.hash()
    }

    /// Returns the block index.
    pub fn index(&self) -> u32 {
        self.header.index()
    }

    /// Returns the transaction hashes.
    pub fn hashes(&self) -> &[UInt256] {
        &self.hashes
    }
}

impl Serializable for TrimmedBlock {
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;
        writer.write_serializable_vec(&self.hashes)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = BlockHeader::deserialize(reader)?;
        let hashes = reader.read_serializable_array::<UInt256>(u16::MAX as usize)?;
        Ok(Self { header, hashes })
    }

    fn size(&self) -> usize {
        self.header.size()
            + get_var_size(self.hashes.len() as u64)
            + self.hashes.iter().map(|hash| hash.size()).sum::<usize>()
    }
}

impl IInteroperable for TrimmedBlock {
    fn from_stack_item(&mut self, _stack_item: StackItem) -> Result<(), CoreError> {
        // Not supported in C# implementation (throws NotSupportedException)
        Err(CoreError::invalid_operation(
            "FromStackItem is not supported for TrimmedBlock",
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        Ok(StackItem::from_array(vec![
            StackItem::from_byte_string(self.hash().to_bytes()),
            StackItem::from_int(self.header.version),
            StackItem::from_byte_string(self.header.previous_hash.to_bytes()),
            StackItem::from_byte_string(self.header.merkle_root.to_bytes()),
            StackItem::from_int(self.header.timestamp),
            StackItem::from_int(self.header.nonce),
            StackItem::from_int(self.header.index),
            StackItem::from_int(self.header.primary_index as u32),
            StackItem::from_byte_string(self.header.next_consensus.to_bytes()),
            StackItem::from_int(self.hashes.len() as u32),
        ]))
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}
