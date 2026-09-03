//! Runtime stack adapters for arithmetic and bitwise opcodes.

use crate::runtime::{push_value_result, RuntimeStack};
use crate::semantics::arithmetic as rules;

runtime_binary_value_op!(add, rules::add_values);

runtime_binary_value_op!(sub, rules::sub_values);

runtime_binary_value_op!(mul, rules::mul_values);

runtime_binary_value_op!(div, rules::div_values);

runtime_binary_value_op!(modulo, rules::modulo_values);

runtime_unary_value_op!(negate, rules::negate_value);

runtime_unary_value_op!(abs, rules::abs_value);

runtime_unary_value_op!(sign, rules::sign_value);

runtime_binary_value_op!(max, rules::max_values);

runtime_binary_value_op!(min, rules::min_values);

runtime_binary_value_op!(pow, rules::pow_values);

runtime_unary_value_op!(sqrt, rules::sqrt_value);

runtime_ternary_value_op!(modmul, rules::modmul_values);

runtime_ternary_value_op!(modpow, rules::modpow_values);

pub fn shl<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let shift = runtime.pop_i64();
    let value = runtime.pop_value();
    push_value_result(runtime, rules::shl_value(value, shift));
}

pub fn shr<R: RuntimeStack + ?Sized>(runtime: &mut R) {
    let shift = runtime.pop_i64();
    let value = runtime.pop_value();
    push_value_result(runtime, rules::shr_value(value, shift));
}

runtime_binary_value_op!(bitwise_and, rules::bitwise_and_values);

runtime_binary_value_op!(bitwise_or, rules::bitwise_or_values);

runtime_binary_value_op!(bitwise_xor, rules::bitwise_xor_values);

runtime_unary_value_op!(bitwise_not, rules::invert_value);

runtime_unary_value_op!(inc, rules::inc_value);

runtime_unary_value_op!(dec, rules::dec_value);

runtime_ternary_bool_op!(within, rules::within_values);
