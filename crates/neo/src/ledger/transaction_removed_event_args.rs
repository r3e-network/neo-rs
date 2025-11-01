//! Transaction removed event args implementation.
//!
//! This module provides the TransactionRemovedEventArgs functionality exactly matching C# Neo TransactionRemovedEventArgs.

// Matches C# using directives exactly:
// using Neo.Network.P2P.Payloads;
// using System.Collections.Generic;

use super::TransactionRemovalReason;
use crate::network::p2p::payloads::Transaction;

/// namespace Neo.Ledger -> public sealed class TransactionRemovedEventArgs

/// Represents the event data of MemoryPool.TransactionRemoved.
pub struct TransactionRemovedEventArgs {
    /// The Transactions that is being removed.
    /// public IReadOnlyCollection<Transaction> Transactions { get; init; }
    pub transactions: Vec<Transaction>,

    /// The reason a transaction was removed.
    /// public TransactionRemovalReason Reason { get; init; }
    pub reason: TransactionRemovalReason,
}
