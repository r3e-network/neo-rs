// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::types::{Network, H160};

pub mod nep2;
pub mod nep6;

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
