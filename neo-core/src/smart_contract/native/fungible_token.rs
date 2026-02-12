//! Fungible token base trait implementation.
//!
//! This module provides the base trait for NEP-17 compatible tokens,
//! equivalent to the C# FungibleToken\<TState> abstract class.
//!
//! Both [`GasToken`](super::gas_token::GasToken) and
//! [`NeoToken`](super::neo_token::NeoToken) implement this trait, which
//! captures the shared NEP-17 surface: symbol, decimals, total supply
//! queries, balance queries, transfer event emission, and common
//! byte-encoding helpers.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::native_contract::NativeMethod;
use crate::smart_contract::native::NativeContract;
use crate::smart_contract::ContractParameterType;
use crate::UInt160;
use neo_vm::StackItem;
use num_bigint::BigInt;

/// Prefix for storing total supply in storage.
pub const PREFIX_TOTAL_SUPPLY: u8 = 11;

/// Prefix for storing account states in storage.
pub const PREFIX_ACCOUNT: u8 = 20;

/// Base trait for fungible tokens compatible with NEP-17.
///
/// Equivalent to C# `FungibleToken<TState>` abstract class.  Implementors
/// must provide the token-specific constants (`ft_symbol`, `ft_decimals`)
/// and snapshot-level queries (`ft_total_supply`, `ft_balance_of`).
///
/// Default methods supply the shared encoding helpers, Transfer event
/// emission, argument parsing, and NEP-17 method descriptor generation
/// that would otherwise be copy-pasted across GasToken and NeoToken.
pub trait FungibleToken: NativeContract {
    // ------------------------------------------------------------------
    // Required: token-specific constants / queries
    // ------------------------------------------------------------------

    /// The symbol of the token (e.g. "GAS", "NEO").
    fn ft_symbol(&self) -> &str;

    /// The number of decimal places (e.g. 8 for GAS, 0 for NEO).
    fn ft_decimals(&self) -> u8;

    /// Reads the total supply from the current engine snapshot.
    fn ft_total_supply(&self, engine: &ApplicationEngine) -> Result<BigInt>;

    /// Reads the balance of `account` from the current engine snapshot.
    fn ft_balance_of(&self, engine: &ApplicationEngine, account: &UInt160) -> Result<BigInt>;

    // ------------------------------------------------------------------
    // Provided: shared helpers
    // ------------------------------------------------------------------

    /// The display factor: `10^decimals`.
    fn ft_factor(&self) -> BigInt {
        BigInt::from(10).pow(self.ft_decimals() as u32)
    }

    /// Encodes a `BigInt` amount to little-endian signed bytes (C# compatible).
    fn ft_encode_amount(value: &BigInt) -> Vec<u8>
    where
        Self: Sized,
    {
        let mut bytes = value.to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    /// Decodes little-endian signed bytes to a `BigInt`.
    fn ft_decode_amount(data: &[u8]) -> BigInt
    where
        Self: Sized,
    {
        BigInt::from_signed_bytes_le(data)
    }

    /// Parses a 20-byte account argument into a `UInt160`.
    fn ft_read_account(data: &[u8]) -> Result<UInt160>
    where
        Self: Sized,
    {
        if data.len() != 20 {
            return Err(Error::native_contract(
                "Account argument must be 20 bytes".to_string(),
            ));
        }
        UInt160::from_bytes(data).map_err(|err| Error::native_contract(err.to_string()))
    }

    /// Builds the storage key suffix for an account balance.
    fn ft_account_storage_key(&self, account: &UInt160) -> Vec<u8> {
        let mut key = vec![PREFIX_ACCOUNT];
        key.extend_from_slice(&account.as_bytes());
        key
    }

    /// Builds the storage key suffix for total supply.
    fn ft_total_supply_storage_key(&self) -> Vec<u8> {
        vec![PREFIX_TOTAL_SUPPLY]
    }

    /// Emits a NEP-17 `Transfer` event.
    ///
    /// This is the canonical implementation matching C#
    /// `FungibleToken.PostTransferAsync`.  Both GasToken and NeoToken
    /// emit the event in exactly the same way.
    fn ft_emit_transfer(
        &self,
        engine: &mut ApplicationEngine,
        from: Option<&UInt160>,
        to: Option<&UInt160>,
        amount: &BigInt,
    ) -> Result<()> {
        let from_item = from
            .map(|addr| StackItem::from_byte_string(addr.to_bytes()))
            .unwrap_or_else(StackItem::null);
        let to_item = to
            .map(|addr| StackItem::from_byte_string(addr.to_bytes()))
            .unwrap_or_else(StackItem::null);
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

    /// Validates basic transfer parameters (non-negative amount).
    fn ft_validate_amount(amount: &BigInt) -> Result<()>
    where
        Self: Sized,
    {
        if amount < &BigInt::from(0) {
            return Err(Error::native_contract(
                "Transfer amount cannot be negative".to_string(),
            ));
        }
        Ok(())
    }

    /// Generates the five standard NEP-17 method descriptors that every
    /// fungible token must expose: `symbol`, `decimals`, `totalSupply`,
    /// `balanceOf`, and `transfer`.
    ///
    /// Token implementations can call this from their constructor and
    /// append any token-specific methods afterwards.
    fn ft_nep17_methods() -> Vec<NativeMethod>
    where
        Self: Sized,
    {
        vec![
            NativeMethod::safe(
                "symbol".to_string(),
                0,
                Vec::new(),
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "decimals".to_string(),
                0,
                Vec::new(),
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "totalSupply".to_string(),
                1 << 15,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(crate::smart_contract::call_flags::CallFlags::READ_STATES),
            NativeMethod::safe(
                "balanceOf".to_string(),
                1 << 15,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Integer,
            )
            .with_required_call_flags(crate::smart_contract::call_flags::CallFlags::READ_STATES)
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::unsafe_method(
                "transfer".to_string(),
                1 << 17,
                crate::smart_contract::call_flags::CallFlags::ALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Boolean,
            )
            .with_storage_fee(50)
            .with_parameter_names(vec![
                "from".to_string(),
                "to".to_string(),
                "amount".to_string(),
                "data".to_string(),
            ]),
        ]
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
    /// The balance of the account.
    pub balance: BigInt,
    /// The last block height at which the balance was updated.
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
