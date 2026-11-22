use super::take_tail;
use super::*;

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
    built
        .pop()
        .expect("into_abi_value builds exactly one value")
}
#[inline]
fn clone_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(bytes);
    out
}
