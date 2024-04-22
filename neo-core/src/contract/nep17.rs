// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::math::Uint256;
use crate::contract::param::Param;
use crate::types::H160;

/// NEP-17 is a Fungible-Token Contract
pub trait Nep17 {
    const TOTAL_SUPPLY: &'static str = "totalSupply";
    const SYMBOL: &'static str = "symbol";
    const DECIMALS: &'static str = "decimals";

    type TransferError;

    fn symbol(&self) -> &'static str;

    fn decimals(&self) -> u8;

    fn total_supply(&self) -> Uint256;

    fn balance_of(&self, account: &H160) -> Uint256;

    fn transfer(&self, from: &H160, to: &H160, amount: &Uint256, data: &Param) -> Result<bool, Self::TransferError>;
}

/// A triggered event when the `transfer` of a Nep17 contract is called.
pub trait Nep17Event {
    const TRANSFER: &'static str = "Transfer";

    type EmitError;

    fn emit_transfer(&self, from: &H160, to: &H160, amount: &Uint256) -> Result<(), Self::EmitError>;
}