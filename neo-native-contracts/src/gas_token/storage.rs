//! GAS account storage helpers.
//!
//! GAS uses the shared NEP-17 account layout (`Struct[Balance]`) and total
//! supply keying. This module keeps those codecs and state-only fast-forward
//! minting separate from transfer dispatch and block-persist logic.

use super::GasToken;
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use num_bigint::BigInt;
use num_traits::Zero;

impl GasToken {
    pub(crate) fn account_key(account: &UInt160) -> StorageKey {
        crate::nep17_account_key(Self::ID, account)
    }

    pub(crate) fn total_supply_key() -> StorageKey {
        crate::nep17_total_supply_key(Self::ID)
    }

    pub(crate) fn total_supply(snapshot: &DataCache) -> BigInt {
        crate::read_nep17_total_supply(snapshot, Self::ID)
    }

    /// State-only GAS mint used by state-equivalent empty-block fast-forward.
    ///
    /// This is the storage half of [`Self::gas_mint`] with `call_on_payment =
    /// false` and without `Transfer` notifications. It is valid only for paths
    /// that explicitly skip replay artifacts/events and have already proven that
    /// no deployed contract callback can run. A zero amount is a no-op, matching
    /// `FungibleToken.Mint`.
    pub fn fast_forward_mint_state(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
        amount: &BigInt,
    ) -> CoreResult<()> {
        if amount < &BigInt::zero() {
            return Err(CoreError::invalid_operation(
                "GasToken::fast_forward_mint_state: amount cannot be negative",
            ));
        }
        if amount.is_zero() {
            return Ok(());
        }
        let balance = self
            .read_gas_account(snapshot, account)?
            .unwrap_or_else(BigInt::zero)
            + amount;
        self.write_gas_account(snapshot, account, &balance)?;
        let supply_key = Self::total_supply_key();
        let supply = Self::total_supply(snapshot) + amount;
        snapshot.update(
            supply_key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)),
        );
        Ok(())
    }

    /// Reads the GAS account balance, or `None` when the account has no entry. The
    /// GAS account state is the base `FungibleToken.AccountState` = `Struct[Balance]`
    /// (a single field), so `read_nep17_balance`'s field 0 is the balance.
    pub(crate) fn read_gas_account(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
    ) -> CoreResult<Option<BigInt>> {
        let Some(item) = snapshot.get(&Self::account_key(account)) else {
            return Ok(None);
        };
        let state = crate::deserialize_account_state(item.value_bytes().as_ref())?;
        Ok(Some(state.balance))
    }

    /// Writes the GAS account state `Struct[Balance]` (C# `GetAndChange(...).Set`).
    pub(crate) fn write_gas_account(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
        balance: &BigInt,
    ) -> CoreResult<()> {
        let state = crate::AccountState::new(balance.clone());
        let bytes = crate::serialize_account_state(&state)?;
        snapshot.update(Self::account_key(account), StorageItem::from_bytes(bytes));
        Ok(())
    }

    /// Deletes the GAS account entry (C# `Delete(keyFrom)` when a balance reaches 0).
    pub(crate) fn delete_gas_account(&self, snapshot: &DataCache, account: &UInt160) {
        snapshot.delete(&Self::account_key(account));
    }

    /// C# `NativeContract.GAS.BalanceOf(snapshot, account)`: reads the `Balance`
    /// field of the NEP-17 `AccountState` stored under `Prefix_Account + account`
    /// (zero when absent). The single canonical GAS-balance decode, shared by
    /// the mempool fee check and RPC wallet helpers.
    pub fn balance_of(snapshot: &DataCache, account: &UInt160) -> CoreResult<BigInt> {
        crate::read_nep17_balance(snapshot, Self::ID, account)
    }
}
