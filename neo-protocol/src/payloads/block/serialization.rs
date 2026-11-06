use super::{Block, Header, Transaction};
use neo_crypto::MerkleTree;
use neo_io::serializable::helper::{get_var_size_serializable_slice, serialize_array};
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use std::collections::HashSet;

/// Wire transaction-count cap, distinct from runtime consensus limits.
pub const BLOCK_MAX_TX_WIRE_LIMIT: usize = u16::MAX as usize;

impl Serializable for Block {
    fn size(&self) -> usize {
        self.header.size() + get_var_size_serializable_slice(&self.transactions)
    }
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;
        if self.transactions.len() as u64 > BLOCK_MAX_TX_WIRE_LIMIT as u64 {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        serialize_array(&self.transactions, writer)
    }
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = <Header as Serializable>::deserialize(reader)?;
        // Count-only wire cap: there is deliberately no 2 MiB byte-size cap.
        let count = reader.read_var_int(BLOCK_MAX_TX_WIRE_LIMIT as u64)? as usize;
        let mut seen = HashSet::with_capacity(count.min(1024));
        let mut hashes = Vec::with_capacity(count.min(512));
        let mut transactions = Vec::with_capacity(count.min(512));
        for _ in 0..count {
            let tx = <Transaction as Serializable>::deserialize(reader)?;
            let hash = tx.try_hash().map_err(|e| {
                IoError::invalid_data(format!("Invalid transaction in block: Codec error: {e}"))
            })?;
            if !seen.insert(hash) {
                return Err(IoError::invalid_data(format!(
                    "Duplicate transactions on a block: {hash}"
                )));
            }
            hashes.push(hash);
            transactions.push(tx);
        }
        let computed_root = MerkleTree::compute_root(&hashes).unwrap_or_default();
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
