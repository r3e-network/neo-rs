use crate::StackValue as AbiStackValue;
use alloc::{string::String, vec, vec::Vec};
use core::fmt;

mod abi;
mod counted;
mod propagate;
#[cfg(test)]
mod tests;

pub(crate) use abi::{
    discard_abi_values, ensure_abi_nesting, into_abi_value, to_abi_stack, to_abi_value,
};
pub(crate) use counted::counted_items;
pub(crate) use propagate::{
    compound_id, find_affected_indices, propagate_aliases_from_sources, propagate_update,
};

/// Deepest compound nesting the lightweight interpreter lets a value reach.
///
/// A primitive nests zero levels; an Array, Struct or Map nests one level more
/// than its deepest child. This used to be a limit of its own because clone,
/// drop, debug, equality and the ABI conversions descended one native frame set
/// per level. Every one of those walks now runs on an explicit stack, so the
/// only bound left is the canonical engine's own: a value that nests `n` levels
/// deep needs at least `n` items, and `MaxStackSize` caps those at
/// [`MAX_STACK_SIZE`].
///
/// Keeping the two equal is what removes the old consensus divergence where a
/// script that nested 65 to 2048 levels faulted here and halted in the
/// canonical engine.
pub const MAX_COMPOUND_NESTING_DEPTH: usize = super::state::MAX_STACK_SIZE;

/// The fault for a value that would nest deeper than
/// [`MAX_COMPOUND_NESTING_DEPTH`].
///
/// A host call that fails with this message faults the execution even inside
/// a TRY block, the same as a host call that hands back such a value.
pub const NESTING_DEPTH_ERROR: &str = "compound nesting depth exceeds maximum";
pub(crate) const ITEM_COUNT_ERROR: &str = "item count exceeds maximum stack size";

pub(crate) enum StackValue {
    Integer(i64),
    BigInteger(Vec<u8>),
    ByteString(Vec<u8>),
    Boolean(bool),
    Pointer(usize),
    Array(u64, Vec<StackValue>),
    Struct(u64, Vec<StackValue>),
    Map(u64, Vec<(StackValue, StackValue)>),
    Buffer(u64, Vec<u8>),
    Interop(u64),
    Iterator(u64),
    Null,
}

/// Moves `len` trailing values of `built` into a vector of their own.
///
/// The ABI walkers below build their result bottom-up on `built`; taking the
/// tail is how a compound picks up the children it just finished converting.
fn take_tail<T>(built: &mut Vec<T>, len: usize) -> Vec<T> {
    let start = built.len() - len;
    built.split_off(start)
}

impl StackValue {
    /// Moves the children of `self` out, leaving an empty compound behind.
    ///
    /// Only used by [`Drop`] and by the walkers that consume a value in place,
    /// both of which then hold the children on an explicit stack instead of
    /// letting the drop glue recurse.
    fn take_children(&mut self) -> Vec<StackValue> {
        match self {
            StackValue::Array(_, items) | StackValue::Struct(_, items) => core::mem::take(items),
            StackValue::Map(_, entries) => {
                let entries = core::mem::take(entries);
                let mut flat = Vec::with_capacity(entries.len().saturating_mul(2));
                for (key, value) in entries {
                    flat.push(key);
                    flat.push(value);
                }
                flat
            }
            StackValue::Buffer(_, _)
            | StackValue::Integer(_)
            | StackValue::BigInteger(_)
            | StackValue::ByteString(_)
            | StackValue::Boolean(_)
            | StackValue::Pointer(_)
            | StackValue::Interop(_)
            | StackValue::Iterator(_)
            | StackValue::Null => Vec::new(),
        }
    }
}

/// Frees a value without recursing on its nesting depth.
///
/// A value nesting `MaxStackSize` levels deep is legal, and the drop glue the
/// compiler derives descends one native frame set per level. Children are moved
/// onto an explicit work list here, so only two frames are ever live.
impl Drop for StackValue {
    fn drop(&mut self) {
        let mut pending = self.take_children();
        while let Some(mut value) = pending.pop() {
            let children = value.take_children();
            pending.extend(children);
        }
    }
}

/// Copies a value without recursing on its nesting depth.
impl Clone for StackValue {
    fn clone(&self) -> Self {
        enum Step<'a> {
            Enter(&'a StackValue),
            Array(u64, usize),
            Struct(u64, usize),
            Map(u64, usize),
        }

        let mut steps = vec![Step::Enter(self)];
        let mut built: Vec<StackValue> = Vec::new();
        while let Some(step) = steps.pop() {
            match step {
                Step::Enter(value) => match value {
                    StackValue::Integer(value) => built.push(StackValue::Integer(*value)),
                    StackValue::BigInteger(bytes) => {
                        built.push(StackValue::BigInteger(bytes.clone()));
                    }
                    StackValue::ByteString(bytes) => {
                        built.push(StackValue::ByteString(bytes.clone()));
                    }
                    StackValue::Boolean(value) => built.push(StackValue::Boolean(*value)),
                    StackValue::Pointer(value) => built.push(StackValue::Pointer(*value)),
                    StackValue::Buffer(id, bytes) => {
                        built.push(StackValue::Buffer(*id, bytes.clone()));
                    }
                    StackValue::Interop(handle) => built.push(StackValue::Interop(*handle)),
                    StackValue::Iterator(handle) => built.push(StackValue::Iterator(*handle)),
                    StackValue::Null => built.push(StackValue::Null),
                    StackValue::Array(id, items) => {
                        steps.push(Step::Array(*id, items.len()));
                        for item in items.iter().rev() {
                            steps.push(Step::Enter(item));
                        }
                    }
                    StackValue::Struct(id, items) => {
                        steps.push(Step::Struct(*id, items.len()));
                        for item in items.iter().rev() {
                            steps.push(Step::Enter(item));
                        }
                    }
                    StackValue::Map(id, entries) => {
                        steps.push(Step::Map(*id, entries.len()));
                        for (key, value) in entries.iter().rev() {
                            steps.push(Step::Enter(value));
                            steps.push(Step::Enter(key));
                        }
                    }
                },
                Step::Array(id, len) => {
                    let items = take_tail(&mut built, len);
                    built.push(StackValue::Array(id, items));
                }
                Step::Struct(id, len) => {
                    let items = take_tail(&mut built, len);
                    built.push(StackValue::Struct(id, items));
                }
                Step::Map(id, len) => {
                    let mut flat = take_tail(&mut built, len.saturating_mul(2));
                    let mut entries = Vec::with_capacity(len);
                    let mut rest = flat.drain(..);
                    while let (Some(key), Some(value)) = (rest.next(), rest.next()) {
                        entries.push((key, value));
                    }
                    built.push(StackValue::Map(id, entries));
                }
            }
        }
        built.pop().expect("clone builds exactly one value")
    }
}

/// Compares two values without recursing on their nesting depth.
impl PartialEq for StackValue {
    fn eq(&self, other: &Self) -> bool {
        let mut pending: Vec<(&StackValue, &StackValue)> = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (StackValue::Integer(left), StackValue::Integer(right)) => {
                    if left != right {
                        return false;
                    }
                }
                (StackValue::BigInteger(left), StackValue::BigInteger(right)) => {
                    if left != right {
                        return false;
                    }
                }
                (StackValue::ByteString(left), StackValue::ByteString(right)) => {
                    if left != right {
                        return false;
                    }
                }
                (StackValue::Boolean(left), StackValue::Boolean(right)) => {
                    if left != right {
                        return false;
                    }
                }
                (StackValue::Pointer(left), StackValue::Pointer(right)) => {
                    if left != right {
                        return false;
                    }
                }
                (StackValue::Buffer(left, _), StackValue::Buffer(right, _)) => {
                    if left != right {
                        return false;
                    }
                }
                (StackValue::Interop(left), StackValue::Interop(right)) => {
                    if left != right {
                        return false;
                    }
                }
                (StackValue::Iterator(left), StackValue::Iterator(right)) => {
                    if left != right {
                        return false;
                    }
                }
                (StackValue::Null, StackValue::Null) => {}
                (StackValue::Array(left, left_items), StackValue::Array(right, right_items))
                | (StackValue::Struct(left, left_items), StackValue::Struct(right, right_items)) => {
                    if left != right || left_items.len() != right_items.len() {
                        return false;
                    }
                    pending.extend(left_items.iter().zip(right_items.iter()));
                }
                (StackValue::Map(left, left_items), StackValue::Map(right, right_items)) => {
                    if left != right || left_items.len() != right_items.len() {
                        return false;
                    }
                    for ((left_key, left_value), (right_key, right_value)) in
                        left_items.iter().zip(right_items.iter())
                    {
                        pending.push((left_value, right_value));
                        pending.push((left_key, right_key));
                    }
                }
                _ => return false,
            }
        }
        true
    }
}

/// Formats a value without recursing on its nesting depth.
impl fmt::Debug for StackValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut steps = vec![DebugStep::Enter(self)];
        while let Some(step) = steps.pop() {
            match step {
                DebugStep::Text(text) => f.write_str(text)?,
                DebugStep::Separator => f.write_str(", ")?,
                DebugStep::Enter(value) => match value {
                    StackValue::Integer(value) => write!(f, "Integer({value})")?,
                    StackValue::BigInteger(bytes) => write!(f, "BigInteger({bytes:?})")?,
                    StackValue::ByteString(bytes) => write!(f, "ByteString({bytes:?})")?,
                    StackValue::Boolean(value) => write!(f, "Boolean({value})")?,
                    StackValue::Pointer(value) => write!(f, "Pointer({value})")?,
                    StackValue::Buffer(id, bytes) => write!(f, "Buffer({id}, {bytes:?})")?,
                    StackValue::Interop(handle) => write!(f, "Interop({handle})")?,
                    StackValue::Iterator(handle) => write!(f, "Iterator({handle})")?,
                    StackValue::Null => f.write_str("Null")?,
                    StackValue::Array(id, items) => {
                        write!(f, "Array({id}, ")?;
                        steps.push(DebugStep::Text("])"));
                        push_debug_list(&mut steps, items);
                    }
                    StackValue::Struct(id, items) => {
                        write!(f, "Struct({id}, ")?;
                        steps.push(DebugStep::Text("])"));
                        push_debug_list(&mut steps, items);
                    }
                    StackValue::Map(id, entries) => {
                        write!(f, "Map({id}, [")?;
                        steps.push(DebugStep::Text("])"));
                        for (index, (key, value)) in entries.iter().enumerate().rev() {
                            steps.push(DebugStep::Enter(value));
                            steps.push(DebugStep::Text(", "));
                            steps.push(DebugStep::Enter(key));
                            steps.push(DebugStep::Text("("));
                            if index > 0 {
                                steps.push(DebugStep::Separator);
                            }
                        }
                    }
                },
            }
        }
        Ok(())
    }
}

/// Queues `[a, b, c]` for the debug walker, separators included.
fn push_debug_list<'a>(steps: &mut Vec<DebugStep<'a>>, items: &'a [StackValue]) {
    for (index, item) in items.iter().enumerate().rev() {
        steps.push(DebugStep::Enter(item));
        if index > 0 {
            steps.push(DebugStep::Separator);
        }
    }
    steps.push(DebugStep::Text("["));
}

#[derive(Clone, Copy)]
enum DebugStep<'a> {
    Enter(&'a StackValue),
    Text(&'static str),
    Separator,
}

impl crate::runtime::PendingExceptionValue for StackValue {
    fn from_exception_message(message: alloc::string::String) -> Self {
        Self::ByteString(message.into_bytes())
    }
}

/// Drops `values` without recursing on their nesting depth.
pub(crate) fn release_values(values: Vec<StackValue>) {
    let mut pending = values;
    while let Some(mut value) = pending.pop() {
        let children = value.take_children();
        pending.extend(children);
    }
}

/// Disposes of interpreter values that the execution no longer needs.
#[inline]
pub(crate) fn discard_values(values: Vec<StackValue>) {
    if cfg!(target_arch = "riscv32") {
        // The PolkaVM/riscv32 guest allocator is reset per execution, so the
        // guest skips walking and freeing state it is about to abandon.
        core::mem::forget(values);
    } else {
        // Hosts keep running after the execution, so the memory has to be
        // returned. Compound values can nest arbitrarily deep; release them
        // without a recursive drop.
        release_values(values);
    }
}

/// Returns whether `value` nests more than `limit` compound levels.
///
/// The walk runs on an explicit stack, so a value of any depth costs no more
/// native stack than one frame does.
pub(crate) fn nesting_exceeds(value: &StackValue, limit: usize) -> bool {
    let mut pending: Vec<(&StackValue, usize)> = vec![(value, limit)];
    while let Some((value, limit)) = pending.pop() {
        match value {
            StackValue::Array(_, items) | StackValue::Struct(_, items) => {
                let Some(inner) = limit.checked_sub(1) else {
                    return true;
                };
                pending.extend(items.iter().map(|item| (item, inner)));
            }
            StackValue::Map(_, entries) => {
                let Some(inner) = limit.checked_sub(1) else {
                    return true;
                };
                for (key, value) in entries {
                    pending.push((key, inner));
                    pending.push((value, inner));
                }
            }
            StackValue::Buffer(_, _)
            | StackValue::Integer(_)
            | StackValue::BigInteger(_)
            | StackValue::ByteString(_)
            | StackValue::Boolean(_)
            | StackValue::Pointer(_)
            | StackValue::Interop(_)
            | StackValue::Iterator(_)
            | StackValue::Null => {}
        }
    }
    false
}

/// Rejects `value` unless it can become the child of a compound value.
pub(crate) fn ensure_nestable(value: &StackValue) -> Result<(), String> {
    if nesting_exceeds(value, MAX_COMPOUND_NESTING_DEPTH - 1) {
        return Err(NESTING_DEPTH_ERROR.to_string());
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct CompoundIds {
    next: u64,
}

impl CompoundIds {
    fn alloc(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// Clones a value that APPEND or SETITEM stores into a collection.
    pub(crate) fn clone_for_mutation(&mut self, value: &StackValue) -> StackValue {
        self.clone_struct_for_storage(value)
    }

    pub(crate) fn array(&mut self, items: Vec<StackValue>) -> StackValue {
        StackValue::Array(self.alloc(), items)
    }

    pub(crate) fn r#struct(&mut self, items: Vec<StackValue>) -> StackValue {
        StackValue::Struct(self.alloc(), items)
    }

    pub(crate) fn map(&mut self, items: Vec<(StackValue, StackValue)>) -> StackValue {
        StackValue::Map(self.alloc(), items)
    }

    /// Clones a value for storage by APPEND, SETITEM or VALUES.
    ///
    /// Mirrors C# `Struct.Clone` (neo-vm v3.10.1): a struct is copied because
    /// it is a mutable value type, and the copy copies the structs nested in
    /// it. Every other item, including an Array or a Map held by the copied
    /// struct, is shared with the value that was stored.
    pub(crate) fn clone_struct_for_storage(&mut self, value: &StackValue) -> StackValue {
        enum Step<'a> {
            Enter(&'a StackValue),
            Struct(u64, usize),
        }

        let mut steps = vec![Step::Enter(value)];
        let mut built: Vec<StackValue> = Vec::new();
        while let Some(step) = steps.pop() {
            match step {
                Step::Enter(StackValue::Struct(_, items)) => {
                    let id = self.alloc();
                    steps.push(Step::Struct(id, items.len()));
                    for item in items.iter().rev() {
                        steps.push(Step::Enter(item));
                    }
                }
                Step::Enter(shared) => built.push(shared.clone()),
                Step::Struct(id, len) => {
                    let children = take_tail(&mut built, len);
                    built.push(StackValue::Struct(id, children));
                }
            }
        }
        built
            .pop()
            .expect("clone_struct_for_storage builds exactly one value")
    }

    pub(crate) fn deep_clone(&mut self, value: &StackValue) -> StackValue {
        enum Step<'a> {
            Enter(&'a StackValue),
            Array(usize),
            Struct(usize),
            Map(usize),
        }

        let mut steps = vec![Step::Enter(value)];
        let mut built: Vec<StackValue> = Vec::new();
        while let Some(step) = steps.pop() {
            match step {
                Step::Enter(value) => match value {
                    StackValue::Integer(value) => built.push(StackValue::Integer(*value)),
                    StackValue::BigInteger(bytes) => {
                        built.push(StackValue::BigInteger(bytes.clone()));
                    }
                    StackValue::ByteString(bytes) => {
                        built.push(StackValue::ByteString(bytes.clone()));
                    }
                    StackValue::Boolean(value) => built.push(StackValue::Boolean(*value)),
                    StackValue::Pointer(value) => built.push(StackValue::Pointer(*value)),
                    StackValue::Buffer(_, bytes) => {
                        let bytes = bytes.clone();
                        built.push(StackValue::Buffer(self.alloc(), bytes));
                    }
                    StackValue::Interop(handle) => built.push(StackValue::Interop(*handle)),
                    StackValue::Iterator(handle) => built.push(StackValue::Iterator(*handle)),
                    StackValue::Null => built.push(StackValue::Null),
                    StackValue::Array(_, items) => {
                        steps.push(Step::Array(items.len()));
                        for item in items.iter().rev() {
                            steps.push(Step::Enter(item));
                        }
                    }
                    StackValue::Struct(_, items) => {
                        steps.push(Step::Struct(items.len()));
                        for item in items.iter().rev() {
                            steps.push(Step::Enter(item));
                        }
                    }
                    StackValue::Map(_, entries) => {
                        steps.push(Step::Map(entries.len()));
                        for (key, value) in entries.iter().rev() {
                            steps.push(Step::Enter(value));
                            steps.push(Step::Enter(key));
                        }
                    }
                },
                Step::Array(len) => {
                    let items = take_tail(&mut built, len);
                    built.push(self.array(items));
                }
                Step::Struct(len) => {
                    let items = take_tail(&mut built, len);
                    built.push(self.r#struct(items));
                }
                Step::Map(len) => {
                    let mut flat = take_tail(&mut built, len.saturating_mul(2));
                    let mut entries = Vec::with_capacity(len);
                    let mut rest = flat.drain(..);
                    while let (Some(key), Some(value)) = (rest.next(), rest.next()) {
                        entries.push((key, value));
                    }
                    built.push(self.map(entries));
                }
            }
        }
        built.pop().expect("deep_clone builds exactly one value")
    }

    /// Imports a value that crossed the host boundary.
    pub(crate) fn import_abi(&mut self, value: AbiStackValue) -> StackValue {
        enum Step {
            Enter(AbiStackValue),
            Array(usize),
            Struct(usize),
            Map(usize),
        }

        let mut steps = vec![Step::Enter(value)];
        let mut built: Vec<StackValue> = Vec::new();
        while let Some(step) = steps.pop() {
            match step {
                Step::Enter(value) => match value {
                    AbiStackValue::Integer(value) => built.push(StackValue::Integer(value)),
                    AbiStackValue::BigInteger(bytes) => {
                        built.push(StackValue::BigInteger(bytes));
                    }
                    AbiStackValue::ByteString(bytes) => built.push(StackValue::ByteString(bytes)),
                    AbiStackValue::Boolean(value) => built.push(StackValue::Boolean(value)),
                    AbiStackValue::Pointer(value) => {
                        built.push(StackValue::Pointer(value as usize));
                    }
                    AbiStackValue::Buffer(bytes) => {
                        built.push(StackValue::Buffer(self.alloc(), bytes));
                    }
                    AbiStackValue::Interop(handle) => built.push(StackValue::Interop(handle)),
                    AbiStackValue::Iterator(handle) => built.push(StackValue::Iterator(handle)),
                    AbiStackValue::Null => built.push(StackValue::Null),
                    AbiStackValue::Array(items) => {
                        steps.push(Step::Array(items.len()));
                        for item in items.into_iter().rev() {
                            steps.push(Step::Enter(item));
                        }
                    }
                    AbiStackValue::Struct(items) => {
                        steps.push(Step::Struct(items.len()));
                        for item in items.into_iter().rev() {
                            steps.push(Step::Enter(item));
                        }
                    }
                    AbiStackValue::Map(entries) => {
                        steps.push(Step::Map(entries.len()));
                        for (key, value) in entries.into_iter().rev() {
                            steps.push(Step::Enter(value));
                            steps.push(Step::Enter(key));
                        }
                    }
                },
                Step::Array(len) => {
                    let items = take_tail(&mut built, len);
                    built.push(self.array(items));
                }
                Step::Struct(len) => {
                    let items = take_tail(&mut built, len);
                    built.push(self.r#struct(items));
                }
                Step::Map(len) => {
                    let mut flat = take_tail(&mut built, len.saturating_mul(2));
                    let mut entries = Vec::with_capacity(len);
                    let mut rest = flat.drain(..);
                    while let (Some(key), Some(value)) = (rest.next(), rest.next()) {
                        entries.push((key, value));
                    }
                    built.push(self.map(entries));
                }
            }
        }
        built.pop().expect("import_abi builds exactly one value")
    }
}

pub(crate) fn structurally_equal(left: &StackValue, right: &StackValue) -> bool {
    let mut pending: Vec<(&StackValue, &StackValue)> = vec![(left, right)];
    while let Some((left, right)) = pending.pop() {
        match (left, right) {
            (StackValue::Integer(left), StackValue::Integer(right)) if left == right => {}
            (StackValue::BigInteger(left), StackValue::BigInteger(right)) if left == right => {}
            (StackValue::ByteString(left), StackValue::ByteString(right)) if left == right => {}
            (StackValue::Boolean(left), StackValue::Boolean(right)) if left == right => {}
            (StackValue::Pointer(left), StackValue::Pointer(right)) if left == right => {}
            (StackValue::Interop(left), StackValue::Interop(right)) if left == right => {}
            (StackValue::Iterator(left), StackValue::Iterator(right)) if left == right => {}
            (StackValue::Buffer(_, left), StackValue::Buffer(_, right)) if left == right => {}
            (StackValue::Null, StackValue::Null) => {}
            (StackValue::Array(_, left), StackValue::Array(_, right))
            | (StackValue::Struct(_, left), StackValue::Struct(_, right)) => {
                if left.len() != right.len() {
                    return false;
                }
                pending.extend(left.iter().zip(right.iter()));
            }
            (StackValue::Map(_, left), StackValue::Map(_, right)) => {
                if left.len() != right.len() {
                    return false;
                }
                for ((left_key, left_value), (right_key, right_value)) in
                    left.iter().zip(right.iter())
                {
                    pending.push((left_value, right_value));
                    pending.push((left_key, right_key));
                }
            }
            _ => return false,
        }
    }
    true
}
