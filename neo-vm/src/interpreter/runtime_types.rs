use crate::StackValue as AbiStackValue;
use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
    vec::Vec,
};

/// Deepest compound nesting the lightweight interpreter lets a value reach.
///
/// A primitive nests zero levels; an Array, Struct or Map nests one level more
/// than its deepest child. Clone, Drop, Debug, equality and the ABI conversions
/// of interpreter values all descend one native frame set per level, so this
/// bound is what keeps them inside the native stack. It is also the depth the
/// retained-state codec of the riscv32 guest accepts.
///
/// The canonical engine has no such limit: it only bounds the total item
/// count. Scripts that nest deeper than this fault here and not there.
pub const MAX_COMPOUND_NESTING_DEPTH: usize = 64;

/// The fault for a value that would nest deeper than
/// [`MAX_COMPOUND_NESTING_DEPTH`].
///
/// A host call that fails with this message faults the execution even inside
/// a TRY block, the same as a host call that hands back such a value.
pub const NESTING_DEPTH_ERROR: &str = "compound nesting depth exceeds maximum";
pub(crate) const ITEM_COUNT_ERROR: &str = "item count exceeds maximum stack size";

#[derive(Clone, Debug, PartialEq)]
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

impl crate::runtime::PendingExceptionValue for StackValue {
    fn from_exception_message(message: alloc::string::String) -> Self {
        Self::ByteString(message.into_bytes())
    }
}

/// Drops `values` without recursing on their nesting depth.
///
/// The drop glue derived for [`StackValue`] descends one native frame set per
/// compound level, so deeply nested collections could exhaust the native
/// stack. Children are moved onto an explicit work list instead, which leaves
/// every value shallow by the time it is dropped.
pub(crate) fn release_values(values: Vec<StackValue>) {
    let mut pending = values;
    while let Some(mut value) = pending.pop() {
        match &mut value {
            StackValue::Array(_, items) | StackValue::Struct(_, items) => pending.append(items),
            StackValue::Map(_, items) => {
                pending.reserve(items.len().saturating_mul(2));
                for (key, value) in items.drain(..) {
                    pending.push(key);
                    pending.push(value);
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
        // without recursive Drop.
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
/// The walk stops descending once `limit` is spent, so it recurses at most
/// `limit + 1` frames whatever the real depth of `value` is.
pub(crate) fn nesting_exceeds(value: &StackValue, limit: usize) -> bool {
    match value {
        StackValue::Array(_, items) | StackValue::Struct(_, items) => match limit.checked_sub(1) {
            None => true,
            Some(inner) => items.iter().any(|item| nesting_exceeds(item, inner)),
        },
        StackValue::Map(_, items) => match limit.checked_sub(1) {
            None => true,
            Some(inner) => items
                .iter()
                .any(|(key, value)| nesting_exceeds(key, inner) || nesting_exceeds(value, inner)),
        },
        StackValue::Buffer(_, _)
        | StackValue::Integer(_)
        | StackValue::BigInteger(_)
        | StackValue::ByteString(_)
        | StackValue::Boolean(_)
        | StackValue::Pointer(_)
        | StackValue::Interop(_)
        | StackValue::Iterator(_)
        | StackValue::Null => false,
    }
}

/// ABI counterpart of [`nesting_exceeds`].
pub(crate) fn abi_nesting_exceeds(value: &AbiStackValue, limit: usize) -> bool {
    match value {
        AbiStackValue::Array(items) | AbiStackValue::Struct(items) => match limit.checked_sub(1) {
            None => true,
            Some(inner) => items.iter().any(|item| abi_nesting_exceeds(item, inner)),
        },
        AbiStackValue::Map(items) => match limit.checked_sub(1) {
            None => true,
            Some(inner) => items.iter().any(|(key, value)| {
                abi_nesting_exceeds(key, inner) || abi_nesting_exceeds(value, inner)
            }),
        },
        AbiStackValue::Buffer(_)
        | AbiStackValue::Integer(_)
        | AbiStackValue::BigInteger(_)
        | AbiStackValue::ByteString(_)
        | AbiStackValue::Boolean(_)
        | AbiStackValue::Pointer(_)
        | AbiStackValue::Interop(_)
        | AbiStackValue::Iterator(_)
        | AbiStackValue::Null => false,
    }
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
/// Every stack and slot entry counts once, and every distinct Array, Struct
/// or Map reachable from the active entries adds one per child (a Map entry
/// is two children). Slots of suspended call frames arrive as
/// `saved_slot_entries`; what they reference is not walked. The walk stops as
/// soon as the count passes `limit`, so the result is exact only up to there.
pub(crate) fn counted_items(
    roots: [&[StackValue]; 4],
    saved_slot_entries: usize,
    limit: usize,
) -> usize {
    let mut count = saved_slot_entries;
    for values in roots {
        count = count.saturating_add(values.len());
    }
    if count > limit {
        return count;
    }

    let mut seen = BTreeSet::new();
    for values in roots {
        for value in values {
            count_children(value, &mut seen, &mut count, limit);
        }
    }
    count
}

fn count_children(value: &StackValue, seen: &mut BTreeSet<u64>, count: &mut usize, limit: usize) {
    if *count > limit {
        return;
    }
    match value {
        StackValue::Array(id, items) | StackValue::Struct(id, items) => {
            if seen.insert(*id) {
                *count = count.saturating_add(items.len());
                for item in items {
                    count_children(item, seen, count, limit);
                }
            }
        }
        StackValue::Map(id, items) => {
            if seen.insert(*id) {
                *count = count.saturating_add(items.len().saturating_mul(2));
                for (key, value) in items {
                    count_children(key, seen, count, limit);
                    count_children(value, seen, count, limit);
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

#[derive(Default)]
pub(crate) struct CompoundIds {
    next: u64,
    /// Set once interpreter ids stop standing for the canonical engine's
    /// object identities, or once the canonical reference counter is known to
    /// undercount. [`counted_items`] is not comparable from then on.
    count_diverged: bool,
}

impl CompoundIds {
    fn alloc(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    pub(crate) fn count_diverged(&self) -> bool {
        self.count_diverged
    }

    /// Records the values the interpreter kept from an import.
    ///
    /// ABI values carry no object identity, so every imported compound gets
    /// fresh ids whether or not the host shares it with another value.
    pub(crate) fn note_imported(&mut self, values: &[StackValue]) {
        if values.iter().any(|value| {
            matches!(
                value,
                StackValue::Array(..) | StackValue::Struct(..) | StackValue::Map(..)
            )
        }) {
            self.count_diverged = true;
        }
    }

    /// Clones `value` for storage by APPEND or SETITEM.
    ///
    /// The canonical engine records a child stored into an existing compound
    /// as a parent reference only, and from then on counts fewer items than
    /// are reachable whenever that child is itself tracked.
    pub(crate) fn clone_for_mutation(&mut self, value: &StackValue) -> StackValue {
        if compound_id(value).is_some() {
            self.count_diverged = true;
        }
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

    pub(crate) fn buffer(&mut self, bytes: Vec<u8>) -> StackValue {
        StackValue::Buffer(self.alloc(), bytes)
    }

    pub(crate) fn clone_struct_for_storage(&mut self, value: &StackValue) -> StackValue {
        match value {
            StackValue::Struct(_, _) => self.deep_clone(value),
            _ => value.clone(),
        }
    }

    pub(crate) fn deep_clone(&mut self, value: &StackValue) -> StackValue {
        if matches!(
            value,
            StackValue::Array(..) | StackValue::Struct(..) | StackValue::Map(..)
        ) {
            // The copy gets ids of its own, so the two no longer count as one
            // object the way they may in the canonical engine.
            self.count_diverged = true;
        }
        match value {
            StackValue::Integer(value) => StackValue::Integer(*value),
            StackValue::BigInteger(value) => StackValue::BigInteger(value.clone()),
            StackValue::ByteString(value) => StackValue::ByteString(value.clone()),
            StackValue::Boolean(value) => StackValue::Boolean(*value),
            StackValue::Pointer(value) => StackValue::Pointer(*value),
            StackValue::Array(_, items) => {
                let mut cloned = Vec::with_capacity(items.len());
                for item in items {
                    cloned.push(self.deep_clone(item));
                }
                self.array(cloned)
            }
            StackValue::Struct(_, items) => {
                let mut cloned = Vec::with_capacity(items.len());
                for item in items {
                    cloned.push(self.deep_clone(item));
                }
                self.r#struct(cloned)
            }
            StackValue::Map(_, items) => {
                let mut cloned = Vec::with_capacity(items.len());
                for (key, value) in items {
                    cloned.push((self.deep_clone(key), self.deep_clone(value)));
                }
                self.map(cloned)
            }
            StackValue::Buffer(_, bytes) => self.buffer(bytes.clone()),
            StackValue::Interop(handle) => StackValue::Interop(*handle),
            StackValue::Iterator(handle) => StackValue::Iterator(*handle),
            StackValue::Null => StackValue::Null,
        }
    }

    /// Imports a value that crossed the host boundary.
    ///
    /// Callers that keep an imported compound report it through
    /// [`Self::note_imported`].
    pub(crate) fn import_abi(&mut self, value: AbiStackValue) -> StackValue {
        match value {
            AbiStackValue::Integer(value) => StackValue::Integer(value),
            AbiStackValue::BigInteger(value) => StackValue::BigInteger(value),
            AbiStackValue::ByteString(value) => StackValue::ByteString(value),
            AbiStackValue::Boolean(value) => StackValue::Boolean(value),
            AbiStackValue::Pointer(value) => StackValue::Pointer(value as usize),
            AbiStackValue::Array(items) => {
                let len = items.len();
                let mut imported = Vec::with_capacity(len);
                for item in items {
                    imported.push(self.import_abi(item));
                }
                self.array(imported)
            }
            AbiStackValue::Struct(items) => {
                let len = items.len();
                let mut imported = Vec::with_capacity(len);
                for item in items {
                    imported.push(self.import_abi(item));
                }
                self.r#struct(imported)
            }
            AbiStackValue::Map(items) => {
                let len = items.len();
                let mut imported = Vec::with_capacity(len);
                for (key, value) in items {
                    imported.push((self.import_abi(key), self.import_abi(value)));
                }
                self.map(imported)
            }
            AbiStackValue::Buffer(value) => self.buffer(value),
            AbiStackValue::Interop(handle) => StackValue::Interop(handle),
            AbiStackValue::Iterator(handle) => StackValue::Iterator(handle),
            AbiStackValue::Null => StackValue::Null,
        }
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
    match value {
        StackValue::Integer(value) => AbiStackValue::Integer(*value),
        StackValue::BigInteger(value) => AbiStackValue::BigInteger(clone_bytes(value)),
        StackValue::ByteString(value) => AbiStackValue::ByteString(clone_bytes(value)),
        StackValue::Boolean(value) => AbiStackValue::Boolean(*value),
        StackValue::Pointer(value) => AbiStackValue::Pointer(*value as i64),
        StackValue::Array(_, items) => {
            let mut converted = Vec::with_capacity(items.len());
            for item in items {
                converted.push(to_abi_value(item));
            }
            AbiStackValue::Array(converted)
        }
        StackValue::Struct(_, items) => {
            let mut converted = Vec::with_capacity(items.len());
            for item in items {
                converted.push(to_abi_value(item));
            }
            AbiStackValue::Struct(converted)
        }
        StackValue::Map(_, items) => {
            let mut converted = Vec::with_capacity(items.len());
            for (key, value) in items {
                converted.push((to_abi_value(key), to_abi_value(value)));
            }
            AbiStackValue::Map(converted)
        }
        StackValue::Buffer(_, bytes) => AbiStackValue::Buffer(clone_bytes(bytes)),
        StackValue::Interop(handle) => AbiStackValue::Interop(*handle),
        StackValue::Iterator(handle) => AbiStackValue::Iterator(*handle),
        StackValue::Null => AbiStackValue::Null,
    }
}

#[inline]
pub(crate) fn into_abi_value(value: StackValue) -> AbiStackValue {
    match value {
        StackValue::Integer(value) => AbiStackValue::Integer(value),
        StackValue::BigInteger(value) => AbiStackValue::BigInteger(value),
        StackValue::ByteString(value) => AbiStackValue::ByteString(value),
        StackValue::Boolean(value) => AbiStackValue::Boolean(value),
        StackValue::Pointer(value) => AbiStackValue::Pointer(value as i64),
        StackValue::Array(_, items) => {
            AbiStackValue::Array(items.into_iter().map(into_abi_value).collect())
        }
        StackValue::Struct(_, items) => {
            AbiStackValue::Struct(items.into_iter().map(into_abi_value).collect())
        }
        StackValue::Map(_, items) => AbiStackValue::Map(
            items
                .into_iter()
                .map(|(key, value)| (into_abi_value(key), into_abi_value(value)))
                .collect(),
        ),
        StackValue::Buffer(_, bytes) => AbiStackValue::Buffer(bytes),
        StackValue::Interop(handle) => AbiStackValue::Interop(handle),
        StackValue::Iterator(handle) => AbiStackValue::Iterator(handle),
        StackValue::Null => AbiStackValue::Null,
    }
}

pub(crate) fn structurally_equal(left: &StackValue, right: &StackValue) -> bool {
    match (left, right) {
        (StackValue::Integer(l), StackValue::Integer(r)) => l == r,
        (StackValue::BigInteger(l), StackValue::BigInteger(r)) => l == r,
        (StackValue::ByteString(l), StackValue::ByteString(r)) => l == r,
        (StackValue::Boolean(l), StackValue::Boolean(r)) => l == r,
        (StackValue::Pointer(l), StackValue::Pointer(r)) => l == r,
        (StackValue::Null, StackValue::Null) => true,
        (StackValue::Interop(l), StackValue::Interop(r)) => l == r,
        (StackValue::Iterator(l), StackValue::Iterator(r)) => l == r,
        (StackValue::Buffer(_, l), StackValue::Buffer(_, r)) => l == r,
        (StackValue::Array(_, l), StackValue::Array(_, r))
        | (StackValue::Struct(_, l), StackValue::Struct(_, r)) => {
            l.len() == r.len()
                && l.iter()
                    .zip(r.iter())
                    .all(|(l, r)| structurally_equal(l, r))
        }
        (StackValue::Map(_, l), StackValue::Map(_, r)) => {
            l.len() == r.len()
                && l.iter().zip(r.iter()).all(|((lk, lv), (rk, rv))| {
                    structurally_equal(lk, rk) && structurally_equal(lv, rv)
                })
        }
        _ => false,
    }
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
    if compound_id(value) == Some(target_id) {
        return true;
    }
    match value {
        StackValue::Array(_, items) | StackValue::Struct(_, items) => items
            .iter()
            .any(|item| contains_compound_id(item, target_id)),
        StackValue::Map(_, items) => items
            .iter()
            .any(|(k, v)| contains_compound_id(k, target_id) || contains_compound_id(v, target_id)),
        _ => false,
    }
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
    if compound_id(source).is_some() {
        for target in targets.iter_mut() {
            fits &= replace_alias(target, source, MAX_COMPOUND_NESTING_DEPTH);
        }
    }

    match source {
        StackValue::Array(_, items) | StackValue::Struct(_, items) => {
            for item in items {
                fits &= propagate_alias_from_source(targets, item);
            }
        }
        StackValue::Map(_, items) => {
            for (key, value) in items {
                fits &= propagate_alias_from_source(targets, key);
                fits &= propagate_alias_from_source(targets, value);
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
    fits
}

/// Replaces every alias of `updated` inside `target`, which may still nest
/// `room` compound levels. Returns `false`, leaving that alias as it was, when
/// the replacement would nest deeper than `room`.
fn replace_alias(target: &mut StackValue, updated: &StackValue, room: usize) -> bool {
    let target_id = compound_id(target);
    if target_id.is_some() && target_id == compound_id(updated) {
        if nesting_exceeds(updated, room) {
            return false;
        }
        *target = updated.clone();
        return true;
    }

    let inner = room.saturating_sub(1);
    let mut fits = true;
    match target {
        StackValue::Array(_, items) | StackValue::Struct(_, items) => {
            for item in items {
                fits &= replace_alias(item, updated, inner);
            }
        }
        StackValue::Map(_, items) => {
            for (key, value) in items {
                fits &= replace_alias(key, updated, inner);
                fits &= replace_alias(value, updated, inner);
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
    fits
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
}
