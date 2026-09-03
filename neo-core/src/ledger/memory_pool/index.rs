use super::PoolItem;
use crate::UInt256;
use crate::network::p2p::payloads::Transaction;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct PoolIndex {
    transactions: HashMap<UInt256, PoolItem>,
    sorted: BTreeSet<PoolItem>,
}

impl PoolIndex {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            transactions: HashMap::with_capacity(capacity),
            sorted: BTreeSet::new(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.transactions.len()
    }

    pub(super) fn sorted_len(&self) -> usize {
        self.sorted.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    pub(super) fn contains_key(&self, hash: &UInt256) -> bool {
        self.transactions.contains_key(hash)
    }

    pub(super) fn get(&self, hash: &UInt256) -> Option<&PoolItem> {
        self.transactions.get(hash)
    }

    pub(super) fn get_mut(&mut self, hash: &UInt256) -> Option<&mut PoolItem> {
        self.transactions.get_mut(hash)
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &PoolItem> {
        self.transactions.values()
    }

    pub(super) fn by_priority_ascending(&self) -> impl DoubleEndedIterator<Item = &PoolItem> {
        self.sorted.iter()
    }

    pub(super) fn by_priority_descending(&self) -> impl Iterator<Item = &PoolItem> {
        self.sorted.iter().rev()
    }

    pub(super) fn lowest(&self) -> Option<&PoolItem> {
        self.sorted.iter().next()
    }

    pub(super) fn reserve(&mut self, additional: usize) {
        self.transactions.reserve(additional);
    }

    pub(super) fn insert(&mut self, hash: UInt256, item: PoolItem) -> Option<PoolItem> {
        let replaced = self.transactions.insert(hash, item.clone());
        if let Some(previous) = &replaced {
            self.sorted.take(previous);
        }
        self.sorted.insert(item);
        replaced
    }

    pub(super) fn remove(&mut self, hash: &UInt256) -> Option<PoolItem> {
        let item = self.transactions.remove(hash)?;
        self.sorted.take(&item);
        Some(item)
    }

    pub(super) fn clear(&mut self) {
        self.transactions.clear();
        self.sorted.clear();
    }

    pub(super) fn drain_by_priority(&mut self) -> Vec<PoolItem> {
        let sorted = std::mem::take(&mut self.sorted);
        let mut transactions = std::mem::take(&mut self.transactions);
        let mut items = Vec::with_capacity(sorted.len().max(transactions.len()));

        for item in sorted {
            let hash = item.transaction.hash();
            items.push(transactions.remove(&hash).unwrap_or(item));
        }
        items.extend(transactions.into_values());
        items
    }

    pub(super) fn into_transactions(self) -> Vec<Transaction> {
        self.transactions
            .into_values()
            .map(|item| Arc::try_unwrap(item.transaction).unwrap_or_else(|arc| (*arc).clone()))
            .collect()
    }
}
