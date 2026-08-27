//! Runtime stack adapters for type conversion opcodes.

use crate::runtime::{push_value_result, RuntimeStack};
use crate::semantics::conversion as rules;

pub fn is_type<R: RuntimeStack + ?Sized>(runtime: &mut R, type_byte: u8) {
    let value = runtime.pop_value();
    runtime.push_bool(rules::is_type(&value, type_byte));
}

pub fn convert_to<R: RuntimeStack + ?Sized>(runtime: &mut R, target_type: u8) {
    let value = runtime.pop_value();
    push_value_result(runtime, rules::convert_value(value, target_type));
}
