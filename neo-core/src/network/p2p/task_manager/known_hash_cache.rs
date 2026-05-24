use crate::UInt256;
use indexmap::IndexSet;

pub(super) struct KnownHashCache {
    hashes: IndexSet<UInt256>,
    capacity: usize,
}

impl KnownHashCache {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            hashes: IndexSet::with_capacity(capacity),
            capacity,
        }
    }

    pub(super) fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
    }

    pub(super) fn contains(&self, hash: &UInt256) -> bool {
        self.hashes.contains(hash)
    }

    pub(super) fn remember(&mut self, hash: UInt256) -> bool {
        let inserted = self.hashes.insert(hash);

        while self.hashes.len() > self.capacity {
            self.hashes.shift_remove_index(0);
        }

        inserted
    }

    pub(super) fn forget(&mut self, hash: &UInt256) -> bool {
        self.hashes.shift_remove(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_without_refreshing_fifo_order() {
        let mut cache = KnownHashCache::new(2);
        let a = UInt256::from([1u8; 32]);
        let b = UInt256::from([2u8; 32]);
        let c = UInt256::from([3u8; 32]);

        assert!(cache.remember(a));
        assert!(cache.remember(b));
        assert!(!cache.remember(a));
        assert!(cache.remember(c));

        assert!(!cache.contains(&a));
        assert!(cache.contains(&b));
        assert!(cache.contains(&c));
    }

    #[test]
    fn forget_removes_membership_and_order() {
        let mut cache = KnownHashCache::new(2);
        let a = UInt256::from([1u8; 32]);
        let b = UInt256::from([2u8; 32]);
        let c = UInt256::from([3u8; 32]);
        let d = UInt256::from([4u8; 32]);

        assert!(cache.remember(a));
        assert!(cache.remember(b));
        assert!(cache.forget(&a));
        assert!(!cache.contains(&a));

        assert!(cache.remember(c));
        assert!(cache.remember(d));

        assert!(!cache.contains(&b));
        assert!(cache.contains(&c));
        assert!(cache.contains(&d));
    }

    #[test]
    fn capacity_change_trims_only_on_next_insert() {
        let mut cache = KnownHashCache::new(3);
        let a = UInt256::from([1u8; 32]);
        let b = UInt256::from([2u8; 32]);
        let c = UInt256::from([3u8; 32]);
        let d = UInt256::from([4u8; 32]);

        assert!(cache.remember(a));
        assert!(cache.remember(b));
        assert!(cache.remember(c));

        cache.set_capacity(2);
        assert!(cache.contains(&a));
        assert!(cache.contains(&b));
        assert!(cache.contains(&c));

        assert!(cache.remember(d));
        assert!(!cache.contains(&a));
        assert!(!cache.contains(&b));
        assert!(cache.contains(&c));
        assert!(cache.contains(&d));
    }
}
