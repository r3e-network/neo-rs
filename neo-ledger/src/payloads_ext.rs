//! Payload extension delegation for neo-ledger.
//!
//! This module re-exports `VerifiableExt` from `neo-core` (via validation::resolver)
//! and provides helper functions for validation. The trait implementations for Block,
//! Header, and Transaction live in `neo-core` to avoid circular dependencies.

use neo_native_contracts::LedgerContract;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::payloads::{Block, Header, Transaction};
use neo_storage::persistence::DataCache;

// Re-export the trait from neo-core (via validation::resolver)
pub use crate::validation::resolver::VerifiableExt;

/// Helper function to get script hashes for verifying a header.
pub fn header_script_hashes_for_verifying(header: &Header, snapshot: &DataCache) -> Vec<UInt160> {
    if *header.prev_hash() == UInt256::default() {
        return vec![header.witness.script_hash()];
    }
    let prev = match LedgerContract::new().get_trimmed_block(snapshot, header.prev_hash()) {
        Ok(Some(prev)) => prev,
        Ok(None) => {
            tracing::warn!(prev_hash = %header.prev_hash(), "previous header not found when verifying header");
            return Vec::new();
        }
        Err(err) => {
            tracing::warn!(prev_hash = %header.prev_hash(), error = %err, "failed to fetch previous header when verifying header");
            return Vec::new();
        }
    };
    vec![prev.header.next_consensus]
}

/// Helper function to get script hashes for verifying a block.
pub fn block_script_hashes_for_verifying(block: &Block, snapshot: &DataCache) -> Vec<UInt160> {
    header_script_hashes_for_verifying(&block.header, snapshot)
}

/// Helper function to get script hashes for verifying a transaction.
pub fn transaction_script_hashes_for_verifying(transaction: &Transaction) -> Vec<UInt160> {
    transaction.signer_accounts()
}
