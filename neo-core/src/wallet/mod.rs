// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_type::{Network, H160};

pub mod nep6;
mod helper;
mod iwallet_factory;
mod iwallet_provider;
mod key_pair;
mod transfer_output;
mod wallet;
mod wallet_account;
mod asset_descriptor;

pub type Account = nep6::Account;

pub trait AccountHolder {
    fn get_account(&self, script_hash: &H160) -> Option<Account>;
}

pub trait Wallet: AccountHolder {
    type CreateError;

    fn network(&self) -> Network;

    fn create_account(
        &mut self,
        name: &str,
        passphrase: &[u8],
    ) -> Result<&Account, Self::CreateError>;

    fn delete_account(&self, script_hash: &H160) -> bool;

    fn change_password(&mut self, old: &[u8], new: &[u8]) -> bool;
}
