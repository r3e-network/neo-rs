use super::MemoryPool;
#[cfg(test)]
use super::PoolItem;
use crate::network::p2p::payloads::Transaction;
use crate::{UInt160, UInt256};
use std::sync::Arc;

impl MemoryPool {
    /// Returns the highest-priority verified transactions, sorted in descending order by fee.
    /// Uses `Arc<Transaction>` to avoid expensive cloning of transaction data.
    pub fn get_sorted_verified_transactions(&self, limit: usize) -> Vec<Arc<Transaction>> {
        if limit == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(limit.min(self.verified.sorted_len()));
        result.extend(
            self.verified
                .by_priority_descending()
                .take(limit)
                .map(|item| Arc::clone(&item.transaction)),
        );
        result
    }

    /// internal int SortedTxCount => _sortedTransactions.Count;
    #[cfg(test)]
    pub(crate) fn sorted_tx_count(&self) -> usize {
        self.verified.sorted_len()
    }

    /// internal int UnverifiedSortedTxCount => _unverifiedSortedTransactions.Count;
    #[cfg(test)]
    pub(crate) fn unverified_sorted_tx_count(&self) -> usize {
        self.unverified.sorted_len()
    }

    /// public int Count
    pub fn count(&self) -> usize {
        self.verified.len() + self.unverified.len()
    }

    /// public int VerifiedCount => _unsortedTransactions.Count;
    pub fn verified_count(&self) -> usize {
        self.verified.len()
    }

    /// public int UnVerifiedCount => _unverifiedTransactions.Count;
    pub fn unverified_count(&self) -> usize {
        self.unverified.len()
    }

    /// public bool ContainsKey(UInt256 hash)
    pub fn contains_key(&self, hash: &UInt256) -> bool {
        self.verified.contains_key(hash) || self.unverified.contains_key(hash)
    }

    /// Returns the total number of transactions in the pool attributed to `sender`.
    pub fn sender_transaction_count(&self, sender: &UInt160) -> usize {
        self.verified
            .values()
            .filter(|item| item.transaction.sender() == Some(*sender))
            .count()
            + self
                .unverified
                .values()
                .filter(|item| item.transaction.sender() == Some(*sender))
                .count()
    }

    #[cfg(test)]
    fn lowest_fee_item(&self) -> Option<&PoolItem> {
        let verified = self.verified.lowest();
        let unverified = self.unverified.lowest();

        match (verified, unverified) {
            (None, None) => None,
            (Some(item), None) => Some(item),
            (None, Some(item)) => Some(item),
            (Some(verified_item), Some(unverified_item)) => {
                if verified_item.compare_to(unverified_item) != std::cmp::Ordering::Less {
                    Some(unverified_item)
                } else {
                    Some(verified_item)
                }
            }
        }
    }

    /// Returns true if the pool has capacity for a transaction with at least the given priority.
    #[cfg(test)]
    pub(crate) fn can_transaction_fit_in_pool(&self, tx: &Transaction) -> bool {
        if self.count() < self.capacity {
            return true;
        }

        let Some(item) = self.lowest_fee_item() else {
            return false;
        };
        item.compare_to_transaction(tx) != std::cmp::Ordering::Greater
    }

    /// Attempts to fetch a transaction from either the verified or unverified sets.
    /// Returns `Arc<Transaction>` to avoid expensive cloning.
    pub fn try_get(&self, hash: &UInt256) -> Option<Arc<Transaction>> {
        if let Some(item) = self.verified.get(hash) {
            return Some(Arc::clone(&item.transaction));
        }
        self.unverified
            .get(hash)
            .map(|item| Arc::clone(&item.transaction))
    }

    /// Returns the highest priority verified transactions, up to `limit`.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn sorted_verified_transactions(&self, limit: usize) -> Vec<Arc<Transaction>> {
        if limit == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(limit.min(self.verified.sorted_len()));
        result.extend(
            self.verified
                .by_priority_descending()
                .take(limit)
                .map(|item| Arc::clone(&item.transaction)),
        );
        result
    }

    /// Returns all verified transactions without any ordering guarantees.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn verified_transactions_vec(&self) -> Vec<Arc<Transaction>> {
        let mut result = Vec::with_capacity(self.verified.len());
        result.extend(
            self.verified
                .values()
                .map(|item| Arc::clone(&item.transaction)),
        );
        result
    }

    /// Returns all unverified transactions currently tracked by the mempool.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn unverified_transactions_vec(&self) -> Vec<Arc<Transaction>> {
        let mut result = Vec::with_capacity(self.unverified.len());
        result.extend(
            self.unverified
                .values()
                .map(|item| Arc::clone(&item.transaction)),
        );
        result
    }

    /// Returns all transactions (verified followed by unverified) currently tracked by the mempool.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn all_transactions_vec(&self) -> Vec<Arc<Transaction>> {
        let total_len = self.verified.len() + self.unverified.len();
        let mut transactions = Vec::with_capacity(total_len);
        transactions.extend(
            self.verified
                .values()
                .map(|item| Arc::clone(&item.transaction)),
        );
        transactions.extend(
            self.unverified
                .values()
                .map(|item| Arc::clone(&item.transaction)),
        );
        transactions
    }

    /// Returns verified and unverified transactions as separate vectors,
    /// sorted in descending priority order.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn verified_and_unverified_transactions(
        &self,
    ) -> (Vec<Arc<Transaction>>, Vec<Arc<Transaction>>) {
        let verified_capacity = self.verified.sorted_len();
        let unverified_capacity = self.unverified.sorted_len();

        let mut verified = Vec::with_capacity(verified_capacity);
        let mut unverified = Vec::with_capacity(unverified_capacity);

        verified.extend(
            self.verified
                .by_priority_descending()
                .map(|item| Arc::clone(&item.transaction)),
        );
        unverified.extend(
            self.unverified
                .by_priority_descending()
                .map(|item| Arc::clone(&item.transaction)),
        );
        (verified, unverified)
    }

    /// Returns an iterator over verified transactions in descending priority order.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn iter_verified(&self) -> impl Iterator<Item = Arc<Transaction>> + '_ {
        self.verified
            .by_priority_descending()
            .map(|item| Arc::clone(&item.transaction))
    }

    /// Returns an iterator over unverified transactions in descending priority order.
    /// Uses `Arc<Transaction>` to avoid expensive cloning.
    pub fn iter_unverified(&self) -> impl Iterator<Item = Arc<Transaction>> + '_ {
        self.unverified
            .by_priority_descending()
            .map(|item| Arc::clone(&item.transaction))
    }
}

// IReadOnlyCollection<Transaction> implementation
impl IntoIterator for MemoryPool {
    type Item = Transaction;
    type IntoIter = std::vec::IntoIter<Transaction>;

    fn into_iter(self) -> Self::IntoIter {
        let MemoryPool {
            verified,
            unverified,
            ..
        } = self;

        let mut transactions = verified.into_transactions();
        transactions.extend(unverified.into_transactions());
        transactions.into_iter()
    }
}

impl IntoIterator for &MemoryPool {
    type Item = Transaction;
    type IntoIter = std::vec::IntoIter<Transaction>;

    fn into_iter(self) -> Self::IntoIter {
        // Collect all transactions - this requires cloning since we're borrowing self
        let total_len = self.verified.len() + self.unverified.len();
        let mut transactions = Vec::with_capacity(total_len);
        transactions.extend(
            self.verified
                .values()
                .map(|item| item.transaction.as_ref().clone()),
        );
        transactions.extend(
            self.unverified
                .values()
                .map(|item| item.transaction.as_ref().clone()),
        );
        transactions.into_iter()
    }
}
