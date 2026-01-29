use crate::network::p2p::payloads::{Transaction, TransactionAttributeType};
use std::cmp::Ordering;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone)]
pub struct PoolItem {
    pub transaction: Arc<Transaction>,
    pub timestamp: SystemTime,
    pub last_broadcast_timestamp: SystemTime,
}

impl PoolItem {
    pub(crate) fn new(tx: Transaction) -> Self {
        let now = SystemTime::now();
        Self {
            transaction: Arc::new(tx),
            timestamp: now,
            last_broadcast_timestamp: now,
        }
    }

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
    use crate::network::p2p::payloads::{Transaction, TransactionAttribute, Witness};
    use neo_vm::OpCode;
    use std::cmp::Ordering;

    fn make_transaction(nonce: u32, network_fee: i64, high_priority: bool) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_network_fee(network_fee);
        tx.set_script(vec![OpCode::RET as u8]);
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
