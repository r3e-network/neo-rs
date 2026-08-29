use super::{Block, Header, Transaction};
use crate::constants::BLOCK_MAX_TX_WIRE_LIMIT;
use crate::neo_io::serializable::helper::get_var_size_serializable_slice;
use crate::neo_io::serializable::helper::serialize_array;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::UInt256;
use std::collections::HashSet;

impl Serializable for Block {
    fn size(&self) -> usize {
        self.header.size() + get_var_size_serializable_slice(&self.transactions)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;

        const MAX_TRANSACTIONS: u64 = u16::MAX as u64;
        if self.transactions.len() as u64 > MAX_TRANSACTIONS {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        serialize_array(&self.transactions, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = <Header as Serializable>::deserialize(reader)?;

        // C# Block.DeserializeTransactions: the count is capped at ushort.MaxValue
        // and there is deliberately no byte-size cap (a >2 MiB block is valid).
        let tx_count = reader.read_var_int(BLOCK_MAX_TX_WIRE_LIMIT as u64)? as usize;

        // C# rejects duplicate transaction hashes while deserializing.
        let mut seen = HashSet::with_capacity(tx_count.min(1024));
        let mut hashes = Vec::with_capacity(tx_count.min(512));
        let mut transactions = Vec::with_capacity(tx_count.min(512));
        for _ in 0..tx_count {
            let tx = <Transaction as Serializable>::deserialize(reader)?;
            let hash = tx
                .try_hash()
                .map_err(|e| IoError::invalid_data(format!("Invalid transaction in block: {e}")))?;
            if !seen.insert(hash) {
                return Err(IoError::invalid_data(format!(
                    "Duplicate transactions on a block: {hash}"
                )));
            }
            hashes.push(hash);
            transactions.push(tx);
        }

        // C# validates the computed merkle root against the header value;
        // MerkleTree.ComputeRoot(empty) == UInt256.Zero.
        let computed_root = crate::cryptography::MerkleTree::compute_root(&hashes)
            .unwrap_or_else(UInt256::default);
        if computed_root != *header.merkle_root() {
            return Err(IoError::invalid_data(
                "The computed Merkle root does not match the expected value.",
            ));
        }

        Ok(Self {
            header,
            transactions,
        })
    }
}
