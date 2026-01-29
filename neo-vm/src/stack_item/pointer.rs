//! Pointer stack item implementation for the Neo Virtual Machine.
//!
//! Mirrors `Neo.VM.Types.Pointer` by tracking both the script reference and
//! the instruction position. Pointer equality therefore depends on the
//! originating script identity in addition to the offset.

use crate::script::Script;
use crate::stack_item::stack_item_type::StackItemType;
use num_bigint::BigInt;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Represents the instruction pointer in the VM.
#[derive(Debug, Clone)]
pub struct Pointer {
    script: Arc<Script>,
    position: usize,
}

impl Pointer {
    /// Creates a new pointer associated with the given script and position.
    #[must_use] 
    pub const fn new(script: Arc<Script>, position: usize) -> Self {
        Self { script, position }
    }

    /// Returns the script that owns this pointer.
    #[must_use] 
    pub fn script(&self) -> &Script {
        self.script.as_ref()
    }

    /// Returns an `Arc` clone of the script reference.
    #[must_use] 
    pub fn script_arc(&self) -> Arc<Script> {
        Arc::clone(&self.script)
    }

    /// Returns the instruction position inside the script.
    #[must_use] 
    pub const fn position(&self) -> usize {
        self.position
    }

    /// Returns the stack item type.
    #[must_use] 
    pub const fn stack_item_type(&self) -> StackItemType {
        StackItemType::Pointer
    }

    /// Pointers are always truthy in Neo VM.
    #[must_use] 
    pub const fn to_boolean(&self) -> bool {
        true
    }

    /// Returns the pointer position as an integer (used by tests/helpers).
    #[must_use] 
    pub fn to_integer(&self) -> BigInt {
        BigInt::from(self.position)
    }

    /// Creates a deep copy. Since pointers are immutable and reference-counted,
    /// this simply clones the underlying `Arc`.
    #[must_use] 
    pub fn deep_copy(&self) -> Self {
        self.clone()
    }
}

impl PartialEq for Pointer {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position && Arc::ptr_eq(&self.script, &other.script)
    }
}

impl Eq for Pointer {}

impl Hash for Pointer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.script) as usize).hash(state);
        self.position.hash(state);
    }
}

impl PartialOrd for Pointer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pointer {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_addr = Arc::as_ptr(&self.script) as usize;
        let other_addr = Arc::as_ptr(&other.script) as usize;
        match self_addr.cmp(&other_addr) {
            Ordering::Equal => self.position.cmp(&other.position),
            ord => ord,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_equality_depends_on_script_identity() {
        let script_a = Arc::new(Script::new_relaxed(vec![0x01, 0x02]));
        let script_b = Arc::new(Script::new_relaxed(vec![0x01, 0x02]));

        let ptr_a1 = Pointer::new(Arc::clone(&script_a), 10);
        let ptr_a2 = Pointer::new(Arc::clone(&script_a), 10);
        let ptr_b = Pointer::new(script_b, 10);

        assert_eq!(ptr_a1, ptr_a2);
        assert_ne!(ptr_a1, ptr_b);
    }

    #[test]
    fn pointer_ordering_uses_script_identity_then_position() {
        let script = Arc::new(Script::new_relaxed(vec![0x01]));
        let ptr_1 = Pointer::new(Arc::clone(&script), 1);
        let ptr_2 = Pointer::new(Arc::clone(&script), 2);

        assert!(ptr_1 < ptr_2);
    }

    #[test]
    fn pointer_to_boolean_and_integer() {
        let script = Arc::new(Script::new_relaxed(vec![0x01]));
        let pointer = Pointer::new(script, 42);

        assert!(pointer.to_boolean());
        assert_eq!(pointer.to_integer(), BigInt::from(42));
        assert_eq!(pointer.stack_item_type(), StackItemType::Pointer);
    }
}
