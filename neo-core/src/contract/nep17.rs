// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::math::U256;
use crate::{contract::Param, types::UInt160};

/// NEP-17 is a Fungible-Token Contract
pub trait Nep17 {
    const TOTAL_SUPPLY: &'static str = "totalSupply";
    const SYMBOL: &'static str = "symbol";
    const DECIMALS: &'static str = "decimals";

    type TransferError;

    fn symbol(&self) -> &'static str;

    fn decimals(&self) -> u8;

    fn total_supply(&self) -> U256;

    fn balance_of(&self, account: &UInt160) -> U256;

    fn transfer(&self, from: &UInt160, to: &UInt160, amount: &U256, data: &Param) -> Result<bool, Self::TransferError>;
}

/// A triggered event when the `transfer` of a Nep17 contract is called.
pub trait Nep17Event {
    const TRANSFER: &'static str = "Transfer";

    type EmitError;

    fn emit_transfer(&self, from: &UInt160, to: &UInt160, amount: &U256) -> Result<(), Self::EmitError>;
}