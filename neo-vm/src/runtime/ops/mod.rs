//! Runtime-level opcode adapters shared by compiled VM runtimes.
//!
//! The pure modules in `semantics::{arithmetic, comparison, conversion,
//! collections, splice, stack, stack_shape}` own the VM rules and stack
//! shapes. This module only adapts those rules to a host runtime's stack/fault
//! interface.

use alloc::string::String;

use crate::{runtime::RuntimeStack, semantics::stack_shape::ValueStack, StackValue};

macro_rules! runtime_unary_value_op {
    ($name:ident, $rule:path) => {
        pub fn $name<R: $crate::runtime::RuntimeStack + ?Sized>(runtime: &mut R) {
            super::apply_or_fault(runtime, |stack| {
                $crate::semantics::stack_shape::unary_value(stack, $rule)
            });
        }
    };
}

macro_rules! runtime_binary_value_op {
    ($name:ident, $rule:path) => {
        pub fn $name<R: $crate::runtime::RuntimeStack + ?Sized>(runtime: &mut R) {
            super::apply_or_fault(runtime, |stack| {
                $crate::semantics::stack_shape::binary_value(stack, $rule)
            });
        }
    };
}

macro_rules! runtime_ternary_value_op {
    ($name:ident, $rule:path) => {
        pub fn $name<R: $crate::runtime::RuntimeStack + ?Sized>(runtime: &mut R) {
            super::apply_or_fault(runtime, |stack| {
                $crate::semantics::stack_shape::ternary_value(stack, $rule)
            });
        }
    };
}

macro_rules! runtime_unary_bool_op {
    ($name:ident, $rule:path) => {
        pub fn $name<R: $crate::runtime::RuntimeStack + ?Sized>(runtime: &mut R) {
            super::apply_or_fault(runtime, |stack| {
                $crate::semantics::stack_shape::unary_bool(stack, $rule)
            });
        }
    };
}

macro_rules! runtime_binary_bool_op {
    ($name:ident, $rule:path) => {
        pub fn $name<R: $crate::runtime::RuntimeStack + ?Sized>(runtime: &mut R) {
            super::apply_or_fault(runtime, |stack| {
                $crate::semantics::stack_shape::binary_bool(stack, $rule)
            });
        }
    };
}

macro_rules! runtime_binary_bool_value_op {
    ($name:ident, $rule:path) => {
        pub fn $name<R: $crate::runtime::RuntimeStack + ?Sized>(runtime: &mut R) {
            super::apply_or_fault(runtime, |stack| {
                $crate::semantics::stack_shape::binary_bool(stack, |left, right| {
                    Ok($rule(left, right))
                })
            });
        }
    };
}

macro_rules! runtime_ternary_bool_op {
    ($name:ident, $rule:path) => {
        pub fn $name<R: $crate::runtime::RuntimeStack + ?Sized>(runtime: &mut R) {
            super::apply_or_fault(runtime, |stack| {
                $crate::semantics::stack_shape::ternary_bool(stack, $rule)
            });
        }
    };
}

macro_rules! runtime_bool_binary_op {
    ($name:ident, $rule:path, $truthy:path) => {
        pub fn $name<R: $crate::runtime::RuntimeStack + ?Sized>(runtime: &mut R) {
            super::apply_or_fault(runtime, |stack| {
                $crate::semantics::stack_shape::bool_binary(stack, $rule, $truthy)
            });
        }
    };
}

pub mod arithmetic;
pub mod bytes;
pub mod collections;
pub mod comparison;
pub mod conversion;
pub mod stack;

impl<R: RuntimeStack + ?Sized> ValueStack for R {
    fn pop_value(&mut self) -> Result<StackValue, String> {
        Ok(RuntimeStack::pop_value(self))
    }

    fn push_value(&mut self, value: StackValue) -> Result<(), String> {
        RuntimeStack::push_value(self, value);
        Ok(())
    }
}

pub(crate) fn apply_or_fault<R, F>(runtime: &mut R, apply: F)
where
    R: RuntimeStack + ?Sized,
    F: FnOnce(&mut R) -> Result<(), String>,
{
    if let Err(message) = apply(runtime) {
        runtime.fault(&message);
    }
}
