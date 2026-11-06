use super::Transaction;
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_vm::{StackItem, StackValue, VmError};

/// Converts the transaction to a neo-vm-rs stack value (matches C# `Transaction.ToStackItem` layout).
pub fn transaction_to_stack_value(transaction: &Transaction) -> CoreResult<StackValue> {
    let sender = transaction
        .sender()
        .ok_or_else(|| CoreError::invalid_argument("Sender is not specified in the transaction"))?
        .to_bytes();

    Ok(StackValue::Array(vec![
        StackValue::ByteString(transaction.try_hash()?.to_bytes()),
        StackValue::Integer(i64::from(transaction.version())),
        StackValue::Integer(i64::from(transaction.nonce())),
        StackValue::ByteString(sender),
        StackValue::Integer(transaction.system_fee()),
        StackValue::Integer(transaction.network_fee()),
        StackValue::Integer(i64::from(transaction.valid_until_block())),
        StackValue::ByteString(transaction.script().to_vec()),
    ]))
}

/// Converts the transaction projection to a VM stack item.
pub fn transaction_to_stack_item(transaction: &Transaction) -> Result<StackItem, VmError> {
    let sv = transaction_to_stack_value(transaction)
        .map_err(|e| VmError::invalid_operation_msg(e.to_string()))?;
    StackItem::try_from(sv).map_err(|error| {
        VmError::invalid_operation_msg(format!(
            "Failed to convert transaction StackValue to StackItem: {error}"
        ))
    })
}
