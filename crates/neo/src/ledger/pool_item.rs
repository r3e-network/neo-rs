use crate::network::p2p::payloads::Transaction;
use std::cmp::Ordering;
use std::time::SystemTime;

#[derive(Clone)]
pub struct PoolItem {
    pub transaction: Transaction,
    pub timestamp: SystemTime,
    pub last_broadcast_timestamp: SystemTime,
}

impl PoolItem {
    pub(crate) fn new(tx: Transaction) -> Self {
        let now = SystemTime::now();
        Self {
            transaction: tx,
            timestamp: now,
            last_broadcast_timestamp: now,
        }
    }

    pub fn compare_to_transaction(&self, other_tx: &Transaction) -> Ordering {
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
