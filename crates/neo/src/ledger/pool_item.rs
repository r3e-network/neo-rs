//! Pool item implementation.
//!
//! This module provides the PoolItem functionality exactly matching C# Neo PoolItem.

// Matches C# using directives exactly:
// using Neo.Network.P2P.Payloads;
// using System;

use crate::network::p2p::payloads::Transaction;
use std::cmp::Ordering;
use std::time::SystemTime;

/// namespace Neo.Ledger -> internal class PoolItem : IComparable<PoolItem>

/// Represents an item in the Memory Pool.
///
/// Note: PoolItem objects don't consider transaction priority (low or high) in their compare CompareTo method.
///       This is because items of differing priority are never added to the same sorted set in MemoryPool.
#[derive(Clone)]
pub struct PoolItem {
    /// Internal transaction for PoolItem
    /// public Transaction Tx { get; }
    pub transaction: Transaction,

    /// Timestamp when transaction was stored on PoolItem
    /// public DateTime Timestamp { get; }
    pub timestamp: SystemTime,

    /// Timestamp when this transaction was last broadcast to other nodes
    /// public DateTime LastBroadcastTimestamp { get; set; }
    pub last_broadcast_timestamp: SystemTime,
}

impl PoolItem {
    /// internal PoolItem(Transaction tx)
    pub(crate) fn new(tx: Transaction) -> Self {
        let now = SystemTime::now();
        Self {
            transaction: tx,
            // Timestamp = TimeProvider.Current.UtcNow;
            timestamp: now,
            // LastBroadcastTimestamp = Timestamp;
            last_broadcast_timestamp: now,
        }
    }

    /// public int CompareTo(Transaction otherTx)
    pub fn compare_to_transaction(&self, other_tx: &Transaction) -> Ordering {
        // var ret = (Tx.GetAttribute<HighPriorityAttribute>() != null)
        //     .CompareTo(otherTx.GetAttribute<HighPriorityAttribute>() != null);
        let self_has_high_priority = self
            .transaction
            .get_attribute::<crate::transaction::HighPriorityAttribute>()
            .is_some();
        let other_has_high_priority = other_tx
            .get_attribute::<crate::transaction::HighPriorityAttribute>()
            .is_some();

        let ret = self_has_high_priority.cmp(&other_has_high_priority);
        if ret != Ordering::Equal {
            return ret;
        }

        // Fees sorted ascending
        // ret = Tx.FeePerByte.CompareTo(otherTx.FeePerByte);
        let ret = self
            .transaction
            .fee_per_byte()
            .cmp(&other_tx.fee_per_byte());
        if ret != Ordering::Equal {
            return ret;
        }

        // ret = Tx.NetworkFee.CompareTo(otherTx.NetworkFee);
        let ret = self.transaction.network_fee.cmp(&other_tx.network_fee);
        if ret != Ordering::Equal {
            return ret;
        }

        // Transaction hash sorted descending
        // return otherTx.Hash.CompareTo(Tx.Hash);
        other_tx.hash().cmp(&self.transaction.hash())
    }

    /// public int CompareTo(PoolItem otherItem)
    pub fn compare_to(&self, other: &PoolItem) -> Ordering {
        self.compare_to_transaction(&other.transaction)
    }
}

// IComparable<PoolItem> implementation
impl PartialEq for PoolItem {
    fn eq(&self, other: &Self) -> bool {
        self.transaction.hash() == other.transaction.hash()
    }
}

impl Eq for PoolItem {}

impl PartialOrd for PoolItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PoolItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_to(other)
    }
}
