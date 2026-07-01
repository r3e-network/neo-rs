//! [`TransactionVerificationContext`] - per-block verification context
//! used by the mempool to track which transactions are still valid
//! after each block commit.
//!
//! Mirrors the C# `TransactionVerificationContext` semantics: when
//! the blockchain persists a block, the engine notifies the
//! mempool of every transaction in the block; the mempool prunes
//! matching items from both the verified and unverified queues.

use neo_primitives::UInt256;
use std::collections::HashSet;

/// Per-block verification context.
///
/// Holds the set of transaction hashes that the blockchain has
/// confirmed in the most recently persisted block. The mempool
/// consults this set on commit to prune already-confirmed
/// transactions.
#[derive(Debug, Default, Clone)]
pub struct TransactionVerificationContext {
    /// Transactions confirmed in the current persisting block.
    pub confirmed: HashSet<UInt256>,
    /// Transactions previously confirmed in earlier blocks but kept
    /// in the context for conflict-detection / re-verification.
    pub historic: HashSet<UInt256>,
}

impl TransactionVerificationContext {
    /// Constructs a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the union of confirmed and historic transaction hashes.
    pub fn all(&self) -> HashSet<UInt256> {
        let mut out = self.confirmed.clone();
        out.extend(self.historic.iter().copied());
        out
    }

    /// Records the given hash as confirmed in the current block.
    pub fn confirm(&mut self, hash: UInt256) -> bool {
        self.confirmed.insert(hash)
    }

    /// Promotes all currently-confirmed hashes to historic (called
    /// after a block has been persisted and a new persisting block
    /// begins).
    pub fn rotate(&mut self) {
        let drained: HashSet<UInt256> = std::mem::take(&mut self.confirmed);
        self.historic.extend(drained);
    }

    /// Returns whether the given transaction hash is known to be
    /// confirmed (current or historic).
    pub fn contains(&self, hash: &UInt256) -> bool {
        self.confirmed.contains(hash) || self.historic.contains(hash)
    }
}

#[cfg(test)]
#[path = "../tests/admission/transaction_verification_context.rs"]
mod tests;
