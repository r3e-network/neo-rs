//! Reference counting implementation for the Neo VM.
//!
//! This module mirrors the behaviour of `Neo.VM.ReferenceCounter` from the C#
//! codebase as closely as possible without relying on a managed runtime. The
//! Rust port keeps bookkeeping data in `ReferenceCounterInner` rather than on
//! the stack items themselves.
//!
//! # Thread Safety
//!
//! The reference counter uses `parking_lot::Mutex` for thread-safe access
//! without the risk of mutex poisoning that comes with `std::sync::Mutex`.

use crate::i_reference_counter::IReferenceCounter;
use crate::stack_item::StackItem;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Shared state for a reference counter instance.
struct ReferenceCounterState {
    /// Total reference count — separated from the mutex so that push/pop of
    /// primitive items (Null, Boolean, Integer, ByteString) can update it with
    /// a single atomic operation instead of acquiring the mutex.
    references_count: AtomicUsize,
    /// Compound-item tracking data, protected by a mutex.
    tracked: Mutex<TrackedItems>,
}

impl std::fmt::Debug for ReferenceCounterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceCounterState")
            .field("references_count", &self.references_count.load(Ordering::Relaxed))
            .finish()
    }
}

/// Tracks references to VM stack items.
#[derive(Clone, Debug)]
pub struct ReferenceCounter {
    state: Arc<ReferenceCounterState>,
}

impl ReferenceCounter {
    /// Creates a new reference counter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(ReferenceCounterState {
                references_count: AtomicUsize::new(0),
                tracked: Mutex::new(TrackedItems::default()),
            }),
        }
    }

    /// Returns the total number of references currently tracked.
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.state.references_count.load(Ordering::Relaxed)
    }

    /// Resets the counter to an empty state.
    pub fn clear(&self) {
        self.state.references_count.store(0, Ordering::Relaxed);
        let mut tracked = self.state.tracked.lock();
        *tracked = TrackedItems::default();
    }

    /// Adds `count` stack references for the supplied item.
    #[inline]
    pub fn add_stack_reference(&self, item: &StackItem, count: usize) {
        if count == 0 {
            return;
        }
        // Always bump the global counter (lock-free).
        self.state.references_count.fetch_add(count, Ordering::Relaxed);

        // Only acquire the mutex when the item is a compound type that needs tracking.
        if let Some(id) = ItemId::from(item) {
            let mut tracked = self.state.tracked.lock();
            let record = tracked.ensure_record(id);
            record.stack_references += count;
            tracked.zero_referred.remove(&id);
        }
    }

    /// Removes a single stack reference from the supplied item.
    #[inline]
    pub fn remove_stack_reference(&self, item: &StackItem) {
        // Always decrement the global counter (lock-free).
        self.state.references_count.fetch_sub(1, Ordering::Relaxed);

        // Only acquire the mutex when the item is a compound type that needs tracking.
        if let Some(id) = ItemId::from(item) {
            let mut tracked = self.state.tracked.lock();
            if let Some(record) = tracked.tracked_items.get_mut(&id) {
                if record.stack_references > 0 {
                    record.stack_references -= 1;
                }
                if record.stack_references == 0 || record.total_references() == 0 {
                    tracked.zero_referred.insert(id);
                }
            }
        }
    }

    /// Adds a parent/child reference relationship.
    #[inline]
    pub fn add_reference(&self, item: &StackItem, parent: &StackItem) {
        if let Some(parent_id) = ItemId::from(parent) {
            self.add_reference_with_parent_id(item, parent_id);
        } else {
            self.state.references_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Removes a previously tracked parent/child reference.
    #[inline]
    pub fn remove_reference(&self, item: &StackItem, parent: &StackItem) {
        if let Some(parent_id) = ItemId::from(parent) {
            self.remove_reference_with_parent_id(item, parent_id);
        } else {
            self.state.references_count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Adds an item to the zero-referred set so it can be collected later.
    #[inline]
    pub fn add_zero_referred(&self, item: &StackItem) {
        if let Some(id) = ItemId::from(item) {
            let mut tracked = self.state.tracked.lock();
            tracked.ensure_record(id);
            tracked.zero_referred.insert(id);
        }
    }

    /// Processes zero-referred items, matching the behaviour of the C# counter.
    #[inline]
    #[must_use]
    pub fn check_zero_referred(&self) -> usize {
        let mut tracked = self.state.tracked.lock();
        if tracked.zero_referred.is_empty() {
            return self.state.references_count.load(Ordering::Relaxed);
        }

        let candidate_filter: HashSet<ItemId> = tracked.zero_referred.drain().collect();

        let mut tarjan = crate::strongly_connected_components::Tarjan::new();
        for id in tracked.tracked_items.keys() {
            tarjan.add_vertex(*id);
        }
        for (parent_id, record) in &tracked.tracked_items {
            for (child_id, count) in &record.children {
                if *count > 0 {
                    tarjan.add_edge(*parent_id, *child_id);
                }
            }
        }

        let mut components_to_remove: Vec<Vec<ItemId>> = Vec::with_capacity(candidate_filter.len());
        for component in tarjan.find_components().iter().cloned() {
            if !component.iter().any(|id| candidate_filter.contains(id)) {
                continue;
            }

            let component_set: HashSet<ItemId> = component.iter().copied().collect();
            let mut keep = false;

            for id in &component {
                if let Some(record) = tracked.tracked_items.get(id) {
                    if record.stack_references > 0 {
                        keep = true;
                        break;
                    }

                    if record
                        .parents
                        .iter()
                        .any(|(parent_id, count)| *count > 0 && !component_set.contains(parent_id))
                    {
                        keep = true;
                        break;
                    }
                }
            }

            if !keep {
                components_to_remove.push(component);
            }
        }

        for component in components_to_remove {
            let component_set: HashSet<ItemId> = component.iter().copied().collect();
            let mut released_internal = 0usize;
            let mut external_parent_updates: Vec<(ItemId, ItemId, usize)> =
                Vec::with_capacity(component.len());
            let mut external_child_updates: Vec<(ItemId, ItemId, usize)> =
                Vec::with_capacity(component.len());

            for id in &component {
                if let Some(record) = tracked.tracked_items.get(id) {
                    released_internal += record.stack_references;

                    for (parent_id, count) in &record.parents {
                        if *count == 0 {
                            continue;
                        }
                        if component_set.contains(parent_id) {
                            released_internal += *count;
                        } else {
                            external_parent_updates.push((*parent_id, *id, *count));
                        }
                    }

                    for (child_id, count) in &record.children {
                        if *count == 0 {
                            continue;
                        }
                        if !component_set.contains(child_id) {
                            external_child_updates.push((*child_id, *id, *count));
                        }
                    }
                }
            }

            for (parent_id, child_id, count) in external_parent_updates {
                if let Some(parent_record) = tracked.tracked_items.get_mut(&parent_id) {
                    for _ in 0..count {
                        parent_record.remove_child(&child_id);
                    }
                }
                self.state.references_count.fetch_sub(count, Ordering::Relaxed);
            }

            for (child_id, parent_id, count) in external_child_updates {
                if let Some(child_record) = tracked.tracked_items.get_mut(&child_id) {
                    for _ in 0..count {
                        child_record.remove_parent(&parent_id);
                    }
                    if child_record.total_references() == 0 {
                        tracked.zero_referred.insert(child_id);
                    }
                }
                self.state.references_count.fetch_sub(count, Ordering::Relaxed);
            }

            self.state.references_count.fetch_sub(released_internal, Ordering::Relaxed);

            for id in &component {
                tracked.zero_referred.remove(id);
                tracked.tracked_items.remove(id);
            }
        }

        self.state.references_count.load(Ordering::Relaxed)
    }

    /// Adds a parent reference using the parent's tracked identity.
    pub fn add_compound_reference(&self, item: &StackItem, parent: CompoundParent) {
        self.add_reference_with_parent_id(item, parent.into());
    }

    /// Removes a parent reference using the parent's tracked identity.
    pub fn remove_compound_reference(&self, item: &StackItem, parent: CompoundParent) {
        self.remove_reference_with_parent_id(item, parent.into());
    }

    /// Returns true when both counters share the same underlying state.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    fn add_reference_with_parent_id(&self, item: &StackItem, parent_id: ItemId) {
        self.state.references_count.fetch_add(1, Ordering::Relaxed);

        let mut tracked = self.state.tracked.lock();
        if let Some(item_id) = ItemId::from(item) {
            {
                let record = tracked.ensure_record(item_id);
                record.add_parent(parent_id);
                tracked.zero_referred.remove(&item_id);
            }
            {
                let parent_record = tracked.ensure_record(parent_id);
                parent_record.add_child(item_id);
            }
        } else {
            tracked.ensure_record(parent_id);
        }
    }

    fn remove_reference_with_parent_id(&self, item: &StackItem, parent_id: ItemId) {
        self.state.references_count.fetch_sub(1, Ordering::Relaxed);

        let mut tracked = self.state.tracked.lock();
        if let Some(item_id) = ItemId::from(item) {
            if let Some(record) = tracked.tracked_items.get_mut(&item_id) {
                record.remove_parent(&parent_id);
                if record.stack_references == 0 {
                    tracked.zero_referred.insert(item_id);
                }
            }

            if let Some(parent_record) = tracked.tracked_items.get_mut(&parent_id) {
                parent_record.remove_child(&item_id);
            }
        }
    }
}

impl Default for ReferenceCounter {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl IReferenceCounter for ReferenceCounter {
    fn count(&self) -> usize {
        Self::count(self)
    }

    fn add_zero_referred(&self, item: &StackItem) {
        Self::add_zero_referred(self, item);
    }

    fn add_reference(&self, item: &StackItem, parent: &StackItem) {
        Self::add_reference(self, item, parent);
    }

    fn add_stack_reference(&self, item: &StackItem, count: usize) {
        Self::add_stack_reference(self, item, count);
    }

    fn remove_reference(&self, item: &StackItem, parent: &StackItem) {
        Self::remove_reference(self, item, parent);
    }

    fn remove_stack_reference(&self, item: &StackItem) {
        Self::remove_stack_reference(self, item);
    }

    fn check_zero_referred(&self) -> usize {
        Self::check_zero_referred(self)
    }
}

/// Identifies a tracked parent compound item.
#[derive(Clone, Copy, Debug)]
pub enum CompoundParent {
    /// Array parent reference
    Array(usize),
    /// Struct parent reference
    Struct(usize),
    /// Map parent reference
    Map(usize),
    /// Buffer parent reference
    Buffer(usize),
}

#[derive(Default, Debug)]
struct TrackedItems {
    tracked_items: HashMap<ItemId, ItemRecord>,
    zero_referred: HashSet<ItemId>,
}

impl TrackedItems {
    fn ensure_record(&mut self, id: ItemId) -> &mut ItemRecord {
        self.tracked_items.entry(id).or_default()
    }
}

#[derive(Clone, Copy, Debug, Eq)]
enum ItemId {
    Array(usize),
    Struct(usize),
    Map(usize),
    Buffer(usize),
}

impl ItemId {
    fn from(item: &StackItem) -> Option<Self> {
        match item {
            StackItem::Array(inner) => Some(Self::Array(inner.id())),
            StackItem::Struct(inner) => Some(Self::Struct(inner.id())),
            StackItem::Map(inner) => Some(Self::Map(inner.id())),
            StackItem::Buffer(inner) => Some(Self::Buffer(inner.id())),
            _ => None,
        }
    }
}

impl From<CompoundParent> for ItemId {
    fn from(parent: CompoundParent) -> Self {
        match parent {
            CompoundParent::Array(id) => Self::Array(id),
            CompoundParent::Struct(id) => Self::Struct(id),
            CompoundParent::Map(id) => Self::Map(id),
            CompoundParent::Buffer(id) => Self::Buffer(id),
        }
    }
}

impl PartialEq for ItemId {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Array(a), Self::Array(b))
                | (Self::Struct(a), Self::Struct(b))
                | (Self::Map(a), Self::Map(b))
                | (Self::Buffer(a), Self::Buffer(b))
                if a == b
        )
    }
}

impl Hash for ItemId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Array(id) => {
                0u8.hash(state);
                id.hash(state);
            }
            Self::Struct(id) => {
                1u8.hash(state);
                id.hash(state);
            }
            Self::Map(id) => {
                2u8.hash(state);
                id.hash(state);
            }
            Self::Buffer(id) => {
                3u8.hash(state);
                id.hash(state);
            }
        }
    }
}

#[derive(Default, Debug)]
struct ItemRecord {
    stack_references: usize,
    parents: HashMap<ItemId, usize>,
    children: HashMap<ItemId, usize>,
}

impl ItemRecord {
    fn total_references(&self) -> usize {
        self.stack_references + self.parents.values().copied().sum::<usize>()
    }

    fn add_parent(&mut self, parent: ItemId) {
        *self.parents.entry(parent).or_insert(0) += 1;
    }

    fn remove_parent(&mut self, parent: &ItemId) {
        if let Some(count) = self.parents.get_mut(parent) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                self.parents.remove(parent);
            }
        }
    }

    fn add_child(&mut self, child: ItemId) {
        *self.children.entry(child).or_insert(0) += 1;
    }

    fn remove_child(&mut self, child: &ItemId) {
        if let Some(count) = self.children.get_mut(child) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                self.children.remove(child);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stack_item::StackItem;

    fn make_array() -> StackItem {
        StackItem::from_array(vec![StackItem::from_int(1), StackItem::Null])
    }

    fn make_struct() -> StackItem {
        StackItem::from_struct(vec![StackItem::from_int(42)])
    }

    #[test]
    fn stack_references_increment_and_decrement() {
        let counter = ReferenceCounter::new();
        let item = make_array();

        counter.add_stack_reference(&item, 1);
        assert_eq!(counter.count(), 1);

        counter.remove_stack_reference(&item);
        assert_eq!(counter.count(), 0);
        assert_eq!(counter.check_zero_referred(), 0);
    }

    #[test]
    fn object_references_affect_zero_tracking() {
        let counter = ReferenceCounter::new();
        let child = make_struct();
        let parent = make_array();

        counter.add_reference(&child, &parent);
        assert_eq!(counter.count(), 1);

        counter.remove_reference(&child, &parent);
        assert_eq!(counter.count(), 0);
        assert_eq!(counter.check_zero_referred(), 0);
    }

    #[test]
    fn zero_referred_removes_item_records() {
        let counter = ReferenceCounter::new();
        let item = make_array();

        counter.add_stack_reference(&item, 2);
        assert_eq!(counter.count(), 2);

        counter.remove_stack_reference(&item);
        counter.remove_stack_reference(&item);
        assert_eq!(counter.count(), 0);

        // First call drains the zero set and clears the tracked record.
        assert_eq!(counter.check_zero_referred(), 0);

        // Second call should be a no-op.
        assert_eq!(counter.check_zero_referred(), 0);
    }
}
