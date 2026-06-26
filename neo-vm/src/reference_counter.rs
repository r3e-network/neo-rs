//! Reference counting for the Neo VM — the C# v3.10.0 recursive
//! stack-reference model (`Neo.VM.ReferenceCounter`, neo-vm v3.10.0).
//!
//! The total count moves ONLY through [`ReferenceCounter::add_stack_reference`]
//! and [`ReferenceCounter::remove_stack_reference`], which recurse into a
//! compound's sub-items on the `0 <-> count` stack-reference boundary (a
//! compound's children are counted exactly while the compound is itself
//! reachable from an evaluation-stack / slot root). There is no parent/child
//! edge counting and no garbage-collection sweep: a compound whose own
//! `StackReferences` is non-zero gates per-opcode child mutations, and
//! [`ReferenceCounter::count`] exceeding `MaxStackSize` is a protocol fault
//! (`ExecutionEngine.PostExecuteInstruction`).
//!
//! The `== count` (add) and `== 0` (remove) recursion guards are what make this
//! terminate on shared sub-trees and cycles without a Tarjan pass — the count
//! produced is byte-for-byte the C# v3.10.0 quantity.
//!
//! # Thread Safety
//!
//! Per-compound stack-reference bookkeeping is held in a `parking_lot::Mutex`
//! so the counter can be shared (`Arc`) across the engine; the running total is
//! a lock-free atomic so primitive push/pop never touches the map.

use crate::stack_item::StackItem;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};

/// Identity of a tracked compound. Only Array/Struct/Map participate; `Buffer`
/// is not a `CompoundType` in v3.10.0 and is never tracked here.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CompoundId {
    /// An [`crate::stack_item::array::Array`] identified by its stable id.
    Array(usize),
    /// A [`crate::stack_item::struct_item::Struct`] identified by its stable id.
    Struct(usize),
    /// A [`crate::stack_item::map::Map`] identified by its stable id.
    Map(usize),
}

impl CompoundId {
    /// Returns the compound identity of `item`, or `None` for non-compound items.
    #[inline]
    fn from_item(item: &StackItem) -> Option<Self> {
        match item {
            StackItem::Array(array) => Some(Self::Array(array.id())),
            StackItem::Struct(structure) => Some(Self::Struct(structure.id())),
            StackItem::Map(map) => Some(Self::Map(map.id())),
            _ => None,
        }
    }
}

/// The sub-items of a compound, in the C# `SubItems` order (Array/Struct = the
/// list; Map = keys then values, mirroring `Keys.Concat(Values)`).
#[inline]
fn compound_sub_items(item: &StackItem) -> Vec<StackItem> {
    match item {
        StackItem::Array(array) => array.items(),
        StackItem::Struct(structure) => structure.items(),
        StackItem::Map(map) => {
            let dict = map.items();
            dict.keys().cloned().chain(dict.values().cloned()).collect()
        }
        _ => Vec::new(),
    }
}

struct ReferenceCounterState {
    /// Running total reference count (C# `_referencesCount`). Lock-free so
    /// primitive push/pop updates avoid the mutex.
    references_count: AtomicIsize,
    /// Per-compound stack-reference counts (C# `CompoundType.StackReferences`).
    stack_references: Mutex<HashMap<CompoundId, isize>>,
}

impl std::fmt::Debug for ReferenceCounterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceCounterState")
            .field(
                "references_count",
                &self.references_count.load(Ordering::Relaxed),
            )
            .finish()
    }
}

/// Tracks references to VM stack items (C# `Neo.VM.ReferenceCounter`).
#[derive(Clone, Debug)]
pub struct ReferenceCounter {
    state: Arc<ReferenceCounterState>,
}

impl ReferenceCounter {
    /// Creates a new, empty reference counter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(ReferenceCounterState {
                references_count: AtomicIsize::new(0),
                stack_references: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Returns the total number of references currently tracked (C# `Count`).
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.state.references_count.load(Ordering::Relaxed).max(0) as usize
    }

    /// Resets the counter to an empty state.
    pub fn clear(&self) {
        self.state.references_count.store(0, Ordering::Relaxed);
        self.state.stack_references.lock().clear();
    }

    /// C# `AddStackReference(item, count)`: raises the total by `count` and, for
    /// a compound, raises its `StackReferences` by `count`; when that compound
    /// first becomes stack-referenced (`0 -> count`), each sub-item gains one
    /// stack reference recursively.
    pub fn add_stack_reference(&self, item: &StackItem, count: usize) {
        if count == 0 {
            return;
        }
        let count = count as isize;
        self.state
            .references_count
            .fetch_add(count, Ordering::Relaxed);

        if let Some(id) = CompoundId::from_item(item) {
            let recurse = {
                let mut refs = self.state.stack_references.lock();
                let entry = refs.entry(id).or_insert(0);
                *entry += count;
                // C#: `if (StackReferences == count)` — true only on the first
                // (`0 -> count`) transition. Drop the lock before recursing
                // (parking_lot is not re-entrant).
                *entry == count
            };
            if recurse {
                for sub_item in compound_sub_items(item) {
                    self.add_stack_reference(&sub_item, 1);
                }
            }
        }
    }

    /// C# `RemoveStackReference(item)`: lowers the total by one and, for a
    /// compound, lowers its `StackReferences`; when that reaches zero each
    /// sub-item loses one stack reference recursively.
    pub fn remove_stack_reference(&self, item: &StackItem) {
        self.state
            .references_count
            .fetch_sub(1, Ordering::Relaxed);

        if let Some(id) = CompoundId::from_item(item) {
            let recurse = {
                let mut refs = self.state.stack_references.lock();
                match refs.get_mut(&id) {
                    Some(entry) => {
                        *entry -= 1;
                        // C#: `if (StackReferences == 0)`. Removing the entry at
                        // zero is equivalent to a stored zero (absent == 0).
                        if *entry == 0 {
                            refs.remove(&id);
                            true
                        } else {
                            false
                        }
                    }
                    None => false,
                }
            };
            if recurse {
                for sub_item in compound_sub_items(item) {
                    self.remove_stack_reference(&sub_item);
                }
            }
        }
    }

    /// Whether `item` is a compound currently reachable from a stack/slot root
    /// (C# `CompoundType.IsStackReferenced`). Non-compound items are never
    /// "stack referenced" in this sense.
    #[inline]
    #[must_use]
    pub fn is_stack_referenced(&self, item: &StackItem) -> bool {
        CompoundId::from_item(item).is_some_and(|id| self.is_stack_referenced_id(id))
    }

    /// Whether the compound identified by `id` is currently stack-referenced.
    /// Used by the compound mutation methods to gate child reference counting.
    #[inline]
    #[must_use]
    pub fn is_stack_referenced_id(&self, id: CompoundId) -> bool {
        self.state
            .stack_references
            .lock()
            .get(&id)
            .is_some_and(|&n| n != 0)
    }

    /// Returns true when both counters share the same underlying state.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

neo_io::impl_default_via_new!(ReferenceCounter);

#[cfg(test)]
#[path = "tests/reference_counter.rs"]
mod tests;
