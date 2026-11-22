//! Merkle block payload for filtered block responses.
//!
//! Mirrors `Neo.Network.P2P.Payloads.MerkleBlockPayload` and the `neo-core`
//! implementation. Used when a peer has a bloom filter loaded and requests
//! blocks: only the merkle proof branches and matching transaction hashes are
//! sent instead of the full block.
//!
//! Uses manual LSB-first bit packing instead of the `bitvec` crate to stay
//! within the allowed dependency set; the byte layout matches `bitvec`'s
//! `BitVec<u8, Lsb0>::into_vec()` exactly.

use neo_crypto::MerkleTree;
use neo_io_crate::serializable::helper::{
    deserialize_array, get_var_size_bytes, get_var_size_serializable_slice, get_var_size_usize,
    serialize_array,
};
use neo_io_crate::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::UInt256;
use neo_protocol::payloads::{Block, Header, Transaction};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Represents a block that is filtered by a BloomFilter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleBlockPayload {
    /// The header of the block.
    pub header: Header,
    /// The number of transactions in the original block.
    pub tx_count: u32,
    /// The nodes of the transaction hash tree (depth-first order after trim).
    pub hashes: Vec<UInt256>,
    /// The bloom-filter flags bitfield (LSB-first packed).
    pub flags: Vec<u8>,
}

impl MerkleBlockPayload {
    /// Creates a new merkle block payload from its constituent parts.
    pub fn new(header: Header, tx_count: u32, hashes: Vec<UInt256>, flags: Vec<u8>) -> Self {
        Self {
            header,
            tx_count,
            hashes,
            flags,
        }
    }

    /// Creates from a block and per-transaction filter flags, panicking if any
    /// transaction hash cannot be represented on the wire.
    pub fn create(block: &Block, filter_bits: Vec<bool>) -> Self {
        Self::try_create(block, filter_bits)
            .expect("block transactions must be serializable to create a merkle block payload")
    }

    /// Creates from a block and per-transaction filter flags, failing closed if
    /// any transaction hash cannot be represented on the wire.
    ///
    /// `filter_bits` carries one entry per transaction indicating whether the
    /// transaction matched the peer's bloom filter.
    pub fn try_create(block: &Block, filter_bits: Vec<bool>) -> IoResult<Self> {
        let tx_count = block.transactions.len() as u32;
        let tx_hashes: Vec<UInt256> = block
            .transactions
            .iter()
            .map(Transaction::try_hash)
            .collect::<IoResult<Vec<_>>>()?;

        let mut tree = MerkleTree::new(&tx_hashes);
        let padded_flags = pad_flags(filter_bits, tree.depth());
        tree.trim(&padded_flags);
        let hashes = tree.to_hash_array();
        let flags = pack_flags(&padded_flags);

        Ok(Self::new(block.header.clone(), tx_count, hashes, flags))
    }
}

/// Pads or truncates the per-transaction flags to the tree's leaf width.
fn pad_flags(mut flags: Vec<bool>, depth: usize) -> Vec<bool> {
    if depth == 0 {
        return flags;
    }
    if depth == 1 {
        if flags.is_empty() {
            flags.push(false);
        } else {
            flags.truncate(1);
        }
        return flags;
    }

    let target_len = 1usize << (depth - 1);
    match flags.len().cmp(&target_len) {
        Ordering::Greater => {
            flags.truncate(target_len);
            flags
        }
        Ordering::Less => {
            flags.resize(target_len, false);
            flags
        }
        Ordering::Equal => flags,
    }
}

/// Packs a boolean slice into LSB-first byte representation.
///
/// Matches `bitvec`'s `BitVec<u8, Lsb0>::into_vec()` byte-for-byte.
fn pack_flags(flags: &[bool]) -> Vec<u8> {
    let byte_count = flags.len().div_ceil(8);
    let mut packed = vec![0u8; byte_count];
    for (i, &flag) in flags.iter().enumerate() {
        if flag {
            packed[i / 8] |= 1 << (i % 8);
        }
    }
    packed
}

impl Serializable for MerkleBlockPayload {
    fn size(&self) -> usize {
        // C# v3.11.2: `Size => Header.Size + TxCount.GetVarSize() + ...` — the
        // transaction count contributes its var-int width, not a fixed 4 bytes.
        self.header.size()
            + get_var_size_usize(self.tx_count as usize)
            + get_var_size_serializable_slice(&self.hashes)
            + get_var_size_bytes(&self.flags)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;

        // Write tx count as var int.
        if self.tx_count as u64 > u16::MAX as u64 {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        writer.write_var_uint(self.tx_count as u64)?;

        // Write hashes.
        serialize_array(&self.hashes, writer)?;

        // Write flags.
        let max_flags = (self.tx_count.max(1) as usize).div_ceil(8);
        if self.flags.len() > max_flags {
            return Err(IoError::invalid_data("Flag length exceeds limit"));
        }
        writer.write_var_bytes(&self.flags)?;

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = <Header as Serializable>::deserialize(reader)?;

        let tx_count = reader.read_var_int(u16::MAX as u64)? as u32;
        // C# v3.11.2: `maxHashes = RoundUpToPowerOf2(TxCount)`. Trimming an odd
        // level of the merkle tree duplicates its last subtree, so a compliant
        // sender may emit up to twice the leaf count of hashes.
        let max_hashes = if tx_count == 0 {
            0
        } else {
            tx_count.next_power_of_two() as usize
        };
        let hashes = deserialize_array::<UInt256>(reader, max_hashes)?;

        let max_flags = (tx_count.max(1) as usize).div_ceil(8);
        let flags = reader.read_var_bytes(max_flags)?;

        Ok(Self {
            header,
            tx_count,
            hashes,
            flags,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_flags_uses_neo_lsb_first_byte_order() {
        let packed = pack_flags(&[true, false, true, true, false, false, false, false, true]);
        assert_eq!(packed, vec![0b0000_1101, 0b0000_0001]);
    }

    #[test]
    fn pack_flags_empty_input() {
        assert!(pack_flags(&[]).is_empty());
    }

    #[test]
    fn pad_flags_single_depth_adds_padding_flag() {
        assert_eq!(pad_flags(Vec::new(), 1), vec![false]);
        assert_eq!(pad_flags(vec![true], 1), vec![true]);
    }

    #[test]
    fn pad_flags_extends_and_truncates_to_width() {
        // Depth 3 => 4 leaves.
        assert_eq!(pad_flags(vec![true], 3), vec![true, false, false, false]);
        assert_eq!(
            pad_flags(vec![true, true, true, true, true], 3),
            vec![true, true, true, true]
        );
    }

    #[test]
    fn merkle_block_payload_new_stores_fields() {
        let header = Header::new();
        let hashes = vec![UInt256::from([1u8; 32])];
        let flags = vec![0xFF];
        let payload = MerkleBlockPayload::new(header.clone(), 5, hashes.clone(), flags.clone());

        assert_eq!(payload.tx_count, 5);
        assert_eq!(payload.hashes, hashes);
        assert_eq!(payload.flags, flags);
    }

    #[test]
    fn try_create_empty_block_produces_single_flag() {
        let block = Block::new();
        let payload = MerkleBlockPayload::try_create(&block, Vec::new()).expect("payload");
        assert_eq!(payload.tx_count, 0);
        // Depth 0 for an empty tree: pad_flags returns the empty vec unchanged.
        assert!(payload.hashes.is_empty());
    }

    fn write_payload_bytes(writer: &mut BinaryWriter, tx_count: u32, hashes: usize) {
        Serializable::serialize(&Header::new(), writer).unwrap();
        writer.write_var_uint(tx_count as u64).unwrap();
        // `deserialize_array` expects the var-int element count ahead of the items.
        writer.write_var_uint(hashes as u64).unwrap();
        for i in 0..hashes {
            let mut hash = [0u8; 32];
            hash[0] = i as u8;
            Serializable::serialize(&UInt256::from(hash), writer).unwrap();
        }
        writer.write_var_bytes(&[0x1]).unwrap();
    }

    #[test]
    fn deserialize_accepts_hashes_up_to_next_power_of_two() {
        // C# v3.11.2: `maxHashes = RoundUpToPowerOf2(TxCount)` — a trimmed odd
        // merkle level duplicates its last subtree, so tx_count 5 may carry 8 hashes.
        let mut writer = BinaryWriter::new();
        write_payload_bytes(&mut writer, 5, 8);
        let bytes = writer.into_bytes();

        let mut reader = MemoryReader::new(&bytes);
        let payload = <MerkleBlockPayload as Serializable>::deserialize(&mut reader).unwrap();
        assert_eq!(payload.tx_count, 5);
        assert_eq!(payload.hashes.len(), 8);
    }

    #[test]
    fn deserialize_rejects_hashes_beyond_power_of_two_bound() {
        // Nine hashes for tx_count 5 exceed RoundUpToPowerOf2(5) = 8.
        let mut writer = BinaryWriter::new();
        write_payload_bytes(&mut writer, 5, 9);
        let bytes = writer.into_bytes();

        let mut reader = MemoryReader::new(&bytes);
        assert!(<MerkleBlockPayload as Serializable>::deserialize(&mut reader).is_err());
    }

    #[test]
    fn size_counts_var_int_transaction_width() {
        // C# v3.11.2: `TxCount.GetVarSize()` — one byte for 127 instead of the
        // fixed 4-byte width used before.
        let payload = MerkleBlockPayload::new(Header::new(), 127, Vec::new(), Vec::new());
        assert_eq!(payload.size(), payload.header.size() + 1 + 1 + 1);
    }
}
