use std::collections::{BTreeMap, VecDeque};

use super::PendingTransaction;

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

    pub fn get(&self, id: &str) -> Option<&PendingTransaction> {
        self.entries.get(id)
    }

    pub fn reserve_for_block(
        &mut self,
        max_transactions: usize,
        max_bytes: u64,
    ) -> Vec<PendingTransaction> {
        let mut selected = Vec::new();
        let mut total_bytes = 0u64;
        while selected.len() < max_transactions {
            let Some(next_id) = self.queue.pop_front() else {
                break;
            };
            let Some(tx) = self.entries.get(&next_id) else {
                continue;
            };
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

    pub fn total_size_bytes(&self) -> u64 {
        self.entries.values().map(|tx| tx.size_bytes as u64).sum()
    }

    pub fn total_fees(&self) -> u64 {
        self.entries.values().map(|tx| tx.fee).sum()
    }

    pub fn snapshot(&self) -> Vec<PendingTransaction> {
        self.queue
            .iter()
            .filter_map(|id| self.entries.get(id))
            .cloned()
            .collect()
    }

    pub fn restore(&mut self, pending: Vec<PendingTransaction>) {
        self.queue.clear();
        self.entries.clear();
        for tx in pending {
            let id = tx.id.clone();
            self.queue.push_back(id.clone());
            self.entries.insert(id, tx);
        }
    }
}
