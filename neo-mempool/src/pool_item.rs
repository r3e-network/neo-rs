//! [`PoolItem`] - a [`Transaction`] wrapper that tracks mempool-side
//! metadata (insertion timestamp, last-broadcast timestamp, etc.) used
//! by the [`MemoryPool`](crate::MemoryPool) priority queue.

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
    /// When the transaction entered the pool.
    pub timestamp: SystemTime,
    /// When the transaction was last broadcast to peers.
    pub last_broadcast_timestamp: SystemTime,
}

impl fmt::Debug for PoolItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PoolItem")
            .field("hash", &self.hash())
            .field("timestamp", &self.timestamp)
            .field("last_broadcast_timestamp", &self.last_broadcast_timestamp)
            .finish()
    }
}

impl PoolItem {
    /// Constructs a new `PoolItem` for the given transaction with the
    /// current system time as both the insertion and last-broadcast
    /// timestamp.
    pub fn new(tx: Transaction) -> Self {
        Self::with_timestamps(tx, SystemTime::now(), SystemTime::now())
    }

    /// Constructs a new `PoolItem` with explicit timestamps. Useful
    /// for tests and for replaying mempool state from disk.
    pub fn with_timestamps(
        tx: Transaction,
        timestamp: SystemTime,
        last_broadcast_timestamp: SystemTime,
    ) -> Self {
        Self {
            transaction: Arc::new(tx),
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
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_to(other)
    }
}

#[cfg(test)]
mod tests {
    use super::PoolItem;
    use neo_payloads::{Signer, Transaction, TransactionAttribute, Witness};
    use neo_primitives::{UInt160, WitnessScope};
    use neo_vm_rs::OpCode;
    use std::cmp::Ordering;

    fn make_transaction(nonce: u32, network_fee: i64, high_priority: bool) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_network_fee(network_fee);
        tx.set_script(vec![OpCode::RET.byte()]);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_witnesses(vec![Witness::empty()]);
        if high_priority {
            tx.set_attributes(vec![TransactionAttribute::high_priority()]);
        }
        tx
    }

    #[test]
    fn pool_item_compare_orders_by_fee() {
        let tx1 = make_transaction(1, 1, false);
        let tx2 = make_transaction(2, 2, false);
        let item1 = PoolItem::new(tx1);
        let item2 = PoolItem::new(tx2);
        assert_eq!(item1.compare_to(&item2), Ordering::Less);
        assert_eq!(item2.compare_to(&item1), Ordering::Greater);
    }

    #[test]
    fn pool_item_compare_respects_high_priority() {
        let low = PoolItem::new(make_transaction(3, 1, false));
        let high = PoolItem::new(make_transaction(4, 1, true));
        assert_eq!(low.compare_to(&high), Ordering::Less);
        assert_eq!(high.compare_to(&low), Ordering::Greater);
    }

    #[test]
    fn pool_item_compare_orders_by_hash_descending() {
        let tx1 = make_transaction(5, 1, false);
        let tx2 = make_transaction(6, 1, false);
        let item1 = PoolItem::new(tx1.clone());
        let item2 = PoolItem::new(tx2.clone());
        let expected = if tx1.hash() > tx2.hash() {
            Ordering::Less
        } else if tx1.hash() < tx2.hash() {
            Ordering::Greater
        } else {
            Ordering::Equal
        };
        assert_eq!(item1.compare_to(&item2), expected);
    }
}
