//! Runtime-level byte string and buffer opcode adapters.

use crate::runtime::{push_value_result, RuntimeStack};
use crate::{semantics::splice as splice_rules, StackValue};

pub fn cat<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let right = runtime.pop_value();
    let left = runtime.pop_value();
    push_value_result(runtime, splice_rules::cat_values(&left, &right));
}

pub fn substr<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let count = runtime.pop_i64();
    let index = runtime.pop_i64();
    let value = runtime.pop_value();
    push_value_result(runtime, splice_rules::substr_value(&value, index, count));
}

pub fn left<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let count = runtime.pop_i64();
    let value = runtime.pop_value();
    push_value_result(runtime, splice_rules::left_value(&value, count));
}

pub fn right<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let count = runtime.pop_i64();
    let value = runtime.pop_value();
    push_value_result(runtime, splice_rules::right_value(&value, count));
}

pub fn memcpy<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let count = runtime.pop_i64();
    let source_index = runtime.pop_i64();
    let source = runtime.pop_value();
    let destination_index = runtime.pop_i64();
    let result = match runtime.top_value_mut() {
        Some(StackValue::Buffer(buffer)) => {
            splice_rules::memcpy_bytes(buffer, destination_index, &source, source_index, count)
        }
        _ => Err("MEMCPY: destination is not a Buffer".into()),
    };

    if let Err(message) = result {
        runtime.fault(&message);
    }
}
