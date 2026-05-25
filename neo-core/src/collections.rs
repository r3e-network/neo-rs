use indexmap::IndexSet;
use std::hash::Hash;

pub(crate) struct BoundedFifoSet<T> {
    items: IndexSet<T>,
    capacity: usize,
}

impl<T> BoundedFifoSet<T>
where
    T: Eq + Hash,
{
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            items: IndexSet::with_capacity(capacity),
            capacity,
        }
    }

    pub(crate) fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
    }

    pub(crate) fn contains(&self, value: &T) -> bool {
        self.items.contains(value)
    }

    pub(crate) fn insert(&mut self, value: T) -> bool {
        let inserted = self.items.insert(value);
        self.trim_to_capacity();
        inserted
    }

    pub(crate) fn remove(&mut self, value: &T) -> bool {
        self.items.shift_remove(value)
    }

    fn trim_to_capacity(&mut self) {
        while self.items.len() > self.capacity {
            self.items.shift_remove_index(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BoundedFifoSet;

    #[test]
    fn duplicate_insert_does_not_refresh_fifo_order() {
        let mut set = BoundedFifoSet::with_capacity(2);

        assert!(set.insert(1));
        assert!(set.insert(2));
        assert!(!set.insert(1));
        assert!(set.insert(3));

        assert!(!set.contains(&1));
        assert!(set.contains(&2));
        assert!(set.contains(&3));
    }

    #[test]
    fn capacity_shrink_trims_on_next_insert() {
        let mut set = BoundedFifoSet::with_capacity(3);

        assert!(set.insert(1));
        assert!(set.insert(2));
        assert!(set.insert(3));

        set.set_capacity(1);
        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert!(set.contains(&3));

        assert!(set.insert(4));
        assert!(!set.contains(&1));
        assert!(!set.contains(&2));
        assert!(!set.contains(&3));
        assert!(set.contains(&4));
    }

    #[test]
    fn remove_drops_membership_and_fifo_position() {
        let mut set = BoundedFifoSet::with_capacity(2);

        assert!(set.insert(1));
        assert!(set.insert(2));
        assert!(set.remove(&1));
        assert!(set.insert(3));
        assert!(set.insert(4));

        assert!(!set.contains(&1));
        assert!(!set.contains(&2));
        assert!(set.contains(&3));
        assert!(set.contains(&4));
    }

    #[test]
    fn zero_capacity_keeps_no_items_but_reports_new_insert() {
        let mut set = BoundedFifoSet::with_capacity(0);

        assert!(set.insert(1));
        assert!(!set.contains(&1));
        assert!(set.insert(1));
    }
}
