use crate::error::CoreError;
use crate::smart_contract::native::stack_value_numeric::{
    stack_value_to_bigint_result, MAX_VM_INTEGER_BYTES,
};
use crate::neo_vm::StackItem;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;

pub(super) fn stack_value_to_bigint(value: &StackValue) -> Result<BigInt, CoreError> {
    stack_value_to_bigint_result(value)
}

pub(super) fn stack_value_to_bytes(value: &StackValue) -> Result<Vec<u8>, CoreError> {
    value
        .to_byte_string_bytes()
        .ok_or_else(|| CoreError::native_contract("cannot convert stack value to byte array"))
}

pub(super) fn stack_value_to_bool(value: &StackValue) -> Result<bool, CoreError> {
    match value {
        StackValue::Null => Ok(false),
        StackValue::Boolean(value) => Ok(*value),
        StackValue::Integer(value) => Ok(*value != 0),
        StackValue::BigInteger(bytes) => Ok(bytes.iter().any(|byte| *byte != 0)),
        StackValue::ByteString(bytes) if bytes.len() <= MAX_VM_INTEGER_BYTES => {
            Ok(bytes.iter().any(|byte| *byte != 0))
        }
        StackValue::ByteString(_) => Err(CoreError::native_contract(
            "cannot convert oversized byte string to boolean",
        )),
        StackValue::Buffer(_) => Ok(true),
        StackValue::Array(_)
        | StackValue::Struct(_)
        | StackValue::Map(_)
        | StackValue::Pointer(_)
        | StackValue::Interop(_)
        | StackValue::Iterator(_) => Ok(true),
    }
}

pub(super) fn bigint_stack_value(value: &BigInt) -> StackValue {
    StackValue::BigInteger(value.to_signed_bytes_le())
}

pub(super) fn stack_item_to_stack_value(
    stack_item: StackItem,
    context: &str,
) -> Result<StackValue, CoreError> {
    StackValue::try_from(stack_item).map_err(|err| {
        CoreError::native_contract(format!("failed to convert {context} StackItem: {err}"))
    })
}

pub(super) fn stack_value_to_stack_item(
    stack_value: StackValue,
    context: &str,
) -> Result<StackItem, CoreError> {
    StackItem::try_from(stack_value).map_err(|err| {
        CoreError::native_contract(format!("failed to convert {context} StackValue: {err}"))
    })
}
