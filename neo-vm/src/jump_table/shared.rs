//! Shared helpers used by opcode-family handlers.
//!
//! These functions encode C# VM stack coercion boundaries and common execution
//! context guards. Keeping them out of the dispatch-table root lets the root
//! focus on handler registration and hot opcode lookup.

use num_bigint::{BigInt, Sign};

use crate::error::{VmError, VmResult};
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::stack_item::{StackItem, VmInteger};

const VM_INTEGER_MAX_SIZE: usize = 32;

fn integer_size(value: &BigInt) -> usize {
    let bits = value.bits();
    if bits == 0 {
        return 0;
    }

    let mut bytes = bits.div_ceil(8);
    match value.sign() {
        Sign::Plus if bits % 8 == 0 => bytes += 1,
        Sign::Minus if bits % 8 == 0 && value.magnitude().trailing_zeros() != Some(bits - 1) => {
            bytes += 1;
        }
        Sign::NoSign | Sign::Plus | Sign::Minus => {}
    }

    usize::try_from(bytes).unwrap_or(usize::MAX)
}

fn ensure_integer_size(value: &BigInt, error: impl FnOnce(usize) -> VmError) -> VmResult<()> {
    let size = integer_size(value);
    if size > VM_INTEGER_MAX_SIZE {
        return Err(error(size));
    }
    Ok(())
}

/// C# `StackItem.GetInteger()` semantics for an integer operand read off the
/// evaluation stack (a count, index, size or shift a script controls).
///
/// In the reference VM a `Buffer` is NOT a `PrimitiveType` and has no
/// `GetInteger` override, so `GetInteger()` hits the base
/// `=> throw new InvalidCastException()` and FAULTS — even for a short buffer.
/// `Null` and compound items (`Array`/`Struct`/`Map`/pointer/interop) fault too;
/// only the `Integer`/`Boolean`/`ByteString` primitives yield a value.
///
/// This deliberately differs from [`StackItem::into_int`], which coerces a
/// `Buffer` of up to `VM_INTEGER_MAX_SIZE` bytes to its little-endian integer
/// value. That coercion is the `ConvertTo(Integer)` path (the CONVERT opcode);
/// the GetInteger path used by count/index/shift operands faults on a `Buffer`.
///
/// Callers still narrow the returned `BigInt` (e.g. `to_i32`/`to_i64`/`to_usize`)
/// and a value outside the target range faults — matching C#'s `(int)BigInteger`
/// cast, which throws `OverflowException` (it does NOT truncate) before the
/// per-opcode sign/bounds checks run.
pub(crate) fn get_integer(item: StackItem) -> VmResult<BigInt> {
    get_vm_integer(item).map(VmInteger::into_bigint)
}

/// C# `StackItem.GetInteger()` semantics without forcing values that fit in an
/// `i64` through an allocated `BigInt` representation.
///
/// Comparison and branch opcodes can consume this representation directly.
/// Arithmetic handlers that require `BigInt` keep using [`get_integer`].
pub(crate) fn get_vm_integer(item: StackItem) -> VmResult<VmInteger> {
    if matches!(item, StackItem::Buffer(_)) {
        return Err(VmError::invalid_type_simple(
            "operand is not an integer (C# GetInteger faults on Buffer)",
        ));
    }

    let value = match item {
        StackItem::Integer(value) => value,
        StackItem::Boolean(value) => VmInteger::Small(i64::from(value)),
        item => VmInteger::from_bigint(item.into_int()?),
    };
    if let VmInteger::Large(value) = &value {
        ensure_integer_size(value, |size| {
            VmError::invalid_type_simple(format!(
                "integer size {size} bytes exceeds maximum allowed size of {VM_INTEGER_MAX_SIZE} bytes"
            ))
        })?;
    }
    Ok(value)
}

/// The engine's current execution context, or a fault if there is none.
///
/// Shared by every opcode-family handler module so the "No current context"
/// guard reads identically across the jump table.
#[inline]
pub(crate) fn require_context<S>(
    engine: &mut ExecutionEngine<S>,
) -> VmResult<&mut ExecutionContext<S>> {
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))
}

/// Pushes a checked integer result onto the evaluation stack.
///
/// C# constructs an `Integer` for every arithmetic result. Its constructor
/// rejects values whose minimal signed two's-complement representation exceeds
/// 32 bytes, so the bound must be applied before creating the local item.
#[inline]
pub(crate) fn push_integer<S>(
    ctx: &mut ExecutionContext<S>,
    value: BigInt,
    overflow_message: &'static str,
) -> VmResult<()> {
    ensure_integer_size(&value, |_| VmError::invalid_operation_msg(overflow_message))?;
    ctx.push(StackItem::from_int(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialized_integer_size(value: &BigInt) -> usize {
        if value.sign() == Sign::NoSign {
            0
        } else {
            value.to_signed_bytes_le().len()
        }
    }

    #[test]
    fn integer_size_matches_minimal_signed_serialization_at_bit_boundaries() {
        for bit in 0..=264usize {
            let power = BigInt::from(1u8) << bit;
            for magnitude in [&power - 1u8, power.clone(), &power + 1u8] {
                for value in [magnitude.clone(), -magnitude] {
                    assert_eq!(
                        integer_size(&value),
                        serialized_integer_size(&value),
                        "signed size mismatch for {value}"
                    );
                }
            }
        }
    }

    #[test]
    fn integer_size_preserves_neo_vm_32_byte_boundaries() {
        let positive_max = (BigInt::from(1u8) << 255usize) - 1u8;
        let positive_overflow = BigInt::from(1u8) << 255usize;
        let negative_min = -(BigInt::from(1u8) << 255usize);
        let negative_overflow = &negative_min - 1u8;

        assert_eq!(integer_size(&positive_max), VM_INTEGER_MAX_SIZE);
        assert_eq!(integer_size(&positive_overflow), VM_INTEGER_MAX_SIZE + 1);
        assert_eq!(integer_size(&negative_min), VM_INTEGER_MAX_SIZE);
        assert_eq!(integer_size(&negative_overflow), VM_INTEGER_MAX_SIZE + 1);
    }
}
