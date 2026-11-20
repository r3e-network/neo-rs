//! New transaction event args implementation.
//!
//! This module provides the NewTransactionEventArgs functionality matching
//! C# Neo NewTransactionEventArgs.

use neo_protocol::payloads::Transaction;
use neo_storage::persistence::PersistenceDataCache;

/// Represents the event data of MemoryPool.NewTransaction.
pub struct NewTransactionEventArgs {
    /// The transaction being validated.
    pub transaction: Transaction,
    /// The snapshot used during validation.
    pub snapshot: PersistenceDataCache,
    /// Whether the transaction should be rejected by policy.
    pub cancel: bool,
}
