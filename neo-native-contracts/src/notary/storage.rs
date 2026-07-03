//! Notary deposit storage codecs and pure deposit decisions.

use super::{
    DEFAULT_DEPOSIT_DELTA_TILL, Notary, PREFIX_DEPOSIT, PREFIX_MAX_NOT_VALID_BEFORE_DELTA,
};
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_serialization::BinarySerializer;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// C# `Notary.Deposit`: `Struct[Amount, Till]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::notary) struct DepositState {
    amount: BigInt,
    till: u32,
}

impl DepositState {
    pub(in crate::notary) fn new(amount: BigInt, till: u32) -> Self {
        Self { amount, till }
    }

    pub(in crate::notary) fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::BigInteger(self.amount.to_signed_bytes_le()),
                StackValue::Integer(i64::from(self.till)),
            ],
        )
    }

    pub(in crate::notary) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Struct(_, items) = stack_value else {
            return Err(CoreError::invalid_data("Notary deposit is not a struct"));
        };
        let amount_value = items
            .first()
            .ok_or_else(|| CoreError::invalid_data("Notary deposit Amount missing"))?;
        let amount = neo_vm::stack_value_as_bigint(amount_value)
            .map_err(|e| CoreError::invalid_data(format!("Notary deposit Amount: {e}")))?;
        let till = items
            .get(1)
            .and_then(neo_vm_rs::stack_value_as_u32)
            .ok_or_else(|| CoreError::invalid_data("Notary deposit Till out of range"))?;
        Ok(Self { amount, till })
    }
}

neo_vm::impl_interoperable_via_stack_value!(DepositState);

impl Notary {
    /// Reads field `index` of the C# `Deposit` struct (`[Amount, Till]`) stored under
    /// `Prefix_Deposit ++ account`, returning 0 when the account has no deposit.
    /// `balanceOf` reads `Amount` (index 0); `expirationOf` reads `Till` (index 1).
    pub(in crate::notary) fn read_deposit_field(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
        index: usize,
    ) -> CoreResult<BigInt> {
        let key = Self::deposit_key(account);
        let Some(item) = snapshot.get(&key) else {
            return Ok(BigInt::from(0));
        };
        let bytes = item.value_bytes();
        let (amount, till) = Self::decode_deposit(bytes.as_ref())?;
        match index {
            0 => Ok(amount),
            1 => Ok(BigInt::from(till)),
            _ => Err(CoreError::invalid_data("Notary deposit field is missing")),
        }
    }

    /// The deposit storage key `(Notary.ID, [Prefix_Deposit, account])`.
    pub(in crate::notary) fn deposit_key(account: &UInt160) -> StorageKey {
        crate::keys::prefixed_hash160_key(Self::ID, PREFIX_DEPOSIT, account)
    }

    /// The max-not-valid-before-delta setting key
    /// `(Notary.ID, [Prefix_MaxNotValidBeforeDelta])`.
    pub(crate) fn max_not_valid_before_delta_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_MAX_NOT_VALID_BEFORE_DELTA, &[])
    }

    pub(in crate::notary) fn decode_deposit(bytes: &[u8]) -> CoreResult<(BigInt, u32)> {
        let limits = ExecutionEngineLimits::default();
        let state = BinarySerializer::deserialize_stack_value_with_limits(
            bytes,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("Notary deposit: {e}")))?;
        let deposit = DepositState::from_stack_value(state)
            .map_err(|e| CoreError::invalid_data(format!("Notary deposit: {e}")))?;
        Ok((deposit.amount, deposit.till))
    }

    /// Reads the full `Deposit` `(Amount, Till)` for `account`, or `None` when the
    /// account has no deposit (C# `GetDepositFor` returning null).
    pub(in crate::notary) fn read_deposit(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
    ) -> CoreResult<Option<(BigInt, u32)>> {
        let Some(item) = snapshot.get(&Self::deposit_key(account)) else {
            return Ok(None);
        };
        let bytes = item.value_bytes();
        Self::decode_deposit(bytes.as_ref()).map(Some)
    }

    /// Deletes the deposit entry for `account` (C# `RemoveDepositFor`).
    pub(in crate::notary) fn delete_deposit(&self, snapshot: &DataCache, account: &UInt160) {
        snapshot.delete(&Self::deposit_key(account));
    }

    /// Writes the `Deposit` `(Amount, Till)` struct for `account` (C# `PutDepositFor`):
    /// the BinarySerialized `Struct[Amount, Till]`.
    pub(in crate::notary) fn write_deposit(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
        amount: &BigInt,
        till: u32,
    ) -> CoreResult<()> {
        let item = DepositState::new(amount.clone(), till).to_stack_value();
        let bytes = BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::serialization(format!("Notary deposit serialize: {e}")))?;
        snapshot.update(Self::deposit_key(account), StorageItem::from_bytes(bytes));
        Ok(())
    }

    /// Pure decision core of C# `LockDepositUntil` (after the witness check): returns
    /// `Some((amount, till))` to write, or `None` to return `false`. The new `till`
    /// must be at least `current_index + 2` (so the deposit outlives the next block)
    /// and at least the deposit's existing `Till` (locks cannot be shortened), and a
    /// deposit must already exist. `wrapping_add` matches C#'s unchecked `uint` math.
    pub(in crate::notary) fn lock_deposit_decision(
        current_index: u32,
        deposit: Option<(BigInt, u32)>,
        till: u32,
    ) -> Option<(BigInt, u32)> {
        if till < current_index.wrapping_add(2) {
            return None;
        }
        let (amount, existing_till) = deposit?;
        if till < existing_till {
            return None;
        }
        Some((amount, till))
    }

    /// Parses the `onNEP17Payment` `data` argument (an `Any` param that arrives
    /// BinarySerialized). C# requires it to be an `Array` of exactly 2 elements:
    /// `[to, till]` where `to` is `Null` (use the GAS sender `from`) or a `UInt160`,
    /// and `till` is the requested lock height.
    pub(in crate::notary) fn parse_onnep17_data(
        from: &UInt160,
        data: &[u8],
    ) -> CoreResult<(UInt160, u32)> {
        let limits = ExecutionEngineLimits::default();
        let item = BinarySerializer::deserialize_stack_value_with_limits(
            data,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::invalid_operation(format!("Notary::onNEP17Payment data: {e}")))?;
        let StackValue::Array(_, items) = item else {
            return Err(CoreError::invalid_operation(
                "Notary::onNEP17Payment data must be an array of 2 elements",
            ));
        };
        if items.len() != 2 {
            return Err(CoreError::invalid_operation(
                "Notary::onNEP17Payment data must be an array of 2 elements",
            ));
        }
        let to = if matches!(items[0], StackValue::Null) {
            *from
        } else {
            let bytes = items[0].to_byte_string_bytes().ok_or_else(|| {
                CoreError::invalid_operation("Notary::onNEP17Payment to: cannot convert to bytes")
            })?;
            crate::args::bytes_to_hash160(&bytes, "Notary::onNEP17Payment to: bad hash")?
        };
        let till_value = items[1].to_i128().ok_or_else(|| {
            CoreError::invalid_operation("Notary::onNEP17Payment till: cannot convert to integer")
        })?;
        let till = u32::try_from(till_value).map_err(|_| {
            CoreError::invalid_operation("Notary::onNEP17Payment till out of uint range")
        })?;
        Ok((to, till))
    }

    /// The pure deposit decision of C# `Notary.OnNEP17Payment` (after the GAS-caller
    /// and data checks). Returns the `(Amount, Till)` to write, or an error string
    /// describing the C# fault. `existing` is the current deposit for `to`
    /// (`None` = first deposit). `wrapping_add` matches C#'s unchecked `uint` math.
    pub(in crate::notary) fn compute_deposit(
        existing: Option<(BigInt, u32)>,
        amount: &BigInt,
        till: u32,
        allowed_change_till: bool,
        current_height: u32,
        fee_per_key: i64,
    ) -> Result<(BigInt, u32), &'static str> {
        if till < current_height.wrapping_add(2) {
            return Err("`till` is below the chain height + 2");
        }
        match existing {
            Some((existing_amount, existing_till)) => {
                if till < existing_till {
                    return Err("`till` is below the previous deposit Till");
                }
                // An existing deposit only adopts the requested `till` when the GAS
                // sender is the deposit owner; otherwise the lock height is unchanged.
                let final_till = if allowed_change_till {
                    till
                } else {
                    existing_till
                };
                Ok((existing_amount + amount, final_till))
            }
            None => {
                // First deposit must be at least 2 * the NotaryAssisted attribute fee.
                let minimum = BigInt::from(2) * BigInt::from(fee_per_key);
                if amount < &minimum {
                    return Err("first deposit is below 2 * the NotaryAssisted fee");
                }
                let final_till = if allowed_change_till {
                    till
                } else {
                    current_height.wrapping_add(DEFAULT_DEPOSIT_DELTA_TILL)
                };
                Ok((amount.clone(), final_till))
            }
        }
    }

    /// C# `SetMaxNotValidBeforeDelta` storage effect: overwrite
    /// `Prefix_MaxNotValidBeforeDelta` (`GetAndChange(...).Set(value)`). The key is
    /// genesis-initialised (`OnPersist` Add), so `update` (= C# GetAndChange) is the
    /// correct primitive.
    pub(in crate::notary) fn put_max_not_valid_before_delta(
        &self,
        snapshot: &DataCache,
        value: i64,
    ) {
        snapshot.update(
            Self::max_not_valid_before_delta_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
        );
    }

    /// C# `GetMaxNotValidBeforeDelta` directly indexes the initialized setting
    /// storage item; a missing key faults instead of silently using the default.
    pub(in crate::notary) fn read_max_not_valid_before_delta(
        &self,
        snapshot: &DataCache,
    ) -> CoreResult<i64> {
        let key = Self::max_not_valid_before_delta_key();
        let Some(item) = snapshot.get(&key) else {
            return Err(CoreError::invalid_data(
                "Notary MaxNotValidBeforeDelta is missing",
            ));
        };
        BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_i64()
            .ok_or_else(|| {
                CoreError::invalid_operation("Notary MaxNotValidBeforeDelta out of range")
            })
    }

    /// Parses the leading `Hash160` account argument for the deposit reads.
    pub(in crate::notary) fn parse_account(args: &[Vec<u8>], method: &str) -> CoreResult<UInt160> {
        crate::args::raw_account(args, &format!("Notary::{method}"))
    }
}
