use crate::StackValue as AbiStackValue;
use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::fmt;

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

/// ABI counterpart of [`release_values`].
pub(crate) fn release_abi_values(values: Vec<AbiStackValue>) {
    let mut pending = values;
    while let Some(mut value) = pending.pop() {
        match &mut value {
            AbiStackValue::Array(items) | AbiStackValue::Struct(items) => pending.append(items),
            AbiStackValue::Map(items) => {
                pending.reserve(items.len().saturating_mul(2));
                for (key, value) in items.drain(..) {
                    pending.push(key);
                    pending.push(value);
                }
            }
            AbiStackValue::Buffer(_)
            | AbiStackValue::Integer(_)
            | AbiStackValue::BigInteger(_)
            | AbiStackValue::ByteString(_)
            | AbiStackValue::Boolean(_)
            | AbiStackValue::Pointer(_)
            | AbiStackValue::Interop(_)
            | AbiStackValue::Iterator(_)
            | AbiStackValue::Null => {}
        }
    }
}

/// Disposes of ABI values that were lent to a host call and not imported back.
#[inline]
pub(crate) fn discard_abi_values(values: Vec<AbiStackValue>) {
    if cfg!(target_arch = "riscv32") {
        // The PolkaVM/riscv32 guest leaks these to avoid talc free-list
        // corruption after a failed host call. Its allocator is reset per
        // execution, so nothing accumulates.
        core::mem::forget(values);
    } else {
        // The host only borrowed the values, so they are still owned here and
        // nothing else frees them.
        release_abi_values(values);
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

/// ABI counterpart of [`nesting_exceeds`].
pub(crate) fn abi_nesting_exceeds(value: &AbiStackValue, limit: usize) -> bool {
    let mut pending: Vec<(&AbiStackValue, usize)> = vec![(value, limit)];
    while let Some((value, limit)) = pending.pop() {
        match value {
            AbiStackValue::Array(items) | AbiStackValue::Struct(items) => {
                let Some(inner) = limit.checked_sub(1) else {
                    return true;
                };
                pending.extend(items.iter().map(|item| (item, inner)));
            }
            AbiStackValue::Map(entries) => {
                let Some(inner) = limit.checked_sub(1) else {
                    return true;
                };
                for (key, value) in entries {
                    pending.push((key, inner));
                    pending.push((value, inner));
                }
            }
            AbiStackValue::Buffer(_)
            | AbiStackValue::Integer(_)
            | AbiStackValue::BigInteger(_)
            | AbiStackValue::ByteString(_)
            | AbiStackValue::Boolean(_)
            | AbiStackValue::Pointer(_)
            | AbiStackValue::Interop(_)
            | AbiStackValue::Iterator(_)
            | AbiStackValue::Null => {}
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

/// Rejects values supplied from outside the interpreter that nest deeper than
/// it allows, before any of them is imported.
pub(crate) fn ensure_abi_nesting(values: &[AbiStackValue]) -> Result<(), String> {
    if values
        .iter()
        .any(|value| abi_nesting_exceeds(value, MAX_COMPOUND_NESTING_DEPTH))
    {
        return Err(NESTING_DEPTH_ERROR.to_string());
    }
    Ok(())
}

/// Counts the items the canonical reference counter holds for this state.
///
/// Every entry of every root counts once, and every distinct Array, Struct or
/// Map reachable from those entries adds one per child (a Map entry is two
/// children). The roots are the active stack, locals, args and static fields
/// plus the slots of every suspended call frame, which is exactly the set of
/// references the canonical reference counter holds while a call is in
/// progress.
///
/// The walk stops as soon as the count passes `limit`, so the result is exact
/// only up to there.
pub(crate) fn counted_items<'a>(
    roots: impl IntoIterator<Item = &'a [StackValue]>,
    limit: usize,
) -> usize {
    let mut count = 0usize;
    let mut pending: Vec<&StackValue> = Vec::new();
    for values in roots {
        count = count.saturating_add(values.len());
        pending.extend(values.iter());
    }
    if count > limit {
        return count;
    }

    let mut seen = BTreeSet::new();
    while let Some(value) = pending.pop() {
        match value {
            StackValue::Array(id, items) | StackValue::Struct(id, items) => {
                if seen.insert(*id) {
                    count = count.saturating_add(items.len());
                    if count > limit {
                        return count;
                    }
                    pending.extend(items.iter());
                }
            }
            StackValue::Map(id, entries) => {
                if seen.insert(*id) {
                    count = count.saturating_add(entries.len().saturating_mul(2));
                    if count > limit {
                        return count;
                    }
                    for (key, value) in entries {
                        pending.push(key);
                        pending.push(value);
                    }
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
    count
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

#[inline]
pub(crate) fn to_abi_stack(stack: &[StackValue]) -> Vec<AbiStackValue> {
    let mut abi = Vec::with_capacity(stack.len().max(8));
    for item in stack {
        abi.push(to_abi_value(item));
    }
    abi
}

#[inline]
pub(crate) fn to_abi_value(value: &StackValue) -> AbiStackValue {
    enum Step<'a> {
        Enter(&'a StackValue),
        Array(usize),
        Struct(usize),
        Map(usize),
    }

    let mut steps = vec![Step::Enter(value)];
    let mut built: Vec<AbiStackValue> = Vec::new();
    while let Some(step) = steps.pop() {
        match step {
            Step::Enter(value) => match value {
                StackValue::Integer(value) => built.push(AbiStackValue::Integer(*value)),
                StackValue::BigInteger(bytes) => {
                    built.push(AbiStackValue::BigInteger(clone_bytes(bytes)));
                }
                StackValue::ByteString(bytes) => {
                    built.push(AbiStackValue::ByteString(clone_bytes(bytes)));
                }
                StackValue::Boolean(value) => built.push(AbiStackValue::Boolean(*value)),
                StackValue::Pointer(value) => built.push(AbiStackValue::Pointer(*value as i64)),
                StackValue::Buffer(_, bytes) => {
                    built.push(AbiStackValue::Buffer(clone_bytes(bytes)));
                }
                StackValue::Interop(handle) => built.push(AbiStackValue::Interop(*handle)),
                StackValue::Iterator(handle) => built.push(AbiStackValue::Iterator(*handle)),
                StackValue::Null => built.push(AbiStackValue::Null),
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
                built.push(AbiStackValue::Array(items));
            }
            Step::Struct(len) => {
                let items = take_tail(&mut built, len);
                built.push(AbiStackValue::Struct(items));
            }
            Step::Map(len) => {
                let mut flat = take_tail(&mut built, len.saturating_mul(2));
                let mut entries = Vec::with_capacity(len);
                let mut rest = flat.drain(..);
                while let (Some(key), Some(value)) = (rest.next(), rest.next()) {
                    entries.push((key, value));
                }
                built.push(AbiStackValue::Map(entries));
            }
        }
    }
    built.pop().expect("to_abi_value builds exactly one value")
}

#[inline]
pub(crate) fn into_abi_value(value: StackValue) -> AbiStackValue {
    enum Step {
        Enter(StackValue),
        Array(usize),
        Struct(usize),
        Map(usize),
    }

    let mut steps = vec![Step::Enter(value)];
    let mut built: Vec<AbiStackValue> = Vec::new();
    while let Some(step) = steps.pop() {
        match step {
            Step::Enter(mut value) => match &mut value {
                StackValue::Integer(inner) => built.push(AbiStackValue::Integer(*inner)),
                StackValue::BigInteger(bytes) => {
                    built.push(AbiStackValue::BigInteger(core::mem::take(bytes)));
                }
                StackValue::ByteString(bytes) => {
                    built.push(AbiStackValue::ByteString(core::mem::take(bytes)));
                }
                StackValue::Boolean(inner) => built.push(AbiStackValue::Boolean(*inner)),
                StackValue::Pointer(inner) => built.push(AbiStackValue::Pointer(*inner as i64)),
                StackValue::Buffer(_, bytes) => {
                    built.push(AbiStackValue::Buffer(core::mem::take(bytes)));
                }
                StackValue::Interop(handle) => built.push(AbiStackValue::Interop(*handle)),
                StackValue::Iterator(handle) => built.push(AbiStackValue::Iterator(*handle)),
                StackValue::Null => built.push(AbiStackValue::Null),
                StackValue::Array(_, items) => {
                    let items = core::mem::take(items);
                    steps.push(Step::Array(items.len()));
                    for item in items.into_iter().rev() {
                        steps.push(Step::Enter(item));
                    }
                }
                StackValue::Struct(_, items) => {
                    let items = core::mem::take(items);
                    steps.push(Step::Struct(items.len()));
                    for item in items.into_iter().rev() {
                        steps.push(Step::Enter(item));
                    }
                }
                StackValue::Map(_, entries) => {
                    let entries = core::mem::take(entries);
                    let mut flat = Vec::with_capacity(entries.len().saturating_mul(2));
                    for (key, value) in entries {
                        flat.push(key);
                        flat.push(value);
                    }
                    steps.push(Step::Map(flat.len() / 2));
                    for value in flat.into_iter().rev() {
                        steps.push(Step::Enter(value));
                    }
                }
            },
            Step::Array(len) => {
                let items = take_tail(&mut built, len);
                built.push(AbiStackValue::Array(items));
            }
            Step::Struct(len) => {
                let items = take_tail(&mut built, len);
                built.push(AbiStackValue::Struct(items));
            }
            Step::Map(len) => {
                let mut flat = take_tail(&mut built, len.saturating_mul(2));
                let mut entries = Vec::with_capacity(len);
                let mut rest = flat.drain(..);
                while let (Some(key), Some(value)) = (rest.next(), rest.next()) {
                    entries.push((key, value));
                }
                built.push(AbiStackValue::Map(entries));
            }
        }
    }
    built.pop().expect("into_abi_value builds exactly one value")
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

#[inline]
fn clone_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(bytes);
    out
}

#[inline]
pub(crate) fn compound_id(value: &StackValue) -> Option<u64> {
    match value {
        StackValue::Array(id, _)
        | StackValue::Struct(id, _)
        | StackValue::Map(id, _)
        | StackValue::Buffer(id, _) => Some(*id),
        _ => None,
    }
}

pub(crate) fn find_affected_indices(target_id: u64, stack: &[StackValue]) -> Vec<usize> {
    let mut indices = Vec::with_capacity(stack.len().min(8));
    for (idx, value) in stack.iter().enumerate() {
        if contains_compound_id(value, target_id) {
            indices.push(idx);
        }
    }
    indices
}

fn contains_compound_id(value: &StackValue, target_id: u64) -> bool {
    let mut pending: Vec<&StackValue> = vec![value];
    while let Some(value) = pending.pop() {
        if compound_id(value) == Some(target_id) {
            return true;
        }
        match value {
            StackValue::Array(_, items) | StackValue::Struct(_, items) => {
                pending.extend(items.iter());
            }
            StackValue::Map(_, entries) => {
                for (key, value) in entries {
                    pending.push(key);
                    pending.push(value);
                }
            }
            _ => {}
        }
    }
    false
}

/// Rewrites every alias of `updated` in the active state.
///
/// Fails, leaving the offending alias untouched, when an alias sits so deep
/// that taking the update would nest it past [`MAX_COMPOUND_NESTING_DEPTH`].
pub(crate) fn propagate_update(
    updated: &StackValue,
    stack: &mut [StackValue],
    locals: &mut [StackValue],
    args: &mut [StackValue],
    static_fields: &mut [StackValue],
    affected_stack_indices: Option<&[usize]>,
) -> Result<(), String> {
    let mut fits = true;
    match affected_stack_indices {
        Some(indices) if !indices.is_empty() => {
            for &idx in indices {
                if idx < stack.len() {
                    fits &= replace_alias(&mut stack[idx], updated, MAX_COMPOUND_NESTING_DEPTH);
                }
            }
        }
        Some(_) => {
            // Empty indices - skip stack iteration
        }
        None => {
            for value in stack {
                fits &= replace_alias(value, updated, MAX_COMPOUND_NESTING_DEPTH);
            }
        }
    }
    for value in locals {
        fits &= replace_alias(value, updated, MAX_COMPOUND_NESTING_DEPTH);
    }
    for value in args {
        fits &= replace_alias(value, updated, MAX_COMPOUND_NESTING_DEPTH);
    }
    for value in static_fields {
        fits &= replace_alias(value, updated, MAX_COMPOUND_NESTING_DEPTH);
    }
    if fits {
        Ok(())
    } else {
        Err(NESTING_DEPTH_ERROR.to_string())
    }
}

pub(crate) fn propagate_aliases_from_sources(
    targets: &mut [StackValue],
    sources: &[StackValue],
) -> Result<(), String> {
    let mut fits = true;
    for source in sources {
        fits &= propagate_alias_from_source(targets, source);
    }
    if fits {
        Ok(())
    } else {
        Err(NESTING_DEPTH_ERROR.to_string())
    }
}

fn propagate_alias_from_source(targets: &mut [StackValue], source: &StackValue) -> bool {
    let mut fits = true;
    let mut pending: Vec<&StackValue> = vec![source];
    while let Some(source) = pending.pop() {
        if compound_id(source).is_some() {
            for target in targets.iter_mut() {
                fits &= replace_alias(target, source, MAX_COMPOUND_NESTING_DEPTH);
            }
        }
        match source {
            StackValue::Array(_, items) | StackValue::Struct(_, items) => {
                pending.extend(items.iter());
            }
            StackValue::Map(_, entries) => {
                for (key, value) in entries {
                    pending.push(key);
                    pending.push(value);
                }
            }
            _ => {}
        }
    }
    fits
}

/// Replaces every alias of `updated` inside `target`, which may still nest
/// `room` compound levels. Returns `false`, leaving that alias as it was, when
/// the replacement would nest deeper than `room`.
fn replace_alias(target: &mut StackValue, updated: &StackValue, room: usize) -> bool {
    let updated_id = compound_id(updated);
    if updated_id.is_some() && compound_id(target) == updated_id {
        if nesting_exceeds(updated, room) {
            return false;
        }
        *target = updated.clone();
        return true;
    }

    let mut fits = true;
    let mut pending: Vec<(&mut StackValue, usize)> = Vec::new();
    queue_children(target, room, &mut pending);
    while let Some((node, room)) = pending.pop() {
        if updated_id.is_some() && compound_id(node) == updated_id {
            if nesting_exceeds(updated, room) {
                fits = false;
            } else {
                *node = updated.clone();
            }
            continue;
        }
        queue_children(node, room, &mut pending);
    }
    fits
}

/// Pushes the children of `node`, each with one level less room, onto `pending`.
fn queue_children<'a>(
    node: &'a mut StackValue,
    room: usize,
    pending: &mut Vec<(&'a mut StackValue, usize)>,
) {
    let inner = room.saturating_sub(1);
    match node {
        StackValue::Array(_, items) | StackValue::Struct(_, items) => {
            for item in items.iter_mut() {
                pending.push((item, inner));
            }
        }
        StackValue::Map(_, entries) => {
            for (key, value) in entries.iter_mut() {
                pending.push((key, inner));
                pending.push((value, inner));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn discarding_abi_values_does_not_recurse_on_nesting_depth() {
        const DEPTH: usize = 100_000;
        // Far too small for one native frame per nesting level.
        const THREAD_STACK_BYTES: usize = 256 * 1024;

        let nested = |wrap: fn(AbiStackValue) -> AbiStackValue| {
            let mut value = AbiStackValue::ByteString(vec![0xaa; 8]);
            for _ in 0..DEPTH {
                value = wrap(value);
            }
            value
        };
        let values = vec![
            nested(|inner| AbiStackValue::Array(vec![inner])),
            nested(|inner| AbiStackValue::Struct(vec![inner])),
            nested(|inner| AbiStackValue::Map(vec![(AbiStackValue::Integer(0), inner)])),
            nested(|inner| AbiStackValue::Map(vec![(inner, AbiStackValue::Integer(0))])),
            AbiStackValue::Buffer(vec![0xbb; 8]),
        ];

        std::thread::Builder::new()
            .stack_size(THREAD_STACK_BYTES)
            .spawn(move || discard_abi_values(values))
            .expect("spawn small-stack thread")
            .join()
            .expect("release completes");
    }

    /// A single-root chain `levels` deep that mixes every compound kind.
    fn nested(levels: usize) -> StackValue {
        let mut value = StackValue::Struct(0, Vec::new());
        let mut next_id = 1u64;
        for level in 1..levels {
            value = match level % 3 {
                0 => {
                    next_id += 1;
                    StackValue::Array(next_id, vec![value])
                }
                1 => {
                    next_id += 1;
                    StackValue::Struct(next_id, vec![value])
                }
                _ => {
                    next_id += 1;
                    StackValue::Map(next_id, vec![(StackValue::Integer(0), value)])
                }
            };
        }
        value
    }

    /// Every walk over a legal deepest value has to fit a quarter of the node's
    /// stack, because none of them recurses any more.
    #[test]
    fn every_walk_over_a_maximum_depth_value_fits_a_small_stack() {
        const THREAD_STACK_BYTES: usize = 256 * 1024;

        std::thread::Builder::new()
            .stack_size(THREAD_STACK_BYTES)
            .spawn(|| {
                let levels = MAX_COMPOUND_NESTING_DEPTH;
                let value = nested(levels);

                // One entry per level, plus a second item for every map entry,
                // which the canonical counter also counts twice.
                let maps = (1..levels).filter(|level| level % 3 == 2).count();
                let items = 1 + (levels - 1) + maps;
                assert!(items > MAX_COMPOUND_NESTING_DEPTH);
                assert_eq!(
                    counted_items([core::slice::from_ref(&value)], usize::MAX),
                    items
                );
                assert!(
                    counted_items([core::slice::from_ref(&value)], MAX_COMPOUND_NESTING_DEPTH)
                        > MAX_COMPOUND_NESTING_DEPTH
                );
                assert!(!nesting_exceeds(&value, MAX_COMPOUND_NESTING_DEPTH));
                assert!(nesting_exceeds(&value, MAX_COMPOUND_NESTING_DEPTH - 1));
                assert!(contains_compound_id(&value, 0));
                assert_eq!(find_affected_indices(0, core::slice::from_ref(&value)), vec![0]);

                let copy = value.clone();
                assert_eq!(copy, value);
                assert!(structurally_equal(&copy, &value));
                assert!(!format!("{value:?}").is_empty());

                let mut ids = CompoundIds::default();
                let cloned = ids.deep_clone(&value);
                assert!(structurally_equal(&cloned, &value));
                assert_ne!(compound_id(&cloned), compound_id(&value));

                let abi = to_abi_value(&value);
                assert!(!abi_nesting_exceeds(&abi, levels));
                let imported = ids.import_abi(abi);
                assert!(structurally_equal(&imported, &value));

                // Alias replacement touches every level and must not recurse.
                let mut target = value.clone();
                let replacement = StackValue::Array(999, vec![StackValue::Null]);
                assert!(replace_alias(&mut target, &replacement, levels));

                // Dropping the whole set happens on this small stack too.
                drop((value, copy, cloned, imported, target));
            })
            .expect("spawn small-stack thread")
            .join()
            .expect("every walk fits a 256 KiB stack");
    }

    #[test]
    fn counted_items_matches_the_number_of_references() {
        let mut ids = CompoundIds::default();
        let inner = ids.array(vec![StackValue::Integer(1), StackValue::Integer(2)]);
        let shared = ids.array(vec![inner.clone(), inner.clone(), StackValue::Null]);
        // Three root entries, the outer array contributes three children, the
        // shared inner array contributes two, and the duplicate alias adds
        // nothing because its id is already counted.
        let roots = vec![shared.clone(), shared, StackValue::Null];
        assert_eq!(
            counted_items([roots.as_slice()], MAX_COMPOUND_NESTING_DEPTH),
            3 + 3 + 2
        );
    }

    #[test]
    fn counted_items_counts_suspended_slot_entries_too() {
        let mut ids = CompoundIds::default();
        let value = ids.array(vec![StackValue::Integer(1), StackValue::Integer(2)]);
        let slots = vec![value.clone(), value, StackValue::Null];
        assert_eq!(
            counted_items(
                [
                    &[][..],
                    slots.as_slice(),
                ],
                MAX_COMPOUND_NESTING_DEPTH
            ),
            3 + 2
        );
    }
}
