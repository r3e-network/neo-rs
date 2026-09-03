//! Runtime stack adapters for collection opcodes.

use crate::semantics::collections as rules;
use crate::{
    runtime::{
        push_bool_result, push_i64_result, push_value_result, push_values_result, RuntimeStack,
    },
    StackValue,
};
use alloc::{string::String, vec::Vec};

pub fn new_array_0<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    push_value_result(runtime, rules::new_array(0));
}

pub fn new_array<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let count = runtime.pop_i64();
    push_value_result(runtime, rules::new_array(count));
}

pub fn new_array_t<R: RuntimeStack + ?Sized>(runtime: &mut R, type_byte: u8) {
    let count = runtime.pop_i64();
    push_value_result(runtime, rules::new_array_t(count, type_byte));
}

pub fn new_struct_0<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    push_value_result(runtime, rules::new_struct(0));
}

pub fn new_struct<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let count = runtime.pop_i64();
    push_value_result(runtime, rules::new_struct(count));
}

pub fn new_map<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    runtime.push_value(rules::new_map());
}

pub fn new_buffer<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let size = runtime.pop_i64();
    push_value_result(runtime, rules::new_buffer(size));
}

pub fn append<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let value = runtime.pop_value();
    apply_top_mut(
        runtime,
        "APPEND: top-1 is not an array or struct",
        |collection| rules::append(collection, value),
    );
}

pub fn set_item<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let value = runtime.pop_value();
    let key = runtime.pop_value();
    apply_top_mut(runtime, "SETITEM: not a collection", |collection| {
        rules::set_item(collection, key, value)
    });
}

pub fn pick_item<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let key = runtime.pop_value();
    let collection = runtime.pop_value();
    match rules::pick_item(&collection, &key) {
        Ok(value) => runtime.push_value(value),
        Err(message) => runtime.fault(&message),
    }
}

pub fn remove<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let key = runtime.pop_value();
    apply_top_mut(runtime, "REMOVE: not a collection", |collection| {
        rules::remove(collection, &key)
    });
}

pub fn size<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let value = runtime.pop_value();
    push_i64_result(runtime, rules::size(&value));
}

pub fn has_key<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let key = runtime.pop_value();
    let collection = runtime.pop_value();
    push_bool_result(runtime, rules::has_key(&collection, &key));
}

pub fn keys<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let value = runtime.pop_value();
    push_value_result(runtime, rules::keys(value));
}

pub fn values<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let value = runtime.pop_value();
    push_value_result(runtime, rules::values(value));
}

pub fn pack<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let Some(count) = pop_non_negative_count(runtime, "PACK: negative count") else {
        return;
    };
    let items = pop_values(runtime, count);
    runtime.push_value(rules::pack(items));
}

pub fn unpack<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let value = runtime.pop_value();
    push_values_result(runtime, rules::unpack(value));
}

pub fn reverse_items<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_top_mut(
        runtime,
        "REVERSEITEMS: not an array or struct",
        rules::reverse_items,
    );
}

pub fn clear_items<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_top_mut(runtime, "CLEARITEMS: not a collection", rules::clear_items);
}

pub fn pop_item<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let value = runtime.pop_value();
    push_values_result(runtime, rules::pop_item(value));
}

pub fn pack_struct<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let Some(count) = pop_non_negative_count(runtime, "PACKSTRUCT: negative count") else {
        return;
    };
    let items = pop_values(runtime, count);
    runtime.push_value(rules::pack_struct(items));
}

pub fn pack_map<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let Some(count) = pop_non_negative_count(runtime, "PACKMAP: negative count") else {
        return;
    };
    let pairs = pop_pairs(runtime, count);
    runtime.push_value(rules::pack_map(pairs));
}

fn pop_non_negative_count<R: RuntimeStack + ?Sized>(
    runtime: &mut R,
    error: &'static str,
) -> Option<usize> {
    match rules::non_negative_count(runtime.pop_i64(), error) {
        Ok(count) => Some(count),
        Err(message) => {
            runtime.fault(&message);
            None
        }
    }
}

fn pop_values<R: RuntimeStack + ?Sized>(runtime: &mut R, count: usize) -> Vec<StackValue> {
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(runtime.pop_value());
    }
    items.reverse();
    items
}

fn pop_pairs<R: RuntimeStack + ?Sized>(
    runtime: &mut R,
    count: usize,
) -> Vec<(StackValue, StackValue)> {
    let mut pairs = Vec::with_capacity(count);
    for _ in 0..count {
        let value = runtime.pop_value();
        let key = runtime.pop_value();
        pairs.push((key, value));
    }
    pairs.reverse();
    pairs
}

fn apply_top_mut<R, F>(runtime: &mut R, missing_error: &'static str, apply: F)
where
    R: RuntimeStack + ?Sized,
    F: FnOnce(&mut StackValue) -> Result<(), String>,
{
    super::apply_or_fault(runtime, |runtime| match runtime.top_value_mut() {
        Some(collection) => apply(collection),
        None => Err(missing_error.into()),
    });
}
