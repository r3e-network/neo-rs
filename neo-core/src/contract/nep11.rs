// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::math::U256;

use crate::contract::ParamValue;
use crate::types::{Bytes, H160};

pub trait Iter<T> {
    type Item;
    type Error;

    fn next(&mut self) -> Option<Result<Self::Item, Self::Error>>;
}

pub trait Nep11 {
    const TOTAL_SUPPLY: &'static str = "totalSupply";
    const SYMBOL: &'static str = "symbol";
    const DECIMALS: &'static str = "decimals";

    const TRANSFER: &'static str = "transfer";
    const TOKENS: &'static str = "tokens";
    const OWNER_OF: &'static str = "ownerOf";
    const TOKENS_OF: &'static str = "tokensOf";
    const BALANCE_OF: &'static str = "balanceOf";

    const PROPERTIES: &'static str = "properties";

    type TransferError;

    fn symbol(&self) -> &'static str;

    fn decimals(&self) -> u8;

    fn total_supply(&self) -> U256;

    /// `properties` is optional
    fn properties(&self) -> Bytes;

    fn balance_of(&self, owner: &H160) -> U256;

    fn tokens_of<TokenIter: Iter<Bytes>>(&self, owner: &H160) -> TokenIter;

    fn owner_of(&self, token_id: &Bytes) -> H160;

    fn transfer(
        &self,
        to: &H160,
        token_id: &Bytes,
        data: &ParamValue,
    ) -> Result<bool, Self::TransferError>;

    /// `transfer_token` for divisible token
    fn transfer_token(
        &self,
        from: &H160,
        to: &H160,
        amount: u64,
        token_id: &Bytes,
        data: &ParamValue,
    ) -> Result<bool, Self::TransferError>;

    /// `owner_of_token` returns multi owners if this NFT is divided
    fn owners_of_token<OwnerIter: Iter<H160>>(&self, token_id: &Bytes) -> OwnerIter;

    /// `balance_of_token` is for divisible NFT
    fn balance_of_token(&self, owner: &H160, token_id: &Bytes) -> u64;

    /// `tokens` is optional
    fn tokens<TokenIter: Iter<Bytes>>(&self) -> TokenIter;
}

pub trait Nep11Receiver {
    const ON_NEP11_PAYMENT: &'static str = "onNEP11Payment";

    type Error;

    fn on_nep11_payment(
        &self,
        from: &H160,
        amount: u64,
        token_id: &Bytes,
        data: &ParamValue,
    ) -> Result<(), Self::Error>;
}

pub trait Nep11Event {
    const TRANSFER: &'static str = "Transfer";

    type EmitError;

    fn emit_transfer(
        &self,
        from: &H160,
        to: &H160,
        amount: u64,
        token_id: &Bytes,
    ) -> Result<(), Self::EmitError>;
}
