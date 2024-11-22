// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_type::{Network, H160};


mod nep6_account;
mod nep6_contract;
mod nep6_wallet;
mod nep6_wallet_factory;
mod scrypt_parameters;
mod nep2;
mod nep6;
mod wallet_error;

pub use nep6_account::NEP6Account;
pub use nep6_contract::NEP6Contract;
pub use nep6_wallet::NEP6Wallet;
pub use nep6_wallet_factory::NEP6WalletFactory;
pub use scrypt_parameters::ScryptParameters;
pub use wallet_error::*;


mod helper;
mod iwallet_factory;
mod iwallet_provider;
mod key_pair;
mod transfer_output;
mod wallet;
mod asset_descriptor;
mod wallet_account;

pub type Account = NEP6Account;

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
