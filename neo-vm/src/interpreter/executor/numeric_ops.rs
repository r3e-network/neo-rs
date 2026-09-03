use super::super::helpers::*;
use super::super::opcodes::*;
use super::super::runtime_types::StackValue;
use super::control::Dispatch;
use crate::{
    semantics::{
        arithmetic as arithmetic_rules, comparison as comparison_rules,
        stack_shape::{self, ValueStack},
    },
    StackValue as AbiStackValue,
};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

#[inline]
pub(super) fn execute(opcode: u8, stack: &mut Vec<StackValue>) -> Result<Dispatch, String> {
    let mut value_stack = InterpreterValueStack::new(stack);

    match opcode {
        // =============================================================================
        // BITWISE LOGIC OPERATIONS (0x90-0x98)
        // =============================================================================
        INVERT => stack_shape::unary_value(&mut value_stack, arithmetic_rules::invert_value)?,
        AND => stack_shape::binary_value(&mut value_stack, arithmetic_rules::bitwise_and_values)?,
        OR => stack_shape::binary_value(&mut value_stack, arithmetic_rules::bitwise_or_values)?,
        XOR => stack_shape::binary_value(&mut value_stack, arithmetic_rules::bitwise_xor_values)?,
        NUMEQUAL => stack_shape::binary_bool(&mut value_stack, comparison_rules::num_equal_values)?,
        NUMNOTEQUAL => {
            stack_shape::binary_bool(&mut value_stack, comparison_rules::num_not_equal_values)?
        }
        EQUAL => {
            let right = value_stack.pop_interpreter_value()?;
            let left = value_stack.pop_interpreter_value()?;
            value_stack.push_interpreter_value(StackValue::Boolean(vm_equal(&left, &right)));
        }
        NOTEQUAL => {
            let right = value_stack.pop_interpreter_value()?;
            let left = value_stack.pop_interpreter_value()?;
            value_stack.push_interpreter_value(StackValue::Boolean(!vm_equal(&left, &right)));
        }
        LT => stack_shape::binary_bool(&mut value_stack, comparison_rules::less_than_values)?,
        LE => stack_shape::binary_bool(&mut value_stack, comparison_rules::less_or_equal_values)?,
        GT => stack_shape::binary_bool(&mut value_stack, comparison_rules::greater_than_values)?,
        GE => {
            stack_shape::binary_bool(&mut value_stack, comparison_rules::greater_or_equal_values)?
        }
        // =============================================================================
        // ARITHMETIC OPERATIONS (0x99-0xbb)
        // =============================================================================
        SIGN => stack_shape::unary_value(&mut value_stack, arithmetic_rules::sign_value)?,
        ABS => stack_shape::unary_value(&mut value_stack, arithmetic_rules::abs_value)?,
        NEGATE => stack_shape::unary_value(&mut value_stack, arithmetic_rules::negate_value)?,
        ADD => stack_shape::binary_value(&mut value_stack, arithmetic_rules::add_values)?,
        INC => stack_shape::unary_value(&mut value_stack, arithmetic_rules::inc_value)?,
        SUB => stack_shape::binary_value(&mut value_stack, arithmetic_rules::sub_values)?,
        POW => stack_shape::binary_value(&mut value_stack, arithmetic_rules::pow_values)?,
        SQRT => stack_shape::unary_value(&mut value_stack, arithmetic_rules::sqrt_value)?,
        MODMUL => stack_shape::ternary_value(&mut value_stack, arithmetic_rules::modmul_values)?,
        MODPOW => stack_shape::ternary_value(&mut value_stack, arithmetic_rules::modpow_values)?,
        SHL => shift_value(&mut value_stack, arithmetic_rules::shl_value)?,
        SHR => shift_value(&mut value_stack, arithmetic_rules::shr_value)?,
        NOT => stack_shape::unary_bool(&mut value_stack, comparison_rules::not_value)?,
        MUL => stack_shape::binary_value(&mut value_stack, arithmetic_rules::mul_values)?,
        DIV => stack_shape::binary_value(&mut value_stack, arithmetic_rules::div_values)?,
        MOD => stack_shape::binary_value(&mut value_stack, arithmetic_rules::modulo_values)?,
        DEC => stack_shape::unary_value(&mut value_stack, arithmetic_rules::dec_value)?,
        BOOLAND => stack_shape::bool_binary(
            &mut value_stack,
            comparison_rules::bool_and,
            comparison_rules::boolean_value,
        )?,
        BOOLOR => stack_shape::bool_binary(
            &mut value_stack,
            comparison_rules::bool_or,
            comparison_rules::boolean_value,
        )?,
        NZ => stack_shape::unary_bool(&mut value_stack, comparison_rules::nz_value)?,
        MIN => stack_shape::binary_value(&mut value_stack, arithmetic_rules::min_values)?,
        MAX => stack_shape::binary_value(&mut value_stack, arithmetic_rules::max_values)?,
        WITHIN => stack_shape::ternary_bool(&mut value_stack, arithmetic_rules::within_values)?,
        _ => unreachable!("opcode routed to numeric_ops: 0x{opcode:02x}"),
    }
    Ok(Dispatch::Fallthrough)
}

fn shift_value(
    stack: &mut InterpreterValueStack<'_>,
    op: fn(AbiStackValue, i64) -> Result<AbiStackValue, String>,
) -> Result<(), String> {
    let shift = stack.pop_shift_count()?;
    let value = stack.pop_interpreter_value()?;
    if shift == 0 {
        stack.push_interpreter_value(value);
        return Ok(());
    }
    stack.push_abi_value(op(into_abi_value(value), shift)?)
}

struct InterpreterValueStack<'a> {
    stack: &'a mut Vec<StackValue>,
}

impl<'a> InterpreterValueStack<'a> {
    fn new(stack: &'a mut Vec<StackValue>) -> Self {
        Self { stack }
    }

    fn pop_interpreter_value(&mut self) -> Result<StackValue, String> {
        pop_item(self.stack)
    }

    fn push_interpreter_value(&mut self, value: StackValue) {
        self.stack.push(value);
    }

    fn pop_shift_count(&mut self) -> Result<i64, String> {
        pop_shift_count(self.stack)
    }

    fn push_abi_value(&mut self, value: AbiStackValue) -> Result<(), String> {
        <Self as ValueStack>::push_value(self, value)
    }
}

impl ValueStack for InterpreterValueStack<'_> {
    fn pop_value(&mut self) -> Result<AbiStackValue, String> {
        self.stack
            .pop()
            .map(into_abi_value)
            .ok_or_else(|| "stack underflow".to_string())
    }

    fn push_value(&mut self, value: AbiStackValue) -> Result<(), String> {
        self.stack.push(match value {
            AbiStackValue::Integer(value) => StackValue::Integer(value),
            AbiStackValue::BigInteger(value) => StackValue::BigInteger(value),
            AbiStackValue::ByteString(value) => StackValue::ByteString(value),
            AbiStackValue::Boolean(value) => StackValue::Boolean(value),
            AbiStackValue::Null => StackValue::Null,
            AbiStackValue::Pointer(value) => {
                StackValue::Pointer(usize::try_from(value).map_err(|_| {
                    "semantic opcode produced a pointer outside the interpreter range".to_string()
                })?)
            }
            AbiStackValue::Buffer(_)
            | AbiStackValue::Array(_)
            | AbiStackValue::Struct(_)
            | AbiStackValue::Map(_)
            | AbiStackValue::Interop(_)
            | AbiStackValue::Iterator(_) => {
                return Err("semantic numeric opcode produced a compound result".to_string())
            }
        });
        Ok(())
    }
}
