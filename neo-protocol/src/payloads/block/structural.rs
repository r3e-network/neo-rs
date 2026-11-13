// State-independent structural checks retained from the payload block.
use super::{Block, UInt256};
use neo_crypto::MerkleTree;
use neo_io::IoResult;

impl Block {
    /// Checks the transaction merkle root without snapshots or VM execution.
    pub fn verify_merkle_root(&self) -> bool {
        if self.transactions.is_empty() {
            return *self.header.merkle_root() == UInt256::default();
        }
        let hashes = match self.transaction_hashes() {
            Ok(hashes) => hashes,
            Err(error) => {
                tracing::warn!(target: "neo::block", block_index = self.header.index(), error = %error, "Failed to compute transaction hashes for merkle root");
                return false;
            }
        };
        match MerkleTree::compute_root(&hashes) {
            Some(root) => root == *self.header.merkle_root(),
            None => false,
        }
    }
    /// Checks for duplicate transaction hashes, failing closed on wire errors.
    pub fn verify_no_duplicate_transactions(&self) -> bool {
        let mut seen = std::collections::HashSet::with_capacity(self.transactions.len());
        for tx in &self.transactions {
            let hash = match tx.try_hash() {
                Ok(hash) => hash,
                Err(error) => {
                    tracing::warn!(target: "neo::block", block_index = self.header.index(), error = %error, "Failed to compute transaction hash for duplicate check");
                    return false;
                }
            };
            if !seen.insert(hash) {
                return false;
            }
        }
        true
    }
    pub(super) fn transaction_hashes(&self) -> IoResult<Vec<UInt256>> {
        self.transactions
            .iter()
            .map(super::super::transaction::Transaction::try_hash)
            .collect()
    }
}
