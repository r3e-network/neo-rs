use super::*;
use crate::error::CoreError;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::stack_item_extract::{extract_int, extract_u32};
use neo_vm::StackItem;

/// Notary deposit state (matches C# Deposit in Notary).
#[derive(Clone, Debug, Default)]
pub struct Deposit {
    /// The amount of GAS deposited.
    pub amount: BigInt,
    /// The block height until which the deposit is valid.
    pub till: u32,
}

impl Deposit {
    /// Creates a new deposit state.
    pub fn new(amount: BigInt, till: u32) -> Self {
        Self { amount, till }
    }
}

impl IInteroperable for Deposit {
    fn from_stack_item(&mut self, stack_item: StackItem) -> std::result::Result<(), CoreError> {
        let StackItem::Struct(struct_item) = stack_item else {
            return Ok(());
        };
        let items = struct_item.items();
        if items.len() < 2 {
            return Ok(());
        }

        if let Some(amount) = extract_int(&items[0]) {
            self.amount = amount;
        }

        if let Some(till) = extract_u32(&items[1]) {
            self.till = till;
        }
        Ok(())
    }

    fn to_stack_item(&self) -> std::result::Result<StackItem, CoreError> {
        Ok(StackItem::from_struct(vec![
            StackItem::from_int(self.amount.clone()),
            StackItem::from_int(self.till),
        ]))
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

/// Serializes a Deposit to bytes (matching C# StorageItem format).
pub(super) fn serialize_deposit(deposit: &Deposit) -> Vec<u8> {
    let amount_bytes = deposit.amount.to_signed_bytes_le();
    let mut result = Vec::with_capacity(1 + amount_bytes.len() + 4);
    result.push(amount_bytes.len() as u8);
    result.extend_from_slice(&amount_bytes);
    result.extend_from_slice(&deposit.till.to_le_bytes());
    result
}

/// Deserializes a Deposit from bytes.
pub(super) fn deserialize_deposit(data: &[u8]) -> Result<Deposit> {
    if data.is_empty() {
        return Err(Error::native_contract("Empty deposit data"));
    }
    let amount_len = data[0] as usize;
    if data.len() < 1 + amount_len + 4 {
        return Err(Error::native_contract(
            "Invalid deposit data length".to_string(),
        ));
    }
    let amount_bytes = &data[1..1 + amount_len];
    let amount = BigInt::from_signed_bytes_le(amount_bytes);
    let till_bytes = &data[1 + amount_len..1 + amount_len + 4];
    let till = u32::from_le_bytes([till_bytes[0], till_bytes[1], till_bytes[2], till_bytes[3]]);
    Ok(Deposit::new(amount, till))
}
