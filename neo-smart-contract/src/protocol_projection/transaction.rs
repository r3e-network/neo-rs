use crate::error::{CoreError, CoreResult};
use neo_protocol::payloads::Transaction;
use neo_vm::{Interoperable, StackItem, StackValue, VmError};

/// Converts the transaction to a neo-vm-rs stack value (matches C# `Transaction.ToStackItem` layout).
pub fn transaction_to_stack_value(transaction: &Transaction) -> CoreResult<StackValue> {
    let sender = transaction
        .sender()
        .ok_or_else(|| CoreError::invalid_argument("Sender is not specified in the transaction"))?
        .to_bytes();

    Ok(StackValue::Array(vec![
        StackValue::ByteString(transaction.try_hash().map_err(CoreError::from)?.to_bytes()),
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

/// An owned transaction adapter for VM interoperability.
#[derive(Debug, Clone)]
pub struct TransactionInterop(pub Transaction);

impl From<Transaction> for TransactionInterop {
    fn from(transaction: Transaction) -> Self {
        Self(transaction)
    }
}

impl Interoperable for TransactionInterop {
    fn from_stack_item(&mut self, _stack_item: StackItem) -> Result<(), VmError> {
        // This operation is not supported for Transaction.
        // The C# implementation throws NotSupportedException.
        Err(VmError::invalid_operation_msg(
            "FromStackItem is not supported for Transaction",
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, VmError> {
        transaction_to_stack_item(&self.0)
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::{UInt160, WitnessScope};
    use neo_protocol::payloads::Signer;
    use num_traits::ToPrimitive;

    fn sample_transaction() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(42);
        tx.set_system_fee(7);
        tx.set_network_fee(9);
        tx.set_valid_until_block(10);
        tx.set_script(vec![0x01, 0x02]);
        tx.set_signers(vec![Signer::new(
            UInt160::zero(),
            WitnessScope::CALLED_BY_ENTRY,
        )]);
        tx
    }

    /// Test ToStackItem output when sender is set (matches C# stack item layout).
    #[test]
    fn test_to_stack_item_includes_sender() {
        let tx = sample_transaction();

        let item = transaction_to_stack_item(&tx).unwrap();
        assert!(!item.is_null());

        let array = item.as_array().expect("array");
        assert_eq!(array.len(), 8);
        assert_eq!(
            array[0].as_bytes().expect("hash bytes"),
            tx.hash().to_bytes()
        );
        assert_eq!(array[1].as_int().expect("version").to_i64().unwrap(), 0);
        assert_eq!(array[2].as_int().expect("nonce").to_i64().unwrap(), 42);
        assert_eq!(
            array[3].as_bytes().expect("sender"),
            UInt160::zero().to_bytes()
        );
        assert_eq!(array[4].as_int().expect("sysfee").to_i64().unwrap(), 7);
        assert_eq!(array[5].as_int().expect("netfee").to_i64().unwrap(), 9);
        assert_eq!(array[6].as_int().expect("vub").to_i64().unwrap(), 10);
        assert_eq!(array[7].as_bytes().expect("script"), vec![0x01, 0x02]);
    }

    /// Test direct neo-vm-rs StackValue projection for ToStackItem data layout.
    #[test]
    fn test_to_stack_value_includes_sender() {
        let tx = sample_transaction();

        assert_eq!(
            transaction_to_stack_value(&tx).unwrap(),
            StackValue::Array(vec![
                StackValue::ByteString(tx.hash().to_bytes()),
                StackValue::Integer(0),
                StackValue::Integer(42),
                StackValue::ByteString(UInt160::zero().to_bytes()),
                StackValue::Integer(7),
                StackValue::Integer(9),
                StackValue::Integer(10),
                StackValue::ByteString(vec![0x01, 0x02]),
            ])
        );
    }

    /// Test local StackItem projection stays a thin adapter from the StackValue layout.
    #[test]
    fn test_to_stack_item_matches_stack_value_projection() {
        let mut tx = sample_transaction();
        tx.set_nonce(7);
        tx.set_system_fee(11);
        tx.set_network_fee(13);
        tx.set_valid_until_block(17);
        tx.set_script(vec![0x51]);

        let expected = StackItem::try_from(transaction_to_stack_value(&tx).unwrap()).unwrap();
        assert_eq!(transaction_to_stack_item(&tx).unwrap(), expected);
        assert_eq!(TransactionInterop(tx).to_stack_item().unwrap(), expected);
    }

    /// Test ToStackItem returns error when sender is missing (matches C# ArgumentException).
    #[test]
    fn test_to_stack_item_requires_sender() {
        let tx = Transaction::new();
        let result = transaction_to_stack_item(&tx);
        assert!(
            result.is_err(),
            "to_stack_item should return Err when sender is missing"
        );
        assert_eq!(
            result.unwrap_err(),
            VmError::invalid_operation_msg("Invalid: Sender is not specified in the transaction")
        );
    }

    /// Test FromStackItem returns error (matches C# NotSupportedException).
    #[test]
    fn test_from_stack_item_not_supported() {
        let tx = sample_transaction();
        let expected = transaction_to_stack_value(&tx).unwrap();
        let mut interop = TransactionInterop(tx);
        let result = interop.from_stack_item(StackItem::null());
        assert!(
            result.is_err(),
            "Transaction::from_stack_item should return Err for unsupported operation"
        );
        assert_eq!(
            result.unwrap_err(),
            VmError::invalid_operation_msg("FromStackItem is not supported for Transaction")
        );
        assert_eq!(transaction_to_stack_value(&interop.0).unwrap(), expected);
    }

    #[test]
    fn transaction_projection_preserves_sender_error_before_hash_error() {
        let mut tx = Transaction::new();
        tx.set_script(vec![neo_vm::OpCode::NOP.byte(); u16::MAX as usize + 1]);
        assert!(tx.try_hash().is_err());
        assert_eq!(
            transaction_to_stack_value(&tx).unwrap_err(),
            CoreError::invalid_argument("Sender is not specified in the transaction")
        );
        assert_eq!(
            transaction_to_stack_item(&tx).unwrap_err(),
            VmError::invalid_operation_msg("Invalid: Sender is not specified in the transaction")
        );
    }

    #[test]
    fn transaction_projection_preserves_hash_codec_error_and_vm_mapping() {
        let mut tx = sample_transaction();
        tx.set_script(vec![neo_vm::OpCode::NOP.byte(); u16::MAX as usize + 1]);
        let low_error = tx.try_hash().unwrap_err().to_string();
        let error = transaction_to_stack_value(&tx).unwrap_err();
        assert_eq!(
            error,
            CoreError::Codec {
                message: low_error.clone()
            }
        );
        assert_eq!(error.to_string(), format!("Codec error: {low_error}"));
        assert_eq!(
            transaction_to_stack_item(&tx).unwrap_err(),
            VmError::invalid_operation_msg(error.to_string())
        );
        assert_eq!(
            TransactionInterop(tx).to_stack_item().unwrap_err(),
            VmError::invalid_operation_msg(error.to_string())
        );
    }

    #[test]
    fn transaction_interop_clone_box_owns_an_independent_transaction() {
        let (boxed, expected) = {
            let mut interop = TransactionInterop(sample_transaction());
            let expected = interop.to_stack_item().unwrap();
            let boxed = interop.clone_box();
            interop.0.set_nonce(99);
            assert_ne!(interop.to_stack_item().unwrap(), expected);
            (boxed, expected)
        };
        assert_eq!(boxed.to_stack_item().unwrap(), expected);
        assert_eq!(boxed.clone_box().to_stack_item().unwrap(), expected);
    }
}
