//! Runtime-level stack opcode adapters.

use alloc::{string::String, vec::Vec};

use crate::{runtime::RuntimeStack, semantics::stack as stack_rules, StackValue};

pub fn drop_top<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, stack_rules::drop_top);
}

pub fn dup<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, stack_rules::dup);
}

pub fn swap<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, |stack| stack_rules::swap(stack));
}

pub fn nip<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, stack_rules::nip);
}

pub fn xdrop<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let index = runtime.pop_i64();
    apply_stack_result(runtime, |stack| stack_rules::xdrop(stack, index));
}

pub fn over<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, stack_rules::over);
}

pub fn pick<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let index = runtime.pop_i64();
    apply_stack_result(runtime, |stack| stack_rules::pick(stack, index));
}

pub fn tuck<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, stack_rules::tuck);
}

pub fn rot<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, stack_rules::rot);
}

pub fn roll<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let index = runtime.pop_i64();
    apply_stack_result(runtime, |stack| stack_rules::roll(stack, index));
}

pub fn reverse3<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, |stack| stack_rules::reverse_top(stack, 3));
}

pub fn reverse4<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    apply_stack_result(runtime, |stack| stack_rules::reverse_top(stack, 4));
}

pub fn reverse_n<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let count = runtime.pop_i64();
    apply_stack_result(runtime, |stack| stack_rules::reverse_n(stack, count));
}

pub fn depth<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let depth = stack_rules::depth(runtime.stack_values());
    runtime.push_value(StackValue::Integer(depth));
}

pub fn clear<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    stack_rules::clear(runtime.stack_values_mut());
}

fn apply_stack_result<R, F>(runtime: &mut R, apply: F)
where
    R: RuntimeStack + ?Sized,
    F: FnOnce(&mut Vec<StackValue>) -> Result<(), String>,
{
    if let Err(message) = apply(runtime.stack_values_mut()) {
        runtime.fault(&message);
    }
}
