//! [`PoolItem`] - a [`Transaction`] wrapper that tracks mempool-side
//! metadata (insertion timestamp, last-broadcast timestamp, etc.) used
//! by the [`MemoryPool`](crate::MemoryPool) priority queue.

use crate::TransactionOrigin;
use neo_payloads::{Transaction, TransactionAttributeType};
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

/// A pooled transaction plus mempool-side metadata.
#[derive(Clone)]
pub struct PoolItem {
    /// The underlying transaction.
    pub transaction: Arc<Transaction>,
    /// Submission origin retained across block-persist revalidation.
    pub origin: TransactionOrigin,
    /// When the transaction entered the pool.
    pub timestamp: SystemTime,
    /// When the transaction was last broadcast to peers.
    pub last_broadcast_timestamp: SystemTime,
}

impl fmt::Debug for PoolItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolItem")
            .field("hash", &self.hash())
            .field("origin", &self.origin)
            .field("timestamp", &self.timestamp)
            .field("last_broadcast_timestamp", &self.last_broadcast_timestamp)
            .finish()
    }
}

impl PoolItem {
    /// Constructs a new `PoolItem` for the given transaction with the
    /// current system time as both the insertion and last-broadcast
    /// timestamp.
    pub fn new(tx: Transaction, origin: TransactionOrigin) -> Self {
        Self::with_timestamps(tx, origin, SystemTime::now(), SystemTime::now())
    }

    /// Constructs a new `PoolItem` with explicit timestamps. Useful
    /// for tests and for replaying mempool state from disk.
    pub fn with_timestamps(
        tx: Transaction,
        origin: TransactionOrigin,
        timestamp: SystemTime,
        last_broadcast_timestamp: SystemTime,
    ) -> Self {
        Self {
            transaction: Arc::new(tx),
            origin,
            timestamp,
            last_broadcast_timestamp,
        }
    }

    /// Returns the hash of the underlying transaction.
    pub fn hash(&self) -> neo_primitives::UInt256 {
        self.transaction.hash()
    }

    /// Compares this pool item against another transaction, ordering
    /// by:
    /// 1. high-priority attribute (high > low)
    /// 2. fee-per-byte (descending)
    /// 3. network fee (descending)
    /// 4. transaction hash (descending)
    pub fn compare_to_transaction(&self, other_tx: &Transaction) -> Ordering {
        let self_high = self
            .transaction
            .get_attribute(TransactionAttributeType::HighPriority)
            .is_some();
        let other_high = other_tx
            .get_attribute(TransactionAttributeType::HighPriority)
            .is_some();
        let ret = self_high.cmp(&other_high);
        if ret != Ordering::Equal {
            return ret;
        }

        let ret = self
            .transaction
            .fee_per_byte()
            .cmp(&other_tx.fee_per_byte());
        if ret != Ordering::Equal {
            return ret;
        }

        let ret = self.transaction.network_fee().cmp(&other_tx.network_fee());
        if ret != Ordering::Equal {
            return ret;
        }

        other_tx.hash().cmp(&self.transaction.hash())
    }

    /// Compares this pool item against another pool item using the
    /// same ordering as [`Self::compare_to_transaction`].
    pub fn compare_to(&self, other: &PoolItem) -> Ordering {
        self.compare_to_transaction(&other.transaction)
    }
}

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
    /// Orders by priority (HighPriority > fee_per_byte > network_fee > hash).
    ///
    /// `PartialEq` and `Eq` compare by transaction hash only, which means two
    /// `PoolItem`s wrapping the same transaction are considered equal regardless
    /// of metadata (timestamp, etc.). The `Ord` implementation produces a
    /// deterministic tiebreaker when hashes are equal by returning `Ordering::Equal`,
    /// ensuring `Ord` is consistent with `Eq`. Without this, a `BTreeSet<PoolItem>`
    /// would silently drop transactions when identical hashes are inserted with
    /// different timestamps.
    fn cmp(&self, other: &Self) -> Ordering {
        let ordering = self.compare_to(other);
        if self.hash() == other.hash() {
            // Same transaction → always equal, regardless of priority comparison.
            return Ordering::Equal;
        }
        ordering
    }
}

#[cfg(test)]
#[path = "../tests/pool/pool_item.rs"]
mod tests;
