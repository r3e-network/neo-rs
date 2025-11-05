use std::collections::{BTreeMap, VecDeque};

/// Pending transaction tracked by the runtime pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTransaction {
    pub id: String,
    pub fee: u64,
    pub size_bytes: u32,
}

impl PendingTransaction {
    pub fn new(id: impl Into<String>, fee: u64, size_bytes: u32) -> Self {
        Self {
            id: id.into(),
            fee,
            size_bytes,
        }
    }
}

/// Simple FIFO transaction pool with deduplication and basic prioritisation.
#[derive(Debug, Default)]
pub struct TxPool {
    queue: VecDeque<String>,
    entries: BTreeMap<String, PendingTransaction>,
}

impl TxPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn insert(&mut self, tx: PendingTransaction) -> bool {
        let id = tx.id.clone();
        if self.entries.contains_key(&id) {
            return false;
        }
        self.queue.push_back(id.clone());
        self.entries.insert(id, tx);
        true
    }

    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.queue.iter()
    }

    pub fn remove(&mut self, id: &str) -> Option<PendingTransaction> {
        if let Some(pos) = self.queue.iter().position(|queued| queued == id) {
            self.queue.remove(pos);
        }
        self.entries.remove(id)
    }

    pub fn reserve_for_block(
        &mut self,
        max_transactions: usize,
        max_bytes: u64,
    ) -> Vec<PendingTransaction> {
        let mut selected = Vec::new();
        let mut total_bytes = 0u64;
        while selected.len() < max_transactions {
            let Some(next_id) = self.queue.pop_front() else { break };
            let Some(tx) = self.entries.get(&next_id) else { continue };
            if total_bytes + tx.size_bytes as u64 > max_bytes {
                self.queue.push_front(next_id);
                break;
            }
            let tx = self.entries.remove(&next_id).unwrap();
            total_bytes += tx.size_bytes as u64;
            selected.push(tx);
        }
        selected
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_reserve() {
        let mut pool = TxPool::new();
        assert!(pool.insert(PendingTransaction::new("tx1", 10, 100)));
        assert!(pool.insert(PendingTransaction::new("tx2", 20, 150)));
        assert!(!pool.insert(PendingTransaction::new("tx1", 5, 50)));

        let reserved = pool.reserve_for_block(10, 120);
        assert_eq!(reserved.len(), 1);
        assert_eq!(reserved[0].id, "tx1");
        assert!(pool.contains("tx2"));
    }

    #[test]
    fn remove_eliminates_from_queue() {
        let mut pool = TxPool::new();
        pool.insert(PendingTransaction::new("tx1", 10, 100));
        pool.insert(PendingTransaction::new("tx2", 20, 150));
        assert!(pool.remove("tx1").is_some());
        assert!(!pool.contains("tx1"));
        let reserved = pool.reserve_for_block(5, 1_000);
        assert_eq!(reserved.len(), 1);
        assert_eq!(reserved[0].id, "tx2");
    }
}
