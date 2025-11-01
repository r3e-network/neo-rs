//! Reference counting implementation for the Neo VM.
//!
//! This module mirrors the behaviour of `Neo.VM.ReferenceCounter` from the C#
//! codebase as closely as possible without relying on a managed runtime. The
//! Rust port keeps bookkeeping data in `ReferenceCounterInner` rather than on
//! the stack items themselves.

use crate::i_reference_counter::IReferenceCounter;
use crate::stack_item::StackItem;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

/// Tracks references to VM stack items.
#[derive(Clone, Debug)]
pub struct ReferenceCounter {
    inner: Arc<Mutex<ReferenceCounterInner>>,
}

impl ReferenceCounter {
    /// Creates a new reference counter.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ReferenceCounterInner::default())),
        }
    }

    /// Returns the total number of references currently tracked.
    pub fn count(&self) -> usize {
        self.inner.lock().expect("lock poisoned").references_count
    }

    /// Resets the counter to an empty state.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().expect("lock poisoned");
        *inner = ReferenceCounterInner::default();
    }

    /// Adds `count` stack references for the supplied item.
    pub fn add_stack_reference(&self, item: &StackItem, count: usize) {
        self.add_stack_reference_internal(item, count);
    }

    /// Removes a single stack reference from the supplied item.
    pub fn remove_stack_reference(&self, item: &StackItem) {
        self.remove_stack_reference_internal(item);
    }

    /// Adds a parent/child reference relationship.
    pub fn add_reference(&self, item: &StackItem, parent: &StackItem) {
        self.add_reference_internal(item, parent);
    }

    /// Removes a previously tracked parent/child reference.
    pub fn remove_reference(&self, item: &StackItem, parent: &StackItem) {
        self.remove_reference_internal(item, parent);
    }

    /// Adds an item to the zero-referred set so it can be collected later.
    pub fn add_zero_referred(&self, item: &StackItem) {
        self.add_zero_referred_internal(item);
    }

    /// Processes zero-referred items, matching the behaviour of the C# counter.
    pub fn check_zero_referred(&self) -> usize {
        self.check_zero_referred_internal()
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
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    fn add_stack_reference_internal(&self, item: &StackItem, count: usize) {
        if count == 0 {
            return;
        }

        let mut inner = self.inner.lock().expect("lock poisoned");
        inner.references_count += count;

        if let Some(id) = ItemId::from(item) {
            let record = inner.ensure_record(id);
            record.stack_references += count;
            inner.zero_referred.remove(&id);
        }
    }

    fn remove_stack_reference_internal(&self, item: &StackItem) {
        let mut inner = self.inner.lock().expect("lock poisoned");
        inner.references_count = inner.references_count.saturating_sub(1);

        if let Some(id) = ItemId::from(item) {
            let mut enqueue = false;
            {
                if let Some(record) = inner.tracked_items.get_mut(&id) {
                    if record.stack_references > 0 {
                        record.stack_references -= 1;
                        if record.stack_references == 0 {
                            enqueue = true;
                        }
                    }
                    if record.total_references() == 0 {
                        enqueue = true;
                    }
                }
            }
            if enqueue {
                inner.zero_referred.insert(id);
            }
        }
    }

    fn add_reference_internal(&self, item: &StackItem, parent: &StackItem) {
        if let Some(parent_id) = ItemId::from(parent) {
            self.add_reference_with_parent_id(item, parent_id);
        } else {
            // Even if the parent is not tracked we still increase the global count.
            self.inner.lock().expect("lock poisoned").references_count += 1;
        }
    }

    fn remove_reference_internal(&self, item: &StackItem, parent: &StackItem) {
        if let Some(parent_id) = ItemId::from(parent) {
            self.remove_reference_with_parent_id(item, parent_id);
        } else {
            let mut inner = self.inner.lock().expect("lock poisoned");
            inner.references_count = inner.references_count.saturating_sub(1);
        }
    }

    fn add_zero_referred_internal(&self, item: &StackItem) {
        if let Some(id) = ItemId::from(item) {
            let mut inner = self.inner.lock().expect("lock poisoned");
            inner.ensure_record(id);
            inner.zero_referred.insert(id);
        }
    }

    fn check_zero_referred_internal(&self) -> usize {
        let mut inner = self.inner.lock().expect("lock poisoned");
        if inner.zero_referred.is_empty() {
            return inner.references_count;
        }

        let candidate_filter: HashSet<ItemId> = inner.zero_referred.drain().collect();

        let mut tarjan = crate::strongly_connected_components::Tarjan::new();
        for id in inner.tracked_items.keys() {
            tarjan.add_vertex(*id);
        }
        for (parent_id, record) in &inner.tracked_items {
            for (child_id, count) in &record.children {
                if *count > 0 {
                    tarjan.add_edge(*parent_id, *child_id);
                }
            }
        }

        let mut components_to_remove: Vec<Vec<ItemId>> = Vec::new();
        for component in tarjan.find_components().to_vec() {
            if !component.iter().any(|id| candidate_filter.contains(id)) {
                continue;
            }

            let component_set: HashSet<ItemId> = component.iter().copied().collect();
            let mut keep = false;

            for id in &component {
                if let Some(record) = inner.tracked_items.get(id) {
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
            let mut external_parent_updates: Vec<(ItemId, ItemId, usize)> = Vec::new();
            let mut external_child_updates: Vec<(ItemId, ItemId, usize)> = Vec::new();

            for id in &component {
                if let Some(record) = inner.tracked_items.get(id) {
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
                if let Some(parent_record) = inner.tracked_items.get_mut(&parent_id) {
                    for _ in 0..count {
                        parent_record.remove_child(&child_id);
                    }
                }
                inner.references_count = inner.references_count.saturating_sub(count);
            }

            for (child_id, parent_id, count) in external_child_updates {
                if let Some(child_record) = inner.tracked_items.get_mut(&child_id) {
                    for _ in 0..count {
                        child_record.remove_parent(&parent_id);
                    }
                    if child_record.total_references() == 0 {
                        inner.zero_referred.insert(child_id);
                    }
                }
                inner.references_count = inner.references_count.saturating_sub(count);
            }

            inner.references_count = inner.references_count.saturating_sub(released_internal);

            for id in &component {
                inner.zero_referred.remove(id);
                inner.tracked_items.remove(id);
            }
        }

        inner.references_count
    }

    fn add_reference_with_parent_id(&self, item: &StackItem, parent_id: ItemId) {
        let mut inner = self.inner.lock().expect("lock poisoned");
        inner.references_count += 1;

        if let Some(item_id) = ItemId::from(item) {
            {
                let record = inner.ensure_record(item_id);
                record.add_parent(parent_id);
                inner.zero_referred.remove(&item_id);
            }
            {
                let parent_record = inner.ensure_record(parent_id);
                parent_record.add_child(item_id);
            }
        } else {
            inner.ensure_record(parent_id);
        }
    }

    fn remove_reference_with_parent_id(&self, item: &StackItem, parent_id: ItemId) {
        let mut inner = self.inner.lock().expect("lock poisoned");
        inner.references_count = inner.references_count.saturating_sub(1);

        if let Some(item_id) = ItemId::from(item) {
            if let Some(record) = inner.tracked_items.get_mut(&item_id) {
                record.remove_parent(&parent_id);
                if record.total_references() == 0 {
                    inner.zero_referred.insert(item_id);
                }
            }

            if let Some(parent_record) = inner.tracked_items.get_mut(&parent_id) {
                parent_record.remove_child(&item_id);
            }
        }
    }
}

impl Default for ReferenceCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl IReferenceCounter for ReferenceCounter {
    fn count(&self) -> usize {
        ReferenceCounter::count(self)
    }

    fn add_zero_referred(&self, item: &StackItem) {
        self.add_zero_referred_internal(item);
    }

    fn add_reference(&self, item: &StackItem, parent: &StackItem) {
        self.add_reference_internal(item, parent);
    }

    fn add_stack_reference(&self, item: &StackItem, count: usize) {
        self.add_stack_reference_internal(item, count);
    }

    fn remove_reference(&self, item: &StackItem, parent: &StackItem) {
        self.remove_reference_internal(item, parent);
    }

    fn remove_stack_reference(&self, item: &StackItem) {
        self.remove_stack_reference_internal(item);
    }

    fn check_zero_referred(&self) -> usize {
        self.check_zero_referred_internal()
    }
}

/// Identifies a tracked parent compound item.
#[derive(Clone, Copy, Debug)]
pub enum CompoundParent {
    Array(usize),
    Struct(usize),
    Map(usize),
    Buffer(usize),
}

#[derive(Default, Debug)]
struct ReferenceCounterInner {
    references_count: usize,
    tracked_items: HashMap<ItemId, ItemRecord>,
    zero_referred: HashSet<ItemId>,
}

impl ReferenceCounterInner {
    fn ensure_record(&mut self, id: ItemId) -> &mut ItemRecord {
        self.tracked_items
            .entry(id)
            .or_insert_with(ItemRecord::default)
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
            CompoundParent::Array(id) => ItemId::Array(id),
            CompoundParent::Struct(id) => ItemId::Struct(id),
            CompoundParent::Map(id) => ItemId::Map(id),
            CompoundParent::Buffer(id) => ItemId::Buffer(id),
        }
    }
}

impl PartialEq for ItemId {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (ItemId::Array(a), ItemId::Array(b))
                | (ItemId::Struct(a), ItemId::Struct(b))
                | (ItemId::Map(a), ItemId::Map(b))
                | (ItemId::Buffer(a), ItemId::Buffer(b))
                if a == b
        )
    }
}

impl Hash for ItemId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            ItemId::Array(id) => {
                0u8.hash(state);
                id.hash(state);
            }
            ItemId::Struct(id) => {
                1u8.hash(state);
                id.hash(state);
            }
            ItemId::Map(id) => {
                2u8.hash(state);
                id.hash(state);
            }
            ItemId::Buffer(id) => {
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
