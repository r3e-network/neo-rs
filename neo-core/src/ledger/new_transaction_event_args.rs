//! New transaction event args implementation.
//!
//! This module provides the NewTransactionEventArgs functionality matching
//! C# Neo NewTransactionEventArgs.

use crate::network::p2p::payloads::Transaction;
use crate::persistence::DataCache;

/// Represents the event data of MemoryPool.NewTransaction.
pub struct NewTransactionEventArgs {
    /// The transaction being validated.
    pub transaction: Transaction,
    /// The snapshot used during validation.
    pub snapshot: DataCache,
    /// Whether the transaction should be rejected by policy.
    pub cancel: bool,
}
