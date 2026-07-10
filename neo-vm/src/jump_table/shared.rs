//! Shared helpers used by opcode-family handlers.
//!
//! These functions encode C# VM stack coercion boundaries and common execution
//! context guards. Keeping them out of the dispatch-table root lets the root
//! focus on handler registration and hot opcode lookup.

use num_bigint::BigInt;

use crate::error::{VmError, VmResult};
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::stack_item::StackItem;
use neo_vm_rs::StackValue;

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
    if matches!(item, StackItem::Buffer(_)) {
        return Err(VmError::invalid_type_simple(
            "operand is not an integer (C# GetInteger faults on Buffer)",
        ));
    }
    item.into_int()
}

/// C# `StackItem.GetInteger()` for an arithmetic/bitwise VALUE operand, returning
/// the typed [`StackValue`] the semantics layer expects.
///
/// Like [`get_integer`], a `Buffer` (not a `PrimitiveType`) and `Null` fault —
/// the numeric/comparison/bitwise opcodes (ADD/SUB/.../AND/OR/XOR/INVERT) read
/// their operands via `GetInteger()`, which throws on a non-integer. Only the
/// CONVERT opcode coerces a `Buffer`, via a separate `ConvertTo` path.
pub(crate) fn numeric_operand(item: StackItem) -> VmResult<StackValue> {
    match item {
        StackItem::Buffer(_) | StackItem::Null => Err(VmError::invalid_type_simple(
            "operand is not a numeric value (C# GetInteger faults on Buffer/Null)",
        )),
        item => StackValue::try_from(item),
    }
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

/// Maps an error string raised by the `neo_vm_rs` semantics layer into a VM
/// fault, matching how the reference VM surfaces an arithmetic/type failure.
#[inline]
pub(crate) fn semantics_error(error: String) -> VmError {
    VmError::invalid_operation_msg(error)
}

/// Pushes a typed [`StackValue`] result back onto the evaluation stack,
/// converting it into the engine's [`StackItem`] representation.
#[inline]
pub(crate) fn push_stack_value<S>(
    ctx: &mut ExecutionContext<S>,
    value: StackValue,
) -> VmResult<()> {
    ctx.push(StackItem::try_from(value)?)
}
