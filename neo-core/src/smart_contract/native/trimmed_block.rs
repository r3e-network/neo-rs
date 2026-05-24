use crate::error::CoreError;
use crate::extensions::io::memory_reader::MemoryReaderExtensions;
use crate::ledger::{block_header::BlockHeader, Block};
use crate::neo_io::{
    serializable::helper::get_var_size, BinaryWriter, IoResult, MemoryReader, Serializable,
};
use crate::neo_vm::StackItem;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::{CoreResult, UInt256};
use neo_vm_rs::StackValue;
use num_bigint::BigInt;

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
        Self::try_from_block(block)
            .expect("block transactions must be serializable to create a trimmed block")
    }

    /// Creates a trimmed block from a full block, failing closed if any
    /// transaction hash cannot be computed.
    pub fn try_from_block(block: &Block) -> CoreResult<Self> {
        let hashes = block
            .transactions
            .iter()
            .map(|tx| tx.try_hash())
            .collect::<CoreResult<Vec<_>>>()?;
        Ok(Self::create(block.header.clone(), hashes))
    }

    /// Returns the block hash.
    pub fn hash(&self) -> UInt256 {
        self.header.hash()
    }

    /// Returns the block hash, failing closed if the header cannot be
    /// serialized.
    pub fn try_hash(&self) -> CoreResult<UInt256> {
        self.header.try_hash()
    }

    /// Returns the block index.
    pub fn index(&self) -> u32 {
        self.header.index()
    }

    /// Returns the transaction hashes.
    pub fn hashes(&self) -> &[UInt256] {
        &self.hashes
    }

    fn u64_stack_integer(value: u64) -> StackValue {
        i64::try_from(value)
            .map(StackValue::Integer)
            .unwrap_or_else(|_| StackValue::BigInteger(BigInt::from(value).to_signed_bytes_le()))
    }

    fn usize_stack_integer(value: usize) -> StackValue {
        i64::try_from(value)
            .map(StackValue::Integer)
            .unwrap_or_else(|_| StackValue::BigInteger(BigInt::from(value).to_signed_bytes_le()))
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::ByteString(self.hash().to_bytes()),
            StackValue::Integer(i64::from(self.header.version)),
            StackValue::ByteString(self.header.previous_hash.to_bytes()),
            StackValue::ByteString(self.header.merkle_root.to_bytes()),
            Self::u64_stack_integer(self.header.timestamp),
            Self::u64_stack_integer(self.header.nonce),
            StackValue::Integer(i64::from(self.header.index)),
            StackValue::Integer(i64::from(self.header.primary_index)),
            StackValue::ByteString(self.header.next_consensus.to_bytes()),
            Self::usize_stack_integer(self.hashes.len()),
        ])
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
        StackItem::try_from(self.to_stack_value()).map_err(|error| {
            CoreError::invalid_operation(format!(
                "Failed to convert TrimmedBlock StackValue to StackItem: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::BlockHeader;
    use crate::network::p2p::payloads::Transaction;
    use crate::smart_contract::IInteroperable;
    use crate::{UInt160, UInt256, Witness};
    use neo_vm_rs::OpCode;
    use neo_vm_rs::StackValue;

    fn sample_block() -> TrimmedBlock {
        let header = BlockHeader::new(
            0,
            UInt256::from_bytes(&[1u8; 32]).unwrap(),
            UInt256::from_bytes(&[2u8; 32]).unwrap(),
            1_234,
            5_678,
            9,
            1,
            UInt160::from_bytes(&[3u8; 20]).unwrap(),
            vec![Witness::empty()],
        );

        TrimmedBlock::create(
            header,
            vec![
                UInt256::from_bytes(&[4u8; 32]).unwrap(),
                UInt256::from_bytes(&[5u8; 32]).unwrap(),
            ],
        )
    }

    fn transaction_with_script(script: Vec<u8>) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_script(script);
        tx
    }

    #[test]
    fn trimmed_block_projects_to_neo_vm_rs_stack_value() {
        let block = sample_block();

        assert_eq!(
            block.to_stack_value(),
            StackValue::Array(vec![
                StackValue::ByteString(block.hash().to_bytes()),
                StackValue::Integer(0),
                StackValue::ByteString(vec![1u8; 32]),
                StackValue::ByteString(vec![2u8; 32]),
                StackValue::Integer(1_234),
                StackValue::Integer(5_678),
                StackValue::Integer(9),
                StackValue::Integer(1),
                StackValue::ByteString(vec![3u8; 20]),
                StackValue::Integer(2),
            ])
        );
    }

    #[test]
    fn trimmed_block_stack_item_projection_matches_stack_value_projection() {
        let block = sample_block();
        let expected = StackItem::try_from(block.to_stack_value()).unwrap();

        assert_eq!(block.to_stack_item().unwrap(), expected);
    }

    #[test]
    fn try_from_block_rejects_unserializable_transaction_hash() {
        let block = Block::new(
            BlockHeader::default(),
            vec![transaction_with_script(vec![
                OpCode::NOP.byte();
                u16::MAX as usize + 1
            ])],
        );

        assert!(TrimmedBlock::try_from_block(&block).is_err());
    }

    #[test]
    fn try_hash_matches_legacy_hash_for_valid_trimmed_block() {
        let block = sample_block();

        assert_eq!(block.try_hash().expect("try hash"), block.hash());
    }
}
