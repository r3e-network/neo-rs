//! Trimmed block — a block whose transactions are reduced to their hashes.
//!
//! Mirrors C# `Neo.SmartContract.Native.TrimmedBlock`. The native
//! `LedgerContract` persists every block in this trimmed form (the full header
//! plus the ordered list of its transaction hashes) under its `Prefix_Block`
//! storage, and projects it to a VM `Array` for `getBlock` /
//! `getTransactionFromBlock`. This is the canonical type owned by the payloads
//! layer (alongside [`Block`](crate::Block) and [`Header`](crate::Header)) and
//! consumed by the ledger storage layer and the LedgerContract read methods.

use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use neo_primitives::UInt256;
use neo_vm::{Interoperable, InteroperableError, StackItem};

use crate::block::Block;
use crate::header::Header;

/// Upper bound on the number of transaction hashes a trimmed block may carry,
/// matching C# `reader.ReadSerializableArray<UInt256>(ushort.MaxValue)`.
const MAX_TRANSACTION_HASHES: u64 = u16::MAX as u64;

/// A block whose transactions have been trimmed to just their hashes
/// (C# `TrimmedBlock`).
#[derive(Debug, Clone)]
pub struct TrimmedBlock {
    /// The header of the block.
    pub header: Header,
    /// The hashes of the block's transactions, in block order.
    pub hashes: Vec<UInt256>,
}

impl TrimmedBlock {
    /// Creates a trimmed block from a header and its transaction hashes
    /// (C# `TrimmedBlock.Create(Header, UInt256[])`).
    pub fn new(header: Header, hashes: Vec<UInt256>) -> Self {
        Self { header, hashes }
    }

    /// Creates a trimmed block from a full block (C# `TrimmedBlock.Create(Block)`):
    /// keeps the header and replaces each transaction with its hash, propagating
    /// any transaction-hash serialization failure.
    pub fn from_block(block: &Block) -> neo_error::CoreResult<Self> {
        Ok(Self {
            header: block.header.clone(),
            hashes: block.transaction_hashes()?,
        })
    }

    /// The hash of the block (C# `TrimmedBlock.Hash => Header.Hash`).
    pub fn hash(&self) -> UInt256 {
        self.header.hash()
    }

    /// The index (height) of the block (C# `TrimmedBlock.Index => Header.Index`).
    pub fn index(&self) -> u32 {
        self.header.index()
    }

    /// Projects the trimmed block to the VM `Array` produced by C#
    /// `TrimmedBlock.ToStackItem`, field-for-field.
    ///
    /// Header `Timestamp` and `Nonce` are `ulong` in C# and become non-negative
    /// `BigInteger`s in the VM; they are projected here through `u64 -> BigInt`
    /// so the full unsigned range is preserved (never truncated through `i64`),
    /// which is consensus-relevant — a nonce `>= 2^63` would otherwise serialize
    /// as a different integer.
    pub fn to_stack_item(&self) -> StackItem {
        let unsigned_integer = |value: u64| StackItem::from_int(num_bigint::BigInt::from(value));

        StackItem::from_array(vec![
            // Computed property: Header.Hash.ToArray().
            StackItem::from_byte_string(self.header.hash().to_bytes()),
            // BlockBase properties.
            StackItem::from_i64(i64::from(self.header.version())),
            StackItem::from_byte_string(self.header.prev_hash().to_bytes()),
            StackItem::from_byte_string(self.header.merkle_root().to_bytes()),
            unsigned_integer(self.header.timestamp()),
            unsigned_integer(self.header.nonce()),
            StackItem::from_i64(i64::from(self.header.index())),
            StackItem::from_i64(i64::from(self.header.primary_index())),
            StackItem::from_byte_string(self.header.next_consensus().to_bytes()),
            // Block property: Hashes.Length (C# `int`; always non-negative and
            // bounded by MAX_TRANSACTION_HASHES).
            StackItem::from_i64(self.hashes.len() as i64),
        ])
    }
}

impl Serializable for TrimmedBlock {
    fn size(&self) -> usize {
        // C# `Size => Header.Size + Hashes.GetVarSize()`: the header plus the
        // var-int length prefix and the fixed-size hash elements.
        <Header as Serializable>::size(&self.header)
            + neo_io::serializable::helper::SerializeHelper::get_var_size_serializable_slice(
                &self.hashes,
            )
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Header as Serializable>::serialize(&self.header, writer)?;
        writer.write_var_int(self.hashes.len() as u64)?;
        for hash in &self.hashes {
            <UInt256 as Serializable>::serialize(hash, writer)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = <Header as Serializable>::deserialize(reader)?;
        let count = reader.read_var_int(MAX_TRANSACTION_HASHES)? as usize;
        let mut hashes = Vec::with_capacity(count);
        for _ in 0..count {
            hashes.push(<UInt256 as Serializable>::deserialize(reader)?);
        }
        Ok(Self { header, hashes })
    }
}

impl Interoperable for TrimmedBlock {
    fn from_stack_item(&mut self, _value: StackItem) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "TrimmedBlock::from_stack_item is not supported".into(),
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        Ok(TrimmedBlock::to_stack_item(self))
    }
}

#[cfg(test)]
#[path = "../tests/ledger/trimmed_block.rs"]
mod tests;
