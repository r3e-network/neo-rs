use super::*;

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
pub(super) fn contains_compound_id(value: &StackValue, target_id: u64) -> bool {
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
pub(super) fn replace_alias(target: &mut StackValue, updated: &StackValue, room: usize) -> bool {
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
