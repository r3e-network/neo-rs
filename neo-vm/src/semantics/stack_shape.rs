//! Shared stack-shape helpers for value-only opcode semantics.

use alloc::string::String;

use crate::StackValue;

pub(crate) trait ValueStack {
    fn pop_value(&mut self) -> Result<StackValue, String>;
    fn push_value(&mut self, value: StackValue) -> Result<(), String>;
}

pub(crate) fn unary_value<S>(
    stack: &mut S,
    op: fn(StackValue) -> Result<StackValue, String>,
) -> Result<(), String>
where
    S: ValueStack + ?Sized,
{
    let value = stack.pop_value()?;
    stack.push_value(op(value)?)
}

pub(crate) fn binary_value<S>(
    stack: &mut S,
    op: fn(StackValue, StackValue) -> Result<StackValue, String>,
) -> Result<(), String>
where
    S: ValueStack + ?Sized,
{
    let right = stack.pop_value()?;
    let left = stack.pop_value()?;
    stack.push_value(op(left, right)?)
}

pub(crate) fn ternary_value<S>(
    stack: &mut S,
    op: fn(StackValue, StackValue, StackValue) -> Result<StackValue, String>,
) -> Result<(), String>
where
    S: ValueStack + ?Sized,
{
    let third = stack.pop_value()?;
    let second = stack.pop_value()?;
    let first = stack.pop_value()?;
    stack.push_value(op(first, second, third)?)
}

pub(crate) fn unary_bool<S>(
    stack: &mut S,
    op: fn(&StackValue) -> Result<bool, String>,
) -> Result<(), String>
where
    S: ValueStack + ?Sized,
{
    let value = stack.pop_value()?;
    stack.push_value(StackValue::Boolean(op(&value)?))
}

pub(crate) fn binary_bool<S, F>(stack: &mut S, op: F) -> Result<(), String>
where
    S: ValueStack + ?Sized,
    F: FnOnce(&StackValue, &StackValue) -> Result<bool, String>,
{
    let right = stack.pop_value()?;
    let left = stack.pop_value()?;
    stack.push_value(StackValue::Boolean(op(&left, &right)?))
}

pub(crate) fn ternary_bool<S>(
    stack: &mut S,
    op: fn(StackValue, StackValue, StackValue) -> Result<bool, String>,
) -> Result<(), String>
where
    S: ValueStack + ?Sized,
{
    let third = stack.pop_value()?;
    let second = stack.pop_value()?;
    let first = stack.pop_value()?;
    stack.push_value(StackValue::Boolean(op(first, second, third)?))
}

pub(crate) fn bool_binary<S>(
    stack: &mut S,
    op: fn(bool, bool) -> bool,
    truthy: fn(&StackValue) -> bool,
) -> Result<(), String>
where
    S: ValueStack + ?Sized,
{
    let right = stack.pop_value()?;
    let left = stack.pop_value()?;
    stack.push_value(StackValue::Boolean(op(truthy(&left), truthy(&right))))
}
