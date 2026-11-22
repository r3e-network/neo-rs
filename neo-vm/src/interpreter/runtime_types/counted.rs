use super::*;
use alloc::collections::BTreeSet;

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
