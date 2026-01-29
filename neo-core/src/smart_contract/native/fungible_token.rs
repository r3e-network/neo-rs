//! Fungible token base trait implementation.
//!
//! This module provides the base trait for NEP-17 compatible tokens,
//! equivalent to the C# FungibleToken\<TState> abstract class.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::NativeContract;
use crate::UInt160;
use neo_vm::StackItem;
use num_bigint::BigInt;

/// Prefix for storing total supply in storage.
pub const PREFIX_TOTAL_SUPPLY: u8 = 11;

/// Prefix for storing account states in storage.
pub const PREFIX_ACCOUNT: u8 = 20;

/// Base trait for fungible tokens compatible with NEP-17.
/// Equivalent to C# FungibleToken\<TState> abstract class.
pub trait FungibleToken: NativeContract {
    /// The symbol of the token.
    fn symbol(&self) -> &str;

    /// The number of decimal places of the token.
    fn decimals(&self) -> u8;

    /// The factor used when calculating the displayed value.
    fn factor(&self) -> BigInt {
        BigInt::from(10).pow(self.decimals() as u32)
    }

    /// Gets the total supply of the token.
    fn total_supply(&self, engine: &ApplicationEngine) -> Result<BigInt>;

    /// Gets the balance of an account.
    fn balance_of(&self, engine: &ApplicationEngine, account: &UInt160) -> Result<BigInt>;

    /// Transfers tokens between accounts.
    fn transfer(
        &self,
        engine: &mut ApplicationEngine,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        call_on_payment: bool,
    ) -> Result<bool>;

    /// Mints new tokens to an account.
    fn mint(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
        call_on_payment: bool,
    ) -> Result<()>;

    /// Burns tokens from an account.
    fn burn(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
    ) -> Result<()>;

    /// Gets the storage key for an account.
    fn create_account_storage_key(&self, account: &UInt160) -> Vec<u8> {
        let mut key = vec![PREFIX_ACCOUNT];
        key.extend_from_slice(&account.as_bytes());
        key
    }

    /// Gets the storage key for total supply.
    fn create_total_supply_storage_key(&self) -> Vec<u8> {
        vec![PREFIX_TOTAL_SUPPLY]
    }

    /// Emits a Transfer event.
    fn emit_transfer_event(
        &self,
        engine: &mut ApplicationEngine,
        from: Option<&UInt160>,
        to: Option<&UInt160>,
        amount: &BigInt,
    ) -> Result<()> {
        let from_item = match from {
            Some(addr) => StackItem::from_byte_string(addr.to_bytes()),
            None => StackItem::Null,
        };
        let to_item = match to {
            Some(addr) => StackItem::from_byte_string(addr.to_bytes()),
            None => StackItem::Null,
        };
        let amount_item = StackItem::from_int(amount.clone());

        engine
            .send_notification(
                self.hash(),
                "Transfer".to_string(),
                vec![from_item, to_item, amount_item],
            )
            .map_err(Error::native_contract)?;

        Ok(())
    }

    /// Validates transfer parameters.
    fn validate_transfer_params(
        &self,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
    ) -> Result<()> {
        if amount < &BigInt::from(0) {
            return Err(Error::native_contract(
                "Transfer amount cannot be negative".to_string(),
            ));
        }

        if from == to {
            return Err(Error::native_contract(
                "Cannot transfer to same account".to_string(),
            ));
        }

        Ok(())
    }

    /// Called when balance is changing (hook for subclasses).
    fn on_balance_changing(
        &self,
        _engine: &mut ApplicationEngine,
        _account: &UInt160,
        _old_balance: &BigInt,
        _new_balance: &BigInt,
    ) -> Result<()> {
        // Default implementation does nothing
        // Subclasses can override for custom logic
        Ok(())
    }
}

/// Account state for fungible tokens.
pub trait TokenAccountState: Clone + Default {
    /// Gets the balance of this account.
    fn balance(&self) -> &BigInt;

    /// Sets the balance of this account.
    fn set_balance(&mut self, balance: BigInt);

    /// Gets the last update height.
    fn last_update_height(&self) -> u32;

    /// Sets the last update height.
    fn set_last_update_height(&mut self, height: u32);
}

/// Default implementation of token account state.
#[derive(Clone, Debug, Default)]
pub struct DefaultTokenAccountState {
    pub balance: BigInt,
    pub last_update_height: u32,
}

impl TokenAccountState for DefaultTokenAccountState {
    fn balance(&self) -> &BigInt {
        &self.balance
    }

    fn set_balance(&mut self, balance: BigInt) {
        self.balance = balance;
    }

    fn last_update_height(&self) -> u32 {
        self.last_update_height
    }

    fn set_last_update_height(&mut self, height: u32) {
        self.last_update_height = height;
    }
}
