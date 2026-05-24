use crate::error::{CoreError, CoreError as Error, CoreResult as Result};
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

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

    fn stack_value_to_bigint(value: &StackValue) -> Option<BigInt> {
        match value {
            StackValue::Integer(value) => Some(BigInt::from(*value)),
            StackValue::Boolean(value) => Some(BigInt::from(i32::from(*value))),
            StackValue::BigInteger(bytes) => Some(BigInt::from_signed_bytes_le(bytes)),
            StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() <= 32 => {
                Some(BigInt::from_signed_bytes_le(bytes))
            }
            _ => None,
        }
    }

    fn stack_value_to_u32(value: &StackValue) -> Option<u32> {
        match value {
            StackValue::Integer(value) => u32::try_from(*value).ok(),
            StackValue::Boolean(value) => Some(u32::from(*value)),
            StackValue::BigInteger(bytes) => BigInt::from_signed_bytes_le(bytes).to_u32(),
            StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() <= 32 => {
                BigInt::from_signed_bytes_le(bytes).to_u32()
            }
            _ => None,
        }
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            StackValue::BigInteger(self.amount.to_signed_bytes_le()),
            StackValue::Integer(i64::from(self.till)),
        ])
    }

    /// Updates this deposit from a neo-vm-rs stack value.
    pub fn from_stack_value(
        &mut self,
        stack_value: StackValue,
    ) -> std::result::Result<(), CoreError> {
        if let StackValue::Struct(items) = stack_value {
            if items.len() < 2 {
                return Ok(());
            }

            if let Some(amount) = Self::stack_value_to_bigint(&items[0]) {
                self.amount = amount;
            }

            if let Some(till) = Self::stack_value_to_u32(&items[1]) {
                self.till = till;
            }
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_serialization_roundtrips() {
        let deposit = Deposit::new(BigInt::from(1000000000i64), 12345);
        let data = serialize_deposit(&deposit);
        let deserialized = deserialize_deposit(&data).unwrap();
        assert_eq!(deserialized.amount, deposit.amount);
        assert_eq!(deserialized.till, deposit.till);
    }

    #[test]
    fn deposit_projects_to_neo_vm_rs_stack_value() {
        let deposit = Deposit::new(BigInt::from(500), 100);

        assert_eq!(
            deposit.to_stack_value(),
            StackValue::Struct(vec![
                StackValue::BigInteger(BigInt::from(500).to_signed_bytes_le()),
                StackValue::Integer(100),
            ])
        );
    }

    #[test]
    fn deposit_reads_from_neo_vm_rs_stack_value() {
        let mut deposit = Deposit::default();
        let amount = BigInt::from(987_654_321i64);

        deposit
            .from_stack_value(StackValue::Struct(vec![
                StackValue::BigInteger(amount.to_signed_bytes_le()),
                StackValue::Integer(222),
            ]))
            .unwrap();

        assert_eq!(deposit.amount, amount);
        assert_eq!(deposit.till, 222);
    }
}
