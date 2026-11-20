//! Numeric conversions between VM stack values and `BigInt`.
//!
//! Migrated from `neo-core/src/smart_contract/native/stack_value_numeric.rs`.
//! Only the error import changes: `crate::error::CoreError` becomes
//! `neo_smart_contract::error::CoreError`. `neo_vm`, `num_bigint`, and
//! `num_traits` are external crates and keep their paths.

use neo_vm::StackValue;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
// Phase 9 (F17): only the `experimental` token-management module converts a
// stack value into a `CoreError`-carrying result.
#[cfg(feature = "experimental")]
use neo_smart_contract::error::CoreError;

pub(crate) const MAX_VM_INTEGER_BYTES: usize = 32;

pub(crate) fn stack_value_to_bigint(value: &StackValue) -> Option<BigInt> {
    match value {
        StackValue::Integer(value) => Some(BigInt::from(*value)),
        StackValue::Boolean(value) => Some(BigInt::from(i32::from(*value))),
        StackValue::BigInteger(bytes) => Some(BigInt::from_signed_bytes_le(bytes)),
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes)
            if bytes.len() <= MAX_VM_INTEGER_BYTES =>
        {
            Some(BigInt::from_signed_bytes_le(bytes))
        }
        _ => None,
    }
}

/// Converts a stack value into a `BigInt`, or a native-contract error.
///
/// Phase 9 (F17): consumed only by `native::token_management`, which is gated
/// behind the `experimental` feature.
#[cfg(feature = "experimental")]
pub(crate) fn stack_value_to_bigint_result(value: &StackValue) -> Result<BigInt, CoreError> {
    stack_value_to_bigint(value)
        .ok_or_else(|| CoreError::native_contract("cannot convert stack value to integer"))
}

pub(crate) fn stack_value_to_u32(value: &StackValue) -> Option<u32> {
    stack_value_to_bigint(value)?.to_u32()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_stack_values_convert_to_bigint() {
        assert_eq!(
            stack_value_to_bigint(&StackValue::Integer(-7)),
            Some(BigInt::from(-7))
        );
        assert_eq!(
            stack_value_to_bigint(&StackValue::Boolean(true)),
            Some(BigInt::from(1))
        );
        assert_eq!(
            stack_value_to_bigint(&StackValue::BigInteger(vec![0xff])),
            Some(BigInt::from(-1))
        );
        assert_eq!(
            stack_value_to_bigint(&StackValue::ByteString(vec![0x2a])),
            Some(BigInt::from(42))
        );
        assert_eq!(
            stack_value_to_bigint(&StackValue::Buffer(vec![0x2a])),
            Some(BigInt::from(42))
        );
    }

    #[test]
    fn oversized_byte_values_do_not_convert_to_bigint() {
        assert_eq!(
            stack_value_to_bigint(&StackValue::ByteString(vec![0; MAX_VM_INTEGER_BYTES + 1])),
            None
        );
        assert_eq!(
            stack_value_to_bigint(&StackValue::Buffer(vec![0; MAX_VM_INTEGER_BYTES + 1])),
            None
        );
    }

    #[test]
    fn stack_values_convert_to_u32_only_when_in_range() {
        assert_eq!(
            stack_value_to_u32(&StackValue::BigInteger(42u32.to_le_bytes().to_vec())),
            Some(42)
        );
        assert_eq!(stack_value_to_u32(&StackValue::Integer(-1)), None);
        assert_eq!(
            stack_value_to_u32(&StackValue::ByteString(vec![0; MAX_VM_INTEGER_BYTES + 1])),
            None
        );
    }
}
