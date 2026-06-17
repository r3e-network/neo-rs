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
use neo_vm::{Interoperable, InteroperableError};
use neo_vm_rs::StackValue;

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
    pub fn to_stack_value(&self) -> StackValue {
        let unsigned_integer = |value: u64| {
            StackValue::BigInteger(num_bigint::BigInt::from(value).to_signed_bytes_le())
        };

        StackValue::Array(
            0,
            vec![
                // Computed property: Header.Hash.ToArray().
                StackValue::ByteString(self.header.hash().to_bytes()),
                // BlockBase properties.
                StackValue::Integer(i64::from(self.header.version())),
                StackValue::ByteString(self.header.prev_hash().to_bytes()),
                StackValue::ByteString(self.header.merkle_root().to_bytes()),
                unsigned_integer(self.header.timestamp()),
                unsigned_integer(self.header.nonce()),
                StackValue::Integer(i64::from(self.header.index())),
                StackValue::Integer(i64::from(self.header.primary_index())),
                StackValue::ByteString(self.header.next_consensus().to_bytes()),
                // Block property: Hashes.Length (C# `int`; always non-negative and
                // bounded by MAX_TRANSACTION_HASHES).
                StackValue::Integer(self.hashes.len() as i64),
            ],
        )
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
    fn from_stack_value(&mut self, _value: StackValue) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "TrimmedBlock::from_stack_value is not supported".into(),
        ))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(self.to_stack_value())
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt160;
    use num_bigint::BigInt;

    /// Builds a header with distinctive, range-stressing field values. The nonce
    /// is `u64::MAX` (well above `i64::MAX`) to guard the unsigned projection.
    fn sample_header() -> Header {
        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(UInt256::from_bytes(&[0xA1u8; 32]).unwrap());
        header.set_merkle_root(UInt256::from_bytes(&[0xB2u8; 32]).unwrap());
        header.set_timestamp(0x0123_4567_89AB_CDEF);
        header.set_nonce(u64::MAX);
        header.set_index(123_456);
        header.set_primary_index(3);
        header.set_next_consensus(UInt160::from_bytes(&[0xC3u8; 20]).unwrap());
        header
    }

    fn sample_hashes() -> Vec<UInt256> {
        vec![
            UInt256::from_bytes(&[0x01u8; 32]).unwrap(),
            UInt256::from_bytes(&[0x02u8; 32]).unwrap(),
        ]
    }

    #[test]
    fn serialize_deserialize_round_trips() {
        let original = TrimmedBlock::new(sample_header(), sample_hashes());

        let mut writer = BinaryWriter::new();
        original.serialize(&mut writer).unwrap();
        let bytes = writer.into_bytes();

        // size() must match the number of bytes actually written.
        assert_eq!(original.size(), bytes.len());

        let mut reader = MemoryReader::new(&bytes);
        let decoded = TrimmedBlock::deserialize(&mut reader).unwrap();

        // Header has no PartialEq (interior-mutable hash cache), so compare the
        // observable fields plus the computed hash.
        assert_eq!(decoded.header.version(), 0);
        assert_eq!(decoded.header.timestamp(), 0x0123_4567_89AB_CDEF);
        assert_eq!(decoded.header.nonce(), u64::MAX);
        assert_eq!(decoded.header.index(), 123_456);
        assert_eq!(decoded.header.primary_index(), 3);
        assert_eq!(decoded.header.hash(), original.header.hash());
        assert_eq!(decoded.hashes, original.hashes);
    }

    #[test]
    fn deserialize_rejects_more_than_ushort_max_hashes() {
        // A length prefix above ushort.MaxValue (65535) must be rejected, exactly
        // like C# `ReadSerializableArray<UInt256>(ushort.MaxValue)`.
        let mut writer = BinaryWriter::new();
        <Header as Serializable>::serialize(&sample_header(), &mut writer).unwrap();
        writer.write_var_int(0x1_0000).unwrap(); // 65536
        let bytes = writer.into_bytes();

        let mut reader = MemoryReader::new(&bytes);
        assert!(TrimmedBlock::deserialize(&mut reader).is_err());
    }

    #[test]
    fn interoperable_to_stack_value_matches_inherent() {
        let header = sample_header();
        let hashes = sample_hashes();
        let block = TrimmedBlock::new(header, hashes);

        let trait_value = Interoperable::to_stack_value(&block).unwrap();
        let inherent_value = block.to_stack_value();
        assert_eq!(trait_value, inherent_value);
    }

    #[test]
    fn to_stack_value_matches_csharp_layout() {
        let header = sample_header();
        let hashes = sample_hashes();
        let block = TrimmedBlock::new(header, hashes);

        let neo_vm_rs::StackValue::Array(0, fields) = block.to_stack_value() else {
            panic!("TrimmedBlock projects to an Array");
        };
        assert_eq!(fields.len(), 10, "C# ToStackItem produces a 10-field Array");

        assert_eq!(
            fields[0],
            neo_vm_rs::StackValue::ByteString(block.header.hash().to_bytes())
        );
        assert_eq!(fields[1], neo_vm_rs::StackValue::Integer(0));
        assert_eq!(
            fields[2],
            neo_vm_rs::StackValue::ByteString(block.header.prev_hash().to_bytes())
        );
        assert_eq!(
            fields[3],
            neo_vm_rs::StackValue::ByteString(block.header.merkle_root().to_bytes())
        );
        assert_eq!(
            fields[4],
            neo_vm_rs::StackValue::BigInteger(
                BigInt::from(0x0123_4567_89AB_CDEFu64).to_signed_bytes_le()
            )
        );
        assert_eq!(
            fields[5],
            neo_vm_rs::StackValue::BigInteger(BigInt::from(u64::MAX).to_signed_bytes_le())
        );
        assert_eq!(fields[6], neo_vm_rs::StackValue::Integer(123_456));
        assert_eq!(fields[7], neo_vm_rs::StackValue::Integer(3));
        assert_eq!(
            fields[8],
            neo_vm_rs::StackValue::ByteString(block.header.next_consensus().to_bytes())
        );
        assert_eq!(fields[9], neo_vm_rs::StackValue::Integer(2));
    }

    #[test]
    fn from_stack_value_is_unsupported() {
        let mut block = TrimmedBlock::new(sample_header(), sample_hashes());
        let probe = StackValue::Integer(0);
        assert!(Interoperable::from_stack_value(&mut block, probe).is_err());
    }

    #[test]
    fn from_block_trims_transactions_to_hashes() {
        // An empty block trims to an empty hash list and preserves the header.
        let block = Block::from_parts(sample_header(), Vec::new());
        let trimmed = TrimmedBlock::from_block(&block).unwrap();
        assert!(trimmed.hashes.is_empty());
        assert_eq!(trimmed.index(), 123_456);
        assert_eq!(trimmed.hash(), block.header.hash());
    }
}
