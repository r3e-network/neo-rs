//! Numeric operations for the Neo Virtual Machine.
//!
//! This module provides the numeric operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::jump_table::JumpTable;
use crate::op_code::OpCode;
use crate::stack_item::StackItem;
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// Maximum size for BigInt results in bytes (256 bits = 32 bytes)
/// This matches the C# Neo VM behavior to prevent memory exhaustion attacks.
const MAX_BIGINT_SIZE: usize = 32;

/// Checks if a BigInt value exceeds the maximum allowed size.
/// Returns an error if the value is too large.
#[inline]
fn check_bigint_size(value: &BigInt) -> VmResult<()> {
    let byte_len = value.to_signed_bytes_le().len();
    if byte_len > MAX_BIGINT_SIZE {
        return Err(VmError::invalid_operation_msg(format!(
            "BigInt size {} bytes exceeds maximum {} bytes",
            byte_len, MAX_BIGINT_SIZE
        )));
    }
    Ok(())
}

/// Registers the numeric operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    jump_table.register(OpCode::INC, inc);
    jump_table.register(OpCode::DEC, dec);
    jump_table.register(OpCode::SIGN, sign);
    jump_table.register(OpCode::NEGATE, negate);
    jump_table.register(OpCode::ABS, abs);
    jump_table.register(OpCode::ADD, add);
    jump_table.register(OpCode::SUB, sub);
    jump_table.register(OpCode::MUL, mul);
    jump_table.register(OpCode::DIV, div);
    jump_table.register(OpCode::MOD, modulo);
    jump_table.register(OpCode::POW, pow);
    jump_table.register(OpCode::SQRT, sqrt);
    jump_table.register(OpCode::SHL, shl);
    jump_table.register(OpCode::SHR, shr);
    jump_table.register(OpCode::MIN, min);
    jump_table.register(OpCode::MAX, max);
    jump_table.register(OpCode::WITHIN, within);

    // Comparison operations
    jump_table.register(OpCode::LT, lt);
    jump_table.register(OpCode::LE, le);
    jump_table.register(OpCode::GT, gt);
    jump_table.register(OpCode::GE, ge);
    jump_table.register(OpCode::NUMEQUAL, numequal);
    jump_table.register(OpCode::NUMNOTEQUAL, numnotequal);

    // Logical operations
    jump_table.register(OpCode::NOT, not);
    jump_table.register(OpCode::BOOLAND, booland);
    jump_table.register(OpCode::BOOLOR, boolor);
    jump_table.register(OpCode::NZ, nz);

    // Advanced numeric operations
    jump_table.register(OpCode::MODMUL, modmul);
    jump_table.register(OpCode::MODPOW, modpow);
}

/// Implements the INC operation.
fn inc(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the value from the stack
    let value = context.pop()?.as_int()?;

    // Increment the value
    let result = value + BigInt::one();
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the DEC operation.
fn dec(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the value from the stack
    let value = context.pop()?.as_int()?;

    // Decrement the value
    let result = value - BigInt::one();
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the SIGN operation.
fn sign(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the value from the stack
    let value = context.pop()?.as_int()?;

    // Get the sign of the value
    let result = if value.is_zero() {
        BigInt::zero()
    } else if value.is_positive() {
        BigInt::one()
    } else {
        -BigInt::one()
    };

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the NEGATE operation.
fn negate(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the value from the stack
    let value = context.pop()?.as_int()?;

    // Negate the value
    let result = -value;
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the ABS operation.
fn abs(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the value from the stack
    let value = context.pop()?.as_int()?;

    // Get the absolute value
    let result = value.abs();
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the ADD operation.
fn add(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    // Add the values
    let result = match (a, b) {
        (StackItem::Integer(a), StackItem::Integer(b)) => {
            let sum = &a + &b;
            check_bigint_size(&sum)?; // SECURITY: Check result size
            StackItem::from_int(sum)
        }
        (StackItem::ByteString(a), StackItem::ByteString(b)) => {
            let mut result = a.clone();
            result.extend_from_slice(&b);
            StackItem::from_byte_string(result)
        }
        (StackItem::Buffer(mut a), StackItem::Buffer(b)) => {
            a.extend_from_slice(b.data());
            StackItem::Buffer(a)
        }
        (a, b) => {
            // Try to convert to integers and add
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            let sum = &a_int + &b_int;
            check_bigint_size(&sum)?; // SECURITY: Check result size
            StackItem::from_int(sum)
        }
    };

    context.push(result)?;

    Ok(())
}

/// Implements the SUB operation.
fn sub(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    // Subtract the values
    let result = a - b;
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the MUL operation.
fn mul(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    // Multiply the values
    let result = a * b;
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the DIV operation.
fn div(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if b.is_zero() {
        return Err(VmError::invalid_operation_msg("Division by zero"));
    }

    // Divide the values
    let result = a / b;

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the MOD operation.
fn modulo(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if b.is_zero() {
        return Err(VmError::invalid_operation_msg("Division by zero"));
    }

    // Calculate the modulo
    let result = a % b;

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the POW operation.
fn pow(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    // Convert exponent to i32 and enforce MaxShift (matches C# AssertShift).
    let exponent_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Exponent too large"))?;
    limits.assert_shift(exponent_i32)?;
    let exponent = exponent_i32 as u32;

    // Calculate the power
    let result = a.pow(exponent);
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the SQRT operation.
fn sqrt(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the value from the stack
    let value = context.pop()?.as_int()?;

    if value.is_negative() {
        return Err(VmError::invalid_operation_msg(
            "Square root of negative number",
        ));
    }

    let result = if value.is_zero() {
        BigInt::zero()
    } else {
        integer_sqrt(&value)
    };

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Integer square root using Newton's method (matches C# BigInteger.Sqrt exactly)
fn integer_sqrt(value: &BigInt) -> BigInt {
    if value.is_zero() {
        return BigInt::zero();
    }

    if value == &BigInt::from(1) {
        return BigInt::from(1);
    }

    let mut x = value.clone();
    let mut y: BigInt = (value + 1) / 2;

    while y < x {
        x = y.clone();
        y = (&x + value / &x) / 2;
    }

    x
}

/// Implements the SHL operation.
fn shl(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;
    let shift_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Shift amount too large"))?;
    limits.assert_shift(shift_i32)?;

    if shift_i32 == 0 {
        context.push(StackItem::from_int(a))?;
        return Ok(());
    }

    // Perform the left shift
    let result = a << (shift_i32 as u32);
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the SHR operation.
fn shr(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;
    let shift_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Shift amount too large"))?;
    limits.assert_shift(shift_i32)?;

    if shift_i32 == 0 {
        context.push(StackItem::from_int(a))?;
        return Ok(());
    }

    // Perform the right shift
    let result = a >> (shift_i32 as u32);
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the MIN operation.
fn min(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    // Get the minimum value
    let result = if a < b { a } else { b };

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the MAX operation.
fn max(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    // Get the maximum value
    let result = if a > b { a } else { b };

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Implements the WITHIN operation.
fn within(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;
    let x = context.pop()?.as_int()?;

    let result = a <= x && x < b;

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the LT operation.
fn lt(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    if context.evaluation_stack().len() < 2 && context.evaluation_stack().len() == 1 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => false, // null < null is false
        (StackItem::Null, _) => true,                // null < anything is true
        (_, StackItem::Null) => false,               // anything < null is false
        _ => {
            // Both are non-null, convert to integers and compare
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            a_int < b_int
        }
    };

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the LE operation.
fn le(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    if context.evaluation_stack().len() < 2 && context.evaluation_stack().len() == 1 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => true, // null <= null is true
        (StackItem::Null, _) => true,               // null <= anything is true
        (_, StackItem::Null) => false,              // anything <= null is false
        _ => {
            // Both are non-null, convert to integers and compare
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            a_int <= b_int
        }
    };

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the GT operation.
fn gt(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    if context.evaluation_stack().len() < 2 && context.evaluation_stack().len() == 1 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => false, // null > null is false
        (StackItem::Null, _) => false,               // null > anything is false
        (_, StackItem::Null) => true,                // anything > null is true
        _ => {
            // Both are non-null, convert to integers and compare
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            a_int > b_int
        }
    };

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the GE operation.
fn ge(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    if context.evaluation_stack().len() < 2 && context.evaluation_stack().len() == 1 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => true, // null >= null is true
        (StackItem::Null, _) => false,              // null >= anything is false
        (_, StackItem::Null) => true,               // anything >= null is true
        _ => {
            // Both are non-null, convert to integers and compare
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            a_int >= b_int
        }
    };

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the NUMEQUAL operation.
fn numequal(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    if context.evaluation_stack().len() < 2 && context.evaluation_stack().len() == 1 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => true,
        (StackItem::Null, _) => false,
        (_, StackItem::Null) => false,
        (StackItem::Boolean(a_bool), StackItem::Boolean(b_bool)) => a_bool == b_bool,
        (StackItem::Boolean(a_bool), _) => {
            // Convert boolean to integer and compare
            let a_int = if *a_bool { 1 } else { 0 };
            let b_int = b.as_int()?;
            num_bigint::BigInt::from(a_int) == b_int
        }
        (_, StackItem::Boolean(b_bool)) => {
            // Convert boolean to integer and compare
            let a_int = a.as_int()?;
            let b_int = if *b_bool { 1 } else { 0 };
            a_int == num_bigint::BigInt::from(b_int)
        }
        _ => {
            // Both are non-null, non-boolean, convert to integers and compare
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            a_int == b_int
        }
    };

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the NUMNOTEQUAL operation.
fn numnotequal(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    if context.evaluation_stack().len() < 2 && context.evaluation_stack().len() == 1 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => false, // null != null is false
        (StackItem::Null, _) => true,                // null != anything is true
        (_, StackItem::Null) => true,                // anything != null is true
        (StackItem::Boolean(a_bool), StackItem::Boolean(b_bool)) => a_bool != b_bool, // boolean != boolean
        (StackItem::Boolean(a_bool), _) => {
            // Convert boolean to integer and compare
            let a_int = if *a_bool { 1 } else { 0 };
            let b_int = b.as_int()?;
            num_bigint::BigInt::from(a_int) != b_int
        }
        (_, StackItem::Boolean(b_bool)) => {
            // Convert boolean to integer and compare
            let a_int = a.as_int()?;
            let b_int = if *b_bool { 1 } else { 0 };
            a_int != num_bigint::BigInt::from(b_int)
        }
        _ => {
            // Both are non-null, non-boolean, convert to integers and compare
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            a_int != b_int
        }
    };

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the NOT operation.
fn not(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the value from the stack
    let value = context.pop()?;

    // Perform logical NOT
    let result = match value {
        StackItem::Boolean(b) => !b,
        StackItem::Integer(i) => i.is_zero(),
        StackItem::Null => true,
        _ => false,
    };

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the BOOLAND operation.
fn booland(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_bool()?;
    let a = context.pop()?.as_bool()?;

    // Perform logical AND
    let result = a && b;

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the BOOLOR operation.
fn boolor(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_bool()?;
    let a = context.pop()?.as_bool()?;

    // Perform logical OR
    let result = a || b;

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the NZ operation.
fn nz(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the value from the stack
    let value = context.pop()?.as_int()?;

    let result = !value.is_zero();

    context.push(StackItem::from_bool(result))?;

    Ok(())
}

/// Implements the MODMUL operation.
fn modmul(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let modulus = context.pop()?.as_int()?;
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if modulus.is_zero() {
        return Err(VmError::division_by_zero_msg("division"));
    }

    let result = (a * b) % modulus;
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}

/// Computes the modular inverse of `value` modulo `modulus`.
/// Mirrors Neo.Extensions.BigIntegerExtensions.ModInverse exactly.
fn mod_inverse(value: &BigInt, modulus: &BigInt) -> VmResult<BigInt> {
    if value <= &BigInt::zero() {
        return Err(VmError::invalid_operation_msg(
            "Modular inverse requires positive value",
        ));
    }
    if modulus < &BigInt::from(2u8) {
        return Err(VmError::invalid_operation_msg(
            "Modular inverse requires modulus >= 2",
        ));
    }

    let mut r = value.clone();
    let mut old_r = modulus.clone();
    let mut s = BigInt::one();
    let mut old_s = BigInt::zero();

    while r > BigInt::zero() {
        let q = &old_r / &r;
        let new_r = &old_r % &r;
        old_r = r;
        r = new_r;

        let new_s = &old_s - &q * &s;
        old_s = s;
        s = new_s;
    }

    let mut result = old_s % modulus;
    if result.is_negative() {
        result += modulus;
    }

    if (value * &result) % modulus != BigInt::one() {
        return Err(VmError::invalid_operation_msg(
            "No modular inverse exists for the given inputs",
        ));
    }

    Ok(result)
}

/// Implements the MODPOW operation.
fn modpow(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let modulus = context.pop()?.as_int()?;
    let exponent = context.pop()?.as_int()?;
    let base = context.pop()?.as_int()?;

    // Exponent == -1 triggers modular inverse (matches C# ModPow semantics).
    if exponent == -BigInt::one() {
        let result = mod_inverse(&base, &modulus)?;
        check_bigint_size(&result)?; // SECURITY: Check result size
        context.push(StackItem::from_int(result))?;
        return Ok(());
    }

    if exponent < -BigInt::one() {
        return Err(VmError::invalid_operation_msg(
            "Exponent less than -1 not supported",
        ));
    }

    if modulus.is_zero() {
        return Err(VmError::division_by_zero_msg("division"));
    }

    if exponent.is_negative() {
        return Err(VmError::invalid_operation_msg(
            "Negative exponent not supported",
        ));
    }

    // Perform modular exponentiation: base^exponent % modulus
    let result = base.modpow(&exponent, &modulus);
    check_bigint_size(&result)?; // SECURITY: Check result size

    context.push(StackItem::from_int(result))?;

    Ok(())
}
